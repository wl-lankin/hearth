//! Which program listens on a TCP port, read from the OS's listener table. Never by connecting: an
//! IDE takes any connection on its debug port for a debug session.

/// A program listening on a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub pid: u32,
    /// The executable's name without extension (`phpstorm64`, `Code`); `None` when it cannot be read
    /// (another user's process).
    pub program: Option<String>,
}

/// The program listening on `port` (IPv4 or IPv6), if any.
pub fn listener(port: u16) -> Option<Listener> {
    imp::listening_pid(port).map(|pid| Listener { pid, program: imp::program(pid) })
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The listener table of one address family, as raw bytes.
    fn table(family: u16) -> Option<Vec<u8>> {
        let mut size: u32 = 0;
        // SAFETY: a null buffer with size 0 only asks for the size needed.
        let first = unsafe {
            GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, family as u32, TCP_TABLE_OWNER_PID_LISTENER, 0)
        };
        if first != ERROR_INSUFFICIENT_BUFFER && first != NO_ERROR {
            return None;
        }
        // Listeners can appear between the two calls: leave room.
        let mut buf = vec![0u8; size as usize + 4096];
        let mut size = buf.len() as u32;
        // SAFETY: `buf` holds `size` writable bytes.
        let result = unsafe {
            GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut size, 0, family as u32, TCP_TABLE_OWNER_PID_LISTENER, 0)
        };
        (result == NO_ERROR).then_some(buf)
    }

    /// The rows of a table: a u32 count, then the rows (aligned after the count).
    fn rows<T: Copy>(buf: &[u8]) -> Vec<T> {
        if buf.len() < 4 {
            return vec![];
        }
        let count = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let start = std::mem::align_of::<T>().max(4);
        let size = std::mem::size_of::<T>();
        (0..count)
            .filter_map(|i| {
                let at = start + i * size;
                // SAFETY: the bytes at `at..at + size` lie inside `buf` and form one row as the API wrote it.
                (at + size <= buf.len()).then(|| unsafe { std::ptr::read_unaligned(buf[at..].as_ptr().cast::<T>()) })
            })
            .collect()
    }

    /// The port in a row's `dwLocalPort` (network byte order in the low 16 bits).
    fn port_of(raw: u32) -> u16 {
        u16::from_be(raw as u16)
    }

    pub fn listening_pid(port: u16) -> Option<u32> {
        let v4 = table(AF_INET).map(|b| rows::<MIB_TCPROW_OWNER_PID>(&b)).unwrap_or_default();
        if let Some(row) = v4.iter().find(|r| port_of(r.dwLocalPort) == port) {
            return Some(row.dwOwningPid);
        }
        let v6 = table(AF_INET6).map(|b| rows::<MIB_TCP6ROW_OWNER_PID>(&b)).unwrap_or_default();
        v6.iter().find(|r| port_of(r.dwLocalPort) == port).map(|r| r.dwOwningPid)
    }

    pub fn program(pid: u32) -> Option<String> {
        // SAFETY: plain Win32 calls; the handle is closed before returning.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let path = std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]));
            path.file_stem().map(|s| s.to_string_lossy().into_owned())
        }
    }
}

#[cfg(unix)]
mod imp {
    use std::process::Command;

    /// `lsof -Fpc` prints `p<pid>` and `c<command>` lines for each listener. `-b` keeps it from
    /// blocking on kernel calls; after a second it is stopped and nothing counts as found.
    fn lsof(port: u16) -> Option<(u32, Option<String>)> {
        let mut child = Command::new("/usr/sbin/lsof")
            .args(["-b", "-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpc"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() < std::time::Duration::from_secs(1) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        }
        let out = child.wait_with_output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut pid = None;
        let mut command = None;
        for line in text.lines() {
            if let Some(p) = line.strip_prefix('p') {
                if pid.is_some() {
                    break;
                }
                pid = p.parse().ok();
            } else if let Some(c) = line.strip_prefix('c') {
                command = Some(c.to_string());
            }
        }
        pid.map(|p| (p, command))
    }

    pub fn listening_pid(port: u16) -> Option<u32> {
        lsof(port).map(|(pid, _)| pid)
    }

    pub fn program(pid: u32) -> Option<String> {
        let out = Command::new("ps").args(["-p", &pid.to_string(), "-o", "comm="]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let name = std::path::Path::new(&text).file_stem()?.to_string_lossy().into_owned();
        (!name.is_empty()).then_some(name)
    }
}
