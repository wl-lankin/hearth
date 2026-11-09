//! System folders as Windows reports them. The elevated install and the service never take them
//! from environment variables, which the signed-in user can change.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;

use windows_sys::core::PWSTR;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath};

/// `C:\Program Files` (or wherever Windows keeps it).
pub fn program_files() -> PathBuf {
    let mut path: PWSTR = null_mut();
    // SAFETY: Windows allocates `path`, which is freed below whatever the result.
    let found = unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, null_mut(), &mut path) } == 0;
    let result = found.then(|| {
        // SAFETY: on success `path` is a NUL-terminated wide string.
        let text = unsafe { widestring::U16CStr::from_ptr_str(path) };
        PathBuf::from(text.to_os_string())
    });
    // SAFETY: allocated by SHGetKnownFolderPath (freeing null is allowed).
    unsafe { CoTaskMemFree(path.cast()) };
    result.unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
}

/// `C:\Windows\System32` (or wherever Windows keeps it).
pub fn system32() -> PathBuf {
    let mut buf = [0u16; 260];
    // SAFETY: `buf` holds `buf.len()` characters.
    let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if len == 0 || len >= buf.len() {
        return PathBuf::from(r"C:\Windows\System32");
    }
    PathBuf::from(OsString::from_wide(&buf[..len]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_environment_variables_do_not_move_the_system_folders() {
        let d = tempfile::tempdir().unwrap();
        let (programs, root) = (std::env::var_os("ProgramFiles"), std::env::var_os("SystemRoot"));
        std::env::set_var("ProgramFiles", d.path());
        std::env::set_var("SystemRoot", d.path());
        let (p, s) = (program_files(), system32());
        if let Some(v) = programs {
            std::env::set_var("ProgramFiles", v);
        }
        if let Some(v) = root {
            std::env::set_var("SystemRoot", v);
        }
        assert!(!p.starts_with(d.path()) && p.join("..").is_dir(), "{}", p.display());
        assert!(!s.starts_with(d.path()) && s.join("drivers").join("etc").is_dir(), "{}", s.display());
    }
}
