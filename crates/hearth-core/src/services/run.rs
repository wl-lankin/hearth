//! Starting and stopping services. A start checks the ports, installs the build if needed, sets up
//! an empty data folder once, then hands the process to the supervisor and waits for its port.
//! A stop asks a database to shut down cleanly before anything is killed.

use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::ipc::CoreState;
use crate::services::install::{ensure_installed, installed};
use crate::services::kinds::{self, Run};
use crate::services::ports;
use crate::services::store::{self, data_dir, log_file, Instance};
use crate::supervisor::{log_tail, terminate};
use crate::CoreError;

/// A first start of a database (crash recovery, a large data folder) can take a while.
const START_TIMEOUT: Duration = Duration::from_secs(60);
const SETUP_TIMEOUT: Duration = Duration::from_secs(300);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub line: String,
    pub port: u16,
    pub autostart: bool,
    pub running: bool,
    pub pid: Option<u32>,
    /// The latest problem: a start that failed, or a crash the supervisor saw.
    pub error: Option<String>,
    pub log: String,
}

fn key(id: &str) -> String {
    format!("service-{id}")
}

fn running(core: &CoreState, id: &str) -> bool {
    core.supervisor.status().iter().any(|p| p.key == key(id) && p.running)
}

fn remember(core: &CoreState, id: &str, error: Option<String>) {
    let mut errors = core.service_errors.lock().unwrap_or_else(|e| e.into_inner());
    match error {
        Some(e) => errors.insert(id.to_string(), e),
        None => errors.remove(id),
    };
}

/// The lock that keeps a start, stop, delete or port change of one instance from overlapping.
fn lock_for(core: &CoreState, id: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    let mut locks = core.service_locks.lock().unwrap_or_else(|e| e.into_inner());
    locks.entry(id.to_string()).or_default().clone()
}

pub async fn start(core: &CoreState, id: &str) -> Result<(), CoreError> {
    let lock = lock_for(core, id);
    let _one = lock.lock().await;
    start_locked(core, id).await
}

/// `start` for a caller that already holds the instance's lock.
pub(crate) async fn start_locked(core: &CoreState, id: &str) -> Result<(), CoreError> {
    let result = start_inner(core, id).await;
    remember(core, id, result.as_ref().err().map(|e| e.to_string()));
    result
}

async fn start_inner(core: &CoreState, id: &str) -> Result<(), CoreError> {
    let inst = store::get(&core.paths, id)?;
    if running(core, id) {
        return Ok(());
    }
    ports::check(&core.paths, &kinds::ports(&inst), Some(id))?;
    let install = ensure_installed(core, &inst.kind, &inst.line).await?;
    let core = core.clone();
    tokio::task::spawn_blocking(move || launch(&core, &inst, &install))
        .await
        .map_err(|e| CoreError::Php(e.to_string()))?
}

/// Setup (once) and start, then wait for the port. Blocking.
fn launch(core: &CoreState, inst: &Instance, install: &Path) -> Result<(), CoreError> {
    let data = data_dir(&core.paths, inst);
    let log = log_file(&core.paths, inst);
    for dir in [Some(data.as_path()), log.parent()].into_iter().flatten() {
        std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    }
    kinds::clean_stale(inst, &data);
    if kinds::init(inst, install, &data).is_some() {
        // Set up next to the data folder and move it into place only when setup succeeded: a setup
        // that dies halfway must not leave a folder that looks set up.
        let staging = data.with_extension("init");
        let _ = std::fs::remove_dir_all(&staging);
        let setup = kinds::init(inst, install, &staging)
            .ok_or_else(|| CoreError::Php(format!("{} cannot be set up", inst.name)))?;
        if let Err(e) = run_to_end(&setup, &log, SETUP_TIMEOUT) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(CoreError::Php(explain(format!("{} setup failed: {e}", inst.name))));
        }
        let _ = std::fs::remove_dir(&data);
        crate::atomic::rename_dir(&staging, &data).map_err(|e| CoreError::io(&data, e))?;
    }
    let mut spec = kinds::spec(&core.paths, inst, install)?;
    // The supervisor's own wait is short; a database may need longer (see START_TIMEOUT).
    let port = spec.ready_port.take().unwrap_or(inst.port);
    core.supervisor.ensure(spec)?;
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if answers(port) {
            return Ok(());
        }
        let alive = core.supervisor.status().iter().any(|p| p.key == key(&inst.id) && p.running);
        if alive && Instant::now() > deadline {
            // Not killed: a database recovering after a crash can need longer, and killing it
            // would only start the recovery over.
            return Err(CoreError::Php(format!(
                "{} is still starting (after a crash a database may recover for a while); see its log",
                inst.name
            )));
        }
        if !alive {
            core.supervisor.stop(&key(&inst.id));
            let tail = log_tail(&log);
            let last = core.supervisor.status().into_iter().find(|p| p.key == key(&inst.id)).and_then(|p| p.last_error);
            let detail =
                [last.unwrap_or_default(), tail].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(": ");
            return Err(CoreError::Php(explain(if detail.is_empty() {
                format!("{} stopped right away", inst.name)
            } else {
                format!("{} stopped right away: {detail}", inst.name)
            })));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Adds what to do to errors that only show a Windows code: 0xC0000135 means a DLL is missing,
/// for MySQL the Visual C++ runtime, which no window reports when the program runs hidden.
pub(crate) fn explain(message: String) -> String {
    if message.to_lowercase().contains("0xc0000135") {
        format!(
            "{} Install the Microsoft Visual C++ Redistributable (x64): https://aka.ms/vs/17/release/vc_redist.x64.exe",
            message.trim_end()
        )
    } else {
        message
    }
}

fn answers(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(200)).is_ok()
}

fn command(run: &Run, log: &Path) -> Result<Command, CoreError> {
    let out = std::fs::OpenOptions::new().create(true).append(true).open(log).map_err(|e| CoreError::io(log, e))?;
    let err = out.try_clone().map_err(|e| CoreError::io(log, e))?;
    let mut command = Command::new(&run.program);
    command.args(&run.args).stdin(Stdio::null()).stdout(out).stderr(err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(command)
}

/// Runs a setup or shutdown command to its end (output into the service's log).
fn run_to_end(run: &Run, log: &Path, timeout: Duration) -> Result<(), String> {
    let mut child = command(run, log)
        .and_then(|mut c| c.spawn().map_err(|e| CoreError::io(&run.program, e)))
        .map_err(|e| e.to_string())?;
    match wait(&mut child, timeout) {
        Some(status) if status.success() => Ok(()),
        Some(status) => Err(format!("{} ({status}): {}", run.program.display(), log_tail(log))),
        None => {
            terminate(&mut child);
            Err(format!("{} did not finish", run.program.display()))
        }
    }
}

fn wait(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub async fn stop(core: &CoreState, id: &str) -> Result<(), CoreError> {
    let lock = lock_for(core, id);
    let _one = lock.lock().await;
    stop_locked(core, id).await
}

/// `stop` for a caller that already holds the instance's lock.
pub(crate) async fn stop_locked(core: &CoreState, id: &str) -> Result<(), CoreError> {
    let inst = store::get(&core.paths, id)?;
    let owned = core.clone();
    tokio::task::spawn_blocking(move || shut_down(&owned, &inst)).await.map_err(|e| CoreError::Php(e.to_string()))?;
    remember(core, id, None);
    Ok(())
}

/// Clean shutdown for databases, then whatever is left is stopped. Blocking.
fn shut_down(core: &CoreState, inst: &Instance) {
    let Some(mut child) = core.supervisor.detach(&key(&inst.id)) else { return };
    // On Windows, mysqld and mariadbd shut down cleanly when their named event is set: no
    // password needed (mysqladmin fails once root has one).
    #[cfg(windows)]
    if matches!(inst.kind.as_str(), "mysql" | "mariadb")
        && signal_shutdown_event(child.id())
        && wait(&mut child, SHUTDOWN_TIMEOUT).is_some()
    {
        return;
    }
    let data = data_dir(&core.paths, inst);
    let graceful = installed(core, &inst.kind, &inst.line).and_then(|install| kinds::stop(inst, &install, &data));
    if let Some(run) = graceful {
        if run_to_end(&run, &log_file(&core.paths, inst), SHUTDOWN_TIMEOUT).is_ok()
            && wait(&mut child, SHUTDOWN_TIMEOUT).is_some()
        {
            return;
        }
    }
    terminate(&mut child);
}

/// Sets `MySQLShutdown<pid>`, the event MySQL and MariaDB wait on for a clean shutdown on Windows.
#[cfg(windows)]
fn signal_shutdown_event(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
    let name: Vec<u16> = format!("MySQLShutdown{pid}").encode_utf16().chain([0]).collect();
    // SAFETY: a NUL-terminated name; the handle is closed below.
    unsafe {
        let event = OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr());
        if event.is_null() {
            return false;
        }
        let set = SetEvent(event) != 0;
        CloseHandle(event);
        set
    }
}

pub fn status(core: &CoreState) -> Vec<ServiceStatus> {
    let processes = core.supervisor.status();
    let errors = core.service_errors.lock().unwrap_or_else(|e| e.into_inner()).clone();
    store::load(&core.paths)
        .into_iter()
        .map(|inst| {
            let process = processes.iter().find(|p| p.key == key(&inst.id));
            let running = process.is_some_and(|p| p.running);
            let error = errors.get(&inst.id).cloned().or_else(|| process.and_then(|p| p.last_error.clone()));
            ServiceStatus {
                log: log_file(&core.paths, &inst).display().to_string(),
                id: inst.id,
                kind: inst.kind,
                name: inst.name,
                line: inst.line,
                port: inst.port,
                autostart: inst.autostart,
                running,
                pid: process.and_then(|p| p.pid),
                error,
            }
        })
        .collect()
}

/// Starts the instances marked for autostart; problems show in their status.
pub async fn start_autostart(core: &CoreState) {
    for inst in store::load(&core.paths).into_iter().filter(|i| i.autostart) {
        let _ = start(core, &inst.id).await;
    }
}

/// Stops every service, databases cleanly (on quit, before the supervisor stops the rest).
pub async fn stop_all(core: &CoreState) {
    for inst in store::load(&core.paths) {
        let _ = stop(core, &inst.id).await;
    }
}

/// Stops the instance and deletes it with its data.
pub async fn delete(core: &CoreState, id: &str) -> Result<(), CoreError> {
    let lock = lock_for(core, id);
    let _one = lock.lock().await;
    stop_locked(core, id).await?;
    store::delete(&core.paths, id)
}

/// Moves an instance to another port, restarting it when it runs.
pub async fn set_port(core: &CoreState, id: &str, port: u16) -> Result<(), CoreError> {
    let lock = lock_for(core, id);
    let _one = lock.lock().await;
    let mut instance = store::get(&core.paths, id)?;
    instance.port = port;
    ports::check(&core.paths, &kinds::ports(&instance), Some(id))?;
    let was_running = running(core, id);
    if was_running {
        stop_locked(core, id).await?;
    }
    store::set_port(&core.paths, id, port)?;
    if was_running {
        start_locked(core, id).await?;
    }
    Ok(())
}
