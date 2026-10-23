//! Who may talk to whom: the endpoint only admits the current user, and a client only trusts a
//! core that runs as the current user (another local user could otherwise squat the name first).

use std::io;

use interprocess::local_socket::tokio::{prelude::*, Stream};

/// True when the process at the other end of `stream` runs as the current user.
pub(crate) fn is_trusted(stream: &Stream) -> io::Result<bool> {
    let creds = stream.peer_creds()?;
    #[cfg(windows)]
    {
        Ok(creds.pid().map(|pid| same_user_as_me(pid as u32)).unwrap_or(false))
    }
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions.
        Ok(creds.euid() == Some(unsafe { libc::geteuid() }))
    }
}

#[cfg(windows)]
pub(crate) use win::{pipe_sddl, same_user_as_me};

#[cfg(windows)]
mod win {
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by us and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// The TOKEN_USER of a process, in a u64 buffer so the struct is aligned.
    fn token_user(process: HANDLE) -> Option<Vec<u64>> {
        let mut token: HANDLE = null_mut();
        // SAFETY: plain Win32 calls with valid out pointers; buffers are sized by the first call.
        unsafe {
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let token = Handle(token);
            let mut len = 0u32;
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut len);
            if len == 0 {
                return None;
            }
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            if GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) == 0 {
                return None;
            }
            Some(buf)
        }
    }

    fn sid_of(buf: &[u64]) -> *mut core::ffi::c_void {
        // SAFETY: `buf` holds a TOKEN_USER filled by GetTokenInformation.
        unsafe { (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid }
    }

    /// True when process `pid` runs as the same user as this process. False when it cannot be
    /// inspected (e.g. a system process).
    pub(crate) fn same_user_as_me(pid: u32) -> bool {
        // SAFETY: the pseudo handle of the current process needs no closing.
        let Some(mine) = token_user(unsafe { GetCurrentProcess() }) else { return false };
        // SAFETY: plain Win32 call; a null result is handled.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return false;
        }
        let process = Handle(process);
        let Some(theirs) = token_user(process.0) else { return false };
        // SAFETY: both SIDs point into live TOKEN_USER buffers.
        unsafe { EqualSid(sid_of(&mine), sid_of(&theirs)) != 0 }
    }

    /// An SDDL string granting full access to the current user only (protected, no inherited ACEs).
    pub(crate) fn pipe_sddl() -> Option<String> {
        // SAFETY: the pseudo handle of the current process needs no closing.
        let mine = token_user(unsafe { GetCurrentProcess() })?;
        let mut text: *mut u16 = null_mut();
        // SAFETY: the SID is valid; the returned string is freed with LocalFree.
        unsafe {
            if ConvertSidToStringSidW(sid_of(&mine), &mut text) == 0 {
                return None;
            }
            let len = (0..).take_while(|&i| *text.add(i) != 0).count();
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            Some(format!("D:P(A;;GA;;;{sid})"))
        }
    }
}
