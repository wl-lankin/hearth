//! Windows: talking to the helper service, and installing it (one UAC prompt).

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::dns::protocol::{Request, Response, CLIENT_ACCESS, PROTOCOL};
use crate::CoreError;

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PIPE_BUSY: i32 = 231;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// The helper program, once installed (only shown to the user, never run from here).
fn installed_program() -> PathBuf {
    let programs =
        std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    programs.join("Hearth").join("hearth-helper.exe")
}

fn not_reachable() -> CoreError {
    if installed_program().is_file() {
        CoreError::Php("The .test helper is not running".into())
    } else {
        CoreError::Php("The .test helper is not installed".into())
    }
}

/// Sends one request to the helper on `pipe`, waiting at most about a second for it to be free.
pub fn request_on(pipe: &str, request: &Request) -> Result<Response, CoreError> {
    use std::os::windows::fs::OpenOptionsExt;
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let stream = loop {
        match std::fs::OpenOptions::new().access_mode(CLIENT_ACCESS).open(pipe) {
            Ok(stream) => break stream,
            // The helper is answering someone else; it is quick.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => return Err(not_reachable()),
            Err(e) => return Err(CoreError::Php(format!("The .test helper did not answer: {e}"))),
        }
    };
    let mut line = serde_json::to_vec(request).map_err(|e| CoreError::Protocol(e.to_string()))?;
    line.push(b'\n');
    (&stream).write_all(&line).map_err(|e| CoreError::Php(format!("The .test helper did not answer: {e}")))?;
    let mut answer = String::new();
    BufReader::new(&stream)
        .read_line(&mut answer)
        .map_err(|e| CoreError::Php(format!("The .test helper did not answer: {e}")))?;
    serde_json::from_str(&answer).map_err(|e| CoreError::Protocol(e.to_string()))
}

/// Makes the helper's block hold exactly `names`, unless it already does.
pub fn set_names_on(pipe: &str, names: &BTreeSet<String>) -> Result<(), CoreError> {
    let status = request_on(pipe, &Request::Status)?;
    if status.protocol.unwrap_or(0) < PROTOCOL {
        return Err(CoreError::Php("The .test helper needs an update".into()));
    }
    let current: BTreeSet<String> = status.names.unwrap_or_default().into_iter().collect();
    if &current == names {
        return Ok(());
    }
    let answer = request_on(pipe, &Request::Hosts { names: names.iter().cloned().collect() })?;
    match answer.error {
        Some(error) => Err(CoreError::Php(error)),
        None => Ok(()),
    }
}

/// Installs (or updates) the helper from this program, after Windows asks the user (UAC).
pub fn install() -> Result<(), CoreError> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

    const ERROR_CANCELLED: u32 = 1223;
    let exe = std::env::current_exe().map_err(|e| CoreError::Php(e.to_string()))?;
    let file = widestring::U16CString::from_os_str_truncate(exe.as_os_str());
    let verb = widestring::U16CString::from_str_truncate("runas");
    let params = widestring::U16CString::from_str_truncate("--hearth-helper install");
    // SAFETY: a zeroed struct is a valid "no options" SHELLEXECUTEINFOW; the strings outlive the call.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    // SAFETY: `info` is fully set up; on success it carries a process handle we close below.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        // SAFETY: reads this thread's last error.
        let code = unsafe { GetLastError() };
        return Err(CoreError::Php(if code == ERROR_CANCELLED {
            "Setting up .test names was not confirmed".into()
        } else {
            format!("Could not start the .test helper setup (error {code})")
        }));
    }
    let mut exit = 1u32;
    // SAFETY: a process handle from ShellExecuteExW, closed once.
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        GetExitCodeProcess(info.hProcess, &mut exit);
        CloseHandle(info.hProcess);
    }
    if exit != 0 {
        return Err(CoreError::Php("The .test helper could not be installed".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_helper_setting_names_fails_fast_and_says_why() {
        let pipe = format!(r"{}-missing-{}", crate::dns::protocol::PIPE, std::process::id());
        let names: BTreeSet<String> = ["shop.test".to_string()].into();
        let started = Instant::now();
        let err = set_names_on(&pipe, &names).unwrap_err();
        assert!(started.elapsed() < Duration::from_millis(1500));
        assert!(err.to_string().starts_with("The .test helper is not "), "{err}");
    }
}
