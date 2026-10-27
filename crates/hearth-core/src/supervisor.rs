//! Runs the programs Hearth needs (Caddy, PHP FastCGI pools) and keeps them running: a crashed
//! process is restarted with a growing delay, and every child stops with Hearth (a Windows Job
//! Object kills them even when Hearth itself crashes; on macOS each child has its own process group).

use std::collections::HashMap;
use std::ffi::OsString;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::CoreError;

/// Delays before the 1st, 2nd, ... restart; the last one repeats.
const BACKOFF: [u64; 6] = [1, 2, 4, 8, 16, 30];
/// A process that stayed up this long starts over at the shortest delay.
const STABLE: Duration = Duration::from_secs(60);
const READY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    /// Identifies the process ("caddy", "php-8.4-0").
    pub key: String,
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, OsString)>,
    pub cwd: Option<PathBuf>,
    /// stdout and stderr are appended here.
    pub log: PathBuf,
    /// The process counts as started once this port accepts connections.
    pub ready_port: Option<u16>,
    /// Or: the process counts as started once this socket file exists.
    pub ready_socket: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStatus {
    pub key: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub last_error: Option<String>,
}

struct Entry {
    spec: ProcessSpec,
    child: Option<Child>,
    restarts: u32,
    last_error: Option<String>,
    started: Instant,
    retry_at: Option<Instant>,
    backoff_step: usize,
}

type Table = Arc<Mutex<HashMap<String, Entry>>>;

pub struct Supervisor {
    table: Table,
    stop: Arc<AtomicBool>,
    keep_children: AtomicBool,
    /// `<run>/children.json`: the running children, so the next start can stop leftovers (macOS).
    record: Option<PathBuf>,
    #[cfg(windows)]
    job: Arc<job::Job>,
}

/// Writes the running children (key, pid, program) to the record file.
fn persist(table: &HashMap<String, Entry>, record: &Option<PathBuf>) {
    let Some(path) = record else { return };
    let children: Vec<(String, u32, String)> = table
        .values()
        .filter_map(|e| e.child.as_ref().map(|c| (e.spec.key.clone(), c.id(), e.spec.program.display().to_string())))
        .collect();
    if let Ok(bytes) = serde_json::to_vec(&children) {
        let _ = crate::atomic::write(path, &bytes);
    }
}

/// Stops children a crashed Hearth left running (macOS; on Windows the Job Object already did).
/// Only processes that still run the recorded program are touched, never a reused pid.
#[cfg(unix)]
fn stop_leftovers(record: &std::path::Path) {
    let Ok(bytes) = std::fs::read(record) else { return };
    let children: Vec<(String, u32, String)> = serde_json::from_slice(&bytes).unwrap_or_default();
    for (_, pid, program) in children {
        let name = std::path::Path::new(&program).file_name().map(|n| n.to_string_lossy().into_owned());
        let running = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if running.is_empty() || name.is_none_or(|n| !running.ends_with(&n)) {
            continue;
        }
        // SAFETY: plain signal calls on that child's own process group.
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
        std::thread::sleep(Duration::from_millis(500));
        unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    }
    let _ = std::fs::remove_file(record);
}

fn answers(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(150)).is_ok()
}

/// The last lines of a log, for start errors.
pub(crate) fn log_tail(path: &std::path::Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" | ")
}

impl Supervisor {
    /// A supervisor that keeps no record of its children (tests).
    pub fn new() -> Result<Supervisor, CoreError> {
        Self::new_in(None)
    }

    /// A supervisor recording its children in `<run>/children.json`; on macOS it first stops the
    /// children a crashed earlier Hearth left there.
    pub fn new_in(run: Option<PathBuf>) -> Result<Supervisor, CoreError> {
        let record = run.map(|dir| dir.join("children.json"));
        #[cfg(unix)]
        if let Some(path) = &record {
            stop_leftovers(path);
        }
        let table: Table = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        #[cfg(windows)]
        let job =
            Arc::new(job::Job::new().map_err(|e| CoreError::Php(format!("Could not set up child processes: {e}")))?);
        let (t, s, r) = (table.clone(), stop.clone(), record.clone());
        #[cfg(windows)]
        let j = job.clone();
        std::thread::Builder::new()
            .name("hearth-supervisor".into())
            .spawn(move || {
                while !s.load(Ordering::Relaxed) {
                    #[cfg(windows)]
                    watch(&t, &r, &j);
                    #[cfg(unix)]
                    watch(&t, &r);
                    std::thread::sleep(Duration::from_millis(100));
                }
            })
            .map_err(|e| CoreError::Php(e.to_string()))?;
        Ok(Supervisor {
            table,
            stop,
            keep_children: AtomicBool::new(false),
            record,
            #[cfg(windows)]
            job,
        })
    }

    fn spawn(&self, spec: &ProcessSpec) -> Result<Child, CoreError> {
        #[cfg(windows)]
        return spawn(spec, &self.job);
        #[cfg(unix)]
        return spawn(spec);
    }

    /// Starts `spec` or keeps it running; a changed spec for the same key restarts the process.
    /// Fails when its port is already taken or it does not come up.
    pub fn ensure(&self, spec: ProcessSpec) -> Result<(), CoreError> {
        {
            let mut table = self.table.lock().unwrap();
            if let Some(entry) = table.get(&spec.key) {
                if entry.spec == spec {
                    return Ok(());
                }
                if let Some(mut entry) = table.remove(&spec.key) {
                    if let Some(child) = entry.child.as_mut() {
                        terminate(child);
                    }
                }
            }
        }
        if let Some(port) = spec.ready_port {
            // The process we just stopped may need a moment to release it.
            let deadline = Instant::now() + Duration::from_secs(2);
            while answers(port) {
                if Instant::now() > deadline {
                    return Err(CoreError::Php(format!("Port {port} is used by another program")));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if let Some(socket) = &spec.ready_socket {
            // A socket file left by a crash would look ready at once.
            let _ = std::fs::remove_file(socket);
        }
        let child = self.spawn(&spec)?;
        let key = spec.key.clone();
        let (ready_port, ready_socket, log) = (spec.ready_port, spec.ready_socket.clone(), spec.log.clone());
        {
            let mut table = self.table.lock().unwrap();
            table.insert(
                key.clone(),
                Entry {
                    spec,
                    child: Some(child),
                    restarts: 0,
                    last_error: None,
                    started: Instant::now(),
                    retry_at: None,
                    backoff_step: 0,
                },
            );
            persist(&table, &self.record);
        }
        if ready_port.is_none() && ready_socket.is_none() {
            return Ok(());
        }
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if ready_port.is_some_and(answers) || ready_socket.as_ref().is_some_and(|s| s.exists()) {
                return Ok(());
            }
            let exited = {
                let mut table = self.table.lock().unwrap();
                let entry = table.get_mut(&key);
                entry.and_then(|e| e.child.as_mut()).is_none_or(|c| matches!(c.try_wait(), Ok(Some(_))))
            };
            if exited || Instant::now() > deadline {
                self.stop(&key);
                let detail = log_tail(&log);
                let reason = if exited { "stopped right away" } else { "did not start in time" };
                return Err(CoreError::Php(if detail.is_empty() {
                    format!("{key} {reason}")
                } else {
                    format!("{key} {reason}: {detail}")
                }));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn stop(&self, key: &str) {
        let entry = {
            let mut table = self.table.lock().unwrap();
            let entry = table.remove(key);
            persist(&table, &self.record);
            entry
        };
        if let Some(mut child) = entry.and_then(|e| e.child) {
            terminate(&mut child);
        }
    }

    /// Takes a process out of the supervisor's care without stopping it, so it can shut down on
    /// its own (a database's clean shutdown) without being restarted. The caller waits for it.
    pub fn detach(&self, key: &str) -> Option<Child> {
        let mut table = self.table.lock().unwrap();
        let entry = table.remove(key);
        persist(&table, &self.record);
        entry.and_then(|e| e.child)
    }

    pub fn stop_all(&self) {
        let entries: Vec<Entry> = {
            let mut table = self.table.lock().unwrap();
            let entries = table.drain().map(|(_, e)| e).collect();
            persist(&table, &self.record);
            entries
        };
        for mut entry in entries {
            if let Some(child) = entry.child.as_mut() {
                terminate(child);
            }
        }
    }

    pub fn keys(&self) -> Vec<String> {
        self.table.lock().unwrap().keys().cloned().collect()
    }

    pub fn status(&self) -> Vec<ProcessStatus> {
        let mut table = self.table.lock().unwrap();
        let mut list: Vec<ProcessStatus> = table
            .values_mut()
            .map(|e| {
                let running = e.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)));
                ProcessStatus {
                    key: e.spec.key.clone(),
                    running,
                    pid: e.child.as_ref().filter(|_| running).map(Child::id),
                    restarts: e.restarts,
                    last_error: e.last_error.clone(),
                }
            })
            .collect();
        list.sort_by(|a, b| a.key.cmp(&b.key));
        list
    }

    /// Test hook: drop without stopping, as if Hearth crashed (the Job Object must still kill).
    #[cfg(test)]
    pub(crate) fn forget_without_stopping(&self) {
        self.keep_children.store(true, Ordering::Relaxed);
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if !self.keep_children.load(Ordering::Relaxed) {
            self.stop_all();
        }
    }
}

/// Notices exited children and restarts them after their back-off delay.
fn watch(table: &Table, record: &Option<PathBuf>, #[cfg(windows)] job: &job::Job) {
    let mut table = table.lock().unwrap();
    let now = Instant::now();
    let mut respawned = false;
    for entry in table.values_mut() {
        if let Some(child) = entry.child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                entry.child = None;
                if status.success() {
                    // A clean exit is routine (php-cgi after PHP_FCGI_MAX_REQUESTS): back at once.
                    entry.retry_at = Some(now);
                    continue;
                }
                let tail = log_tail(&entry.spec.log);
                entry.last_error = Some(if tail.is_empty() {
                    format!("{} exited ({status})", entry.spec.key)
                } else {
                    format!("{} exited ({status}): {tail}", entry.spec.key)
                });
                if entry.started.elapsed() >= STABLE {
                    entry.backoff_step = 0;
                }
                let delay = BACKOFF[entry.backoff_step.min(BACKOFF.len() - 1)];
                entry.backoff_step += 1;
                entry.retry_at = Some(now + Duration::from_secs(delay));
            }
            continue;
        }
        if entry.retry_at.is_some_and(|t| t <= now) {
            #[cfg(windows)]
            let spawned = spawn(&entry.spec, job);
            #[cfg(unix)]
            let spawned = spawn(&entry.spec);
            match spawned {
                Ok(child) => {
                    entry.child = Some(child);
                    entry.restarts += 1;
                    entry.started = now;
                    entry.retry_at = None;
                    respawned = true;
                }
                Err(e) => {
                    entry.last_error = Some(e.to_string());
                    let delay = BACKOFF[entry.backoff_step.min(BACKOFF.len() - 1)];
                    entry.backoff_step += 1;
                    entry.retry_at = Some(now + Duration::from_secs(delay));
                }
            }
        }
    }
    if respawned {
        persist(&table, record);
    }
}

fn command(spec: &ProcessSpec) -> Result<Command, CoreError> {
    if let Some(dir) = spec.log.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    }
    let open_log = || std::fs::OpenOptions::new().create(true).append(true).open(&spec.log);
    let out = open_log().map_err(|e| CoreError::io(&spec.log, e))?;
    let err = open_log().map_err(|e| CoreError::io(&spec.log, e))?;
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).envs(spec.env.iter().map(|(k, v)| (k, v))).stdin(Stdio::null()).stdout(out).stderr(err);
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    Ok(cmd)
}

fn start_error(spec: &ProcessSpec, e: std::io::Error) -> CoreError {
    CoreError::Php(format!("Could not start {}: {e}", spec.program.display()))
}

#[cfg(windows)]
fn spawn(spec: &ProcessSpec, job: &job::Job) -> Result<Child, CoreError> {
    use std::os::windows::process::CommandExt;
    /// No console window for background programs.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = command(spec)?;
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| start_error(spec, e))?;
    if let Err(e) = job.assign(&child) {
        let _ = child.kill();
        return Err(CoreError::Php(format!("Could not supervise {}: {e}", spec.key)));
    }
    Ok(child)
}

#[cfg(unix)]
fn spawn(spec: &ProcessSpec) -> Result<Child, CoreError> {
    use std::os::unix::process::CommandExt;
    let mut cmd = command(spec)?;
    // Own process group, so stopping it also stops its workers (php-fpm).
    cmd.process_group(0);
    cmd.spawn().map_err(|e| start_error(spec, e))
}

#[cfg(windows)]
pub(crate) fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
pub(crate) fn terminate(child: &mut Child) {
    let group = -(child.id() as i32);
    // SAFETY: plain signal calls on the child's process group.
    unsafe { libc::kill(group, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // SAFETY: as above.
    unsafe { libc::kill(group, libc::SIGKILL) };
    let _ = child.wait();
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A Job Object whose processes are killed when its last handle closes (Hearth exiting or crashing).
    pub struct Job(HANDLE);

    // SAFETY: the handle is only used through thread-safe Win32 calls.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> std::io::Result<Job> {
            // SAFETY: anonymous job, then a fully initialised limit struct of the right size.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                let job = Job(handle);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(job)
            }
        }

        pub fn assign(&self, child: &Child) -> std::io::Result<()> {
            // SAFETY: both handles are valid for the duration of the call.
            let ok = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closed once; closing kills every process still in the job.
            unsafe { CloseHandle(self.0) };
        }
    }
}
