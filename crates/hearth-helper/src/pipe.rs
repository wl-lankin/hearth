//! The helper's pipe: one client at a time, one JSON line in, one JSON line out. Only processes of
//! signed-in users on this computer may connect, and none of them may open a second server end.

use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Serves requests against `hosts` until `stop` is set.
pub fn serve(name: &str, hosts: &Path, stop: &AtomicBool) {
    let Some(pipe) = create(name) else { return };
    while !stop.load(Ordering::SeqCst) {
        // SAFETY: a valid pipe handle; blocks until a client connects.
        let connected =
            unsafe { ConnectNamedPipe(pipe.0, null_mut()) } != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if connected && !stop.load(Ordering::SeqCst) {
            let request = read_request(&pipe);
            let mut answer = serde_json::to_vec(&handle(&request, hosts)).unwrap_or_default();
            answer.push(b'\n');
            write_answer(&pipe, &answer);
        }
        // SAFETY: a valid pipe handle.
        unsafe { DisconnectNamedPipe(pipe.0) };
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use hearth_core::dns::protocol::PIPE;

    fn unique_pipe() -> String {
        format!("{PIPE}-test-{}", std::process::id())
    }

    fn ask(name: &str, line: &str) -> String {
        let mut client = None;
        for _ in 0..50 {
            match open(name) {
                Ok(file) => {
                    client = Some(file);
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        let mut client = client.expect("the pipe opens");
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
}
