//! The helper's pipe: one client at a time, one JSON line in, one JSON line out. Only processes of
//! signed-in users on this computer may connect, and none of them may open a second server end.

use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hearth_core::dns::protocol::{handle, CLIENT_ACCESS, MAX_REQUEST};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    FlushFileBuffers, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{GetCurrentThreadId, OpenThread, THREAD_TERMINATE};
use windows_sys::Win32::System::IO::CancelSynchronousIo;

/// SYSTEM: full control. Authenticated users: read data, write data, read attributes, synchronize
/// (0x100083), deliberately not FILE_CREATE_PIPE_INSTANCE, so no user process can pose as the helper.
const SDDL: &str = "D:P(A;;GA;;;SY)(A;;0x100083;;;AU)";

pub fn hosts_path() -> PathBuf {
    crate::folders::system32().join("drivers").join("etc").join("hosts")
}

/// A client end of the pipe, with exactly the rights the pipe grants.
fn open(name: &str) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().access_mode(CLIENT_ACCESS).open(name)
}

/// Connects once to the pipe so a waiting `serve` returns and sees its stop flag.
pub fn wake(name: &str) {
    let _ = open(name);
}

struct Pipe(HANDLE);

impl Drop for Pipe {
    fn drop(&mut self) {
        // SAFETY: a handle from CreateNamedPipeW, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn create(name: &str) -> Option<Pipe> {
    let sddl = widestring::U16CString::from_str_truncate(SDDL);
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: a valid NUL-terminated SDDL string; the descriptor is freed with LocalFree below.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let name = widestring::U16CString::from_str_truncate(name);
    // SAFETY: valid name and attributes for the duration of the call.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            MAX_REQUEST as u32 + 1,
            0,
            &attributes,
        )
    };
    // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
    unsafe { LocalFree(descriptor) };
    (handle != INVALID_HANDLE_VALUE).then_some(Pipe(handle))
}

/// Reads up to one line (or one byte past the size limit, so `handle` can refuse it).
fn read_request(pipe: &Pipe) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buf = [0u8; 4096];
    while request.len() <= MAX_REQUEST && !request.contains(&b'\n') {
        let mut read = 0u32;
        // SAFETY: `buf` is valid for `buf.len()` bytes.
        let ok = unsafe { ReadFile(pipe.0, buf.as_mut_ptr(), buf.len() as u32, &mut read, null_mut()) };
        if ok == 0 || read == 0 {
            break;
        }
        request.extend_from_slice(&buf[..read as usize]);
    }
    if let Some(end) = request.iter().position(|b| *b == b'\n') {
        request.truncate(end);
    }
    request
}

fn write_answer(pipe: &Pipe, answer: &[u8]) {
    let mut written = 0u32;
    // SAFETY: `answer` is valid for its length.
    unsafe {
        WriteFile(pipe.0, answer.as_ptr(), answer.len() as u32, &mut written, null_mut());
        FlushFileBuffers(pipe.0);
    }
}

/// How long a client may take to send its request before it is dropped (it would hold the only
/// pipe instance otherwise).
const REQUEST_DEADLINE: Duration = Duration::from_secs(2);

/// Serves requests against `hosts` until `stop` is set.
pub fn serve(name: &str, hosts: &Path, stop: &AtomicBool) {
    let (Some(pipe), Some(me)) = (create(name), Thread::current()) else { return };
    let me = Arc::new(me);
    // The client whose request is being read; 0 between reads. A watchdog cancels only the read
    // it was started for, and only while it is still going on.
    let reading = Arc::new(Mutex::new(0u64));
    let mut client = 0u64;
    while !stop.load(Ordering::SeqCst) {
        // SAFETY: a valid pipe handle; blocks until a client connects.
        let connected =
            unsafe { ConnectNamedPipe(pipe.0, null_mut()) } != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if connected && !stop.load(Ordering::SeqCst) {
            client += 1;
            *reading.lock().unwrap_or_else(|e| e.into_inner()) = client;
            let (watched, me, reading_ref) = (client, me.clone(), reading.clone());
            std::thread::spawn(move || {
                std::thread::sleep(REQUEST_DEADLINE);
                let current = reading_ref.lock().unwrap_or_else(|e| e.into_inner());
                if *current == watched {
                    // SAFETY: a thread handle with THREAD_TERMINATE access; its blocked read returns.
                    unsafe { CancelSynchronousIo(me.0) };
                }
            });
            let request = read_request(&pipe);
            *reading.lock().unwrap_or_else(|e| e.into_inner()) = 0;
            let mut answer = serde_json::to_vec(&handle(&request, hosts)).unwrap_or_default();
            answer.push(b'\n');
            write_answer(&pipe, &answer);
        }
        // SAFETY: a valid pipe handle.
        unsafe { DisconnectNamedPipe(pipe.0) };
    }
}

/// The serving thread, for the watchdog to cancel its read.
struct Thread(HANDLE);

// SAFETY: a kernel handle may be used from any thread.
unsafe impl Send for Thread {}
// SAFETY: as above.
unsafe impl Sync for Thread {}

impl Thread {
    fn current() -> Option<Thread> {
        // SAFETY: opens this thread by its id; closed on drop.
        let handle = unsafe { OpenThread(THREAD_TERMINATE, 0, GetCurrentThreadId()) };
        (!handle.is_null()).then_some(Thread(handle))
    }
}

impl Drop for Thread {
    fn drop(&mut self) {
        // SAFETY: a handle from OpenThread, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};

    use super::*;
    use hearth_core::dns::protocol::PIPE;

    fn unique_pipe() -> String {
        format!("{PIPE}-test-{}", std::process::id())
    }

    fn connect(name: &str) -> std::fs::File {
        for _ in 0..50 {
            match open(name) {
                Ok(file) => return file,
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        panic!("the pipe does not open");
    }

    fn start(name: &str, hosts: &Path) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let stop = Arc::new(AtomicBool::new(false));
        let server = {
            let (name, hosts, stop) = (name.to_string(), hosts.to_path_buf(), stop.clone());
            std::thread::spawn(move || serve(&name, &hosts, &stop))
        };
        (stop, server)
    }

    fn ask(name: &str, line: &str) -> String {
        let mut client = connect(name);
        client.write_all(format!("{line}\n").as_bytes()).unwrap();
        let mut answer = String::new();
        BufReader::new(client).read_line(&mut answer).unwrap();
        answer
    }

    #[test]
    fn requests_over_the_pipe_change_the_hosts_file_and_a_second_server_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let hosts = d.path().join("hosts");
        std::fs::write(&hosts, "127.0.0.1 localhost\r\n").unwrap();
        let name = unique_pipe();
        let stop = Arc::new(AtomicBool::new(false));
        let server = {
            let (name, hosts, stop) = (name.clone(), hosts.clone(), stop.clone());
            std::thread::spawn(move || serve(&name, &hosts, &stop))
        };
        assert_eq!(ask(&name, r#"{"op":"hosts","names":["a.test"]}"#), "{}\n");
        assert!(std::fs::read_to_string(&hosts).unwrap().contains("127.0.0.1 a.test\r\n"));
        assert!(ask(&name, r#"{"op":"status"}"#).contains(r#""names":["a.test"]"#));
        assert!(create(&name).is_none(), "nobody may open a second server end of the pipe");
        stop.store(true, Ordering::SeqCst);
        wake(&name);
        server.join().unwrap();
    }

    #[test]
    fn a_client_that_sends_nothing_is_dropped_and_the_next_one_is_served() {
        use std::io::Read;
        let d = tempfile::tempdir().unwrap();
        let hosts = d.path().join("hosts");
        std::fs::write(&hosts, "127.0.0.1 localhost\r\n").unwrap();
        let name = format!("{}-silent", unique_pipe());
        let (stop, server) = start(&name, &hosts);
        let silent = connect(&name);
        let (done, waited) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1];
            let _ = (&silent).read(&mut buf);
            let _ = done.send(());
        });
        assert!(waited.recv_timeout(Duration::from_secs(5)).is_ok(), "the silent client is still connected");
        assert!(ask(&name, r#"{"op":"status"}"#).contains("protocol"));
        stop.store(true, Ordering::SeqCst);
        wake(&name);
        server.join().unwrap();
    }
}
