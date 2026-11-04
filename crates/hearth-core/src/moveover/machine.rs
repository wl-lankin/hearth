//! The real computer for the move: Windows' registry, the hosts file and Claude's settings, read only.

use std::path::{Path, PathBuf};

use super::scan::Machine;

/// This computer. Outside Windows every answer is empty: there is no Herd to move from.
pub struct RealMachine;

impl Machine for RealMachine {
    fn user_path(&self) -> String {
        reg::string(reg::Root::User, "Environment", "Path").unwrap_or_default()
    }

    fn machine_path(&self) -> String {
        self.machine_var("Path").unwrap_or_default()
    }

    fn machine_var(&self, name: &str) -> Option<String> {
        reg::string(reg::Root::Machine, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment", name)
    }

    fn run_values(&self) -> Vec<(String, String)> {
        reg::values(reg::Root::User, r"Software\Microsoft\Windows\CurrentVersion\Run")
    }

    fn uninstall_entries(&self) -> Vec<(String, String)> {
        let keys = [
            (reg::Root::Machine, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
            (reg::Root::Machine, r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
            (reg::Root::User, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
        ];
        let mut entries = Vec::new();
        for (root, key) in keys {
            for sub in reg::subkeys(root, key) {
                let path = format!(r"{key}\{sub}");
                if let (Some(name), Some(command)) =
                    (reg::string(root, &path, "DisplayName"), reg::string(root, &path, "UninstallString"))
                {
                    entries.push((name, command));
                }
            }
        }
        entries
    }

    fn hosts(&self) -> String {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        std::fs::read_to_string(root.join("System32").join("drivers").join("etc").join("hosts")).unwrap_or_default()
    }

    fn claude_json(&self) -> Option<PathBuf> {
        Some(dirs::home_dir()?.join(".claude.json"))
    }

    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        std::fs::read_link(path).ok()
    }
}

/// This computer, for the move's outside changes. Outside Windows each write says it can't.
pub struct RealWrite;

impl super::outside::MachineWrite for RealWrite {
    fn user_path(&self) -> String {
        #[cfg(windows)]
        return crate::path_env::read_user_path().unwrap_or_default();
        #[cfg(not(windows))]
        String::new()
    }

    fn set_user_path(&mut self, value: &str) -> Result<(), crate::CoreError> {
        #[cfg(windows)]
        return crate::path_env::write_user_path(value);
        #[cfg(not(windows))]
        {
            let _ = value;
            Err(crate::CoreError::Php("Only on Windows".into()))
        }
    }

    fn set_run(&mut self, name: &str, command: Option<&str>) -> Result<(), crate::CoreError> {
        reg::set_run(name, command).map_err(crate::CoreError::Php)
    }

    fn claude(&mut self, args: &[String]) -> Result<(), crate::CoreError> {
        // `claude` is a command script on Windows: run it through cmd.
        let mut cmd = crate::workspaces::command(if cfg!(windows) { "cmd" } else { "claude" });
        if cfg!(windows) {
            cmd.args(["/c", "claude"]);
        }
        let out =
            cmd.args(args).output().map_err(|e| crate::CoreError::Php(format!("Claude Code could not be run: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(crate::CoreError::Php(format!(
                "claude {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    fn helper(&mut self, request: &crate::dns::protocol::Request) -> Result<(), crate::CoreError> {
        let answer = crate::dns::client::request_on(crate::dns::protocol::PIPE, request)?;
        match answer.error {
            Some(e) => Err(crate::CoreError::Php(e)),
            None => Ok(()),
        }
    }
}

#[cfg(windows)]
mod reg {
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    };

    #[derive(Clone, Copy)]
    pub enum Root {
        User,
        Machine,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn text(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    fn root(r: Root) -> HKEY {
        match r {
            Root::User => HKEY_CURRENT_USER,
            Root::Machine => HKEY_LOCAL_MACHINE,
        }
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: opened by `open` and closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open(r: Root, path: &str) -> Option<Key> {
        let mut key: HKEY = null_mut();
        // SAFETY: valid NUL-terminated path and out pointer.
        let status = unsafe { RegOpenKeyExW(root(r), wide(path).as_ptr(), 0, KEY_READ, &mut key) };
        (status == ERROR_SUCCESS).then_some(Key(key))
    }

    /// A text value as written (`%VAR%` not expanded).
    pub fn string(r: Root, path: &str, name: &str) -> Option<String> {
        let (path, name) = (wide(path), wide(name));
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut size = 0u32;
        // SAFETY: a size query with a null buffer.
        let status =
            unsafe { RegGetValueW(root(r), path.as_ptr(), name.as_ptr(), flags, null_mut(), null_mut(), &mut size) };
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        // SAFETY: the buffer holds `size` bytes.
        let status = unsafe {
            RegGetValueW(root(r), path.as_ptr(), name.as_ptr(), flags, null_mut(), buf.as_mut_ptr().cast(), &mut size)
        };
        (status == ERROR_SUCCESS).then(|| text(&buf))
    }

    /// The text values of a key (name, data).
    pub fn values(r: Root, path: &str) -> Vec<(String, String)> {
        let Some(key) = open(r, path) else { return Vec::new() };
        let mut out = Vec::new();
        for index in 0.. {
            let mut name = vec![0u16; 16_384];
            let mut name_len = name.len() as u32;
            let mut data = vec![0u16; 8_192];
            let mut data_len = (data.len() * 2) as u32;
            let mut kind = 0u32;
            // SAFETY: buffers and lengths match.
            let status = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    name.as_mut_ptr(),
                    &mut name_len,
                    null_mut(),
                    &mut kind,
                    data.as_mut_ptr().cast(),
                    &mut data_len,
                )
            };
            if status == ERROR_MORE_DATA {
                continue;
            }
            if status != ERROR_SUCCESS {
                break;
            }
            if kind == REG_SZ || kind == REG_EXPAND_SZ {
                out.push((text(&name), text(&data)));
            }
        }
        out
    }

    /// Sets (or with `None` removes) a value of the current user's `Run` key.
    pub fn set_run(name: &str, command: Option<&str>) -> Result<(), String> {
        use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows_sys::Win32::System::Registry::{RegDeleteValueW, RegSetValueExW, KEY_SET_VALUE};
        let mut key: HKEY = null_mut();
        let path = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
        // SAFETY: valid NUL-terminated path and out pointer.
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_SET_VALUE, &mut key) };
        if status != ERROR_SUCCESS {
            return Err(format!("Could not open the Run key (Windows error {status})"));
        }
        let key = Key(key);
        let wname = wide(name);
        let status = match command {
            Some(c) => {
                let data = wide(c);
                // SAFETY: NUL-terminated UTF-16 data with its byte length.
                unsafe {
                    RegSetValueExW(key.0, wname.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32)
                }
            }
            // SAFETY: valid key and NUL-terminated name.
            None => match unsafe { RegDeleteValueW(key.0, wname.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                other => other,
            },
        };
        if status != ERROR_SUCCESS {
            return Err(format!("Could not change {name} in the Run key (Windows error {status})"));
        }
        Ok(())
    }

    /// The names of a key's subkeys.
    pub fn subkeys(r: Root, path: &str) -> Vec<String> {
        let Some(key) = open(r, path) else { return Vec::new() };
        let mut out = Vec::new();
        for index in 0.. {
            let mut name = vec![0u16; 512];
            let mut len = name.len() as u32;
            // SAFETY: buffer and length match.
            let status = unsafe {
                RegEnumKeyExW(key.0, index, name.as_mut_ptr(), &mut len, null_mut(), null_mut(), null_mut(), null_mut())
            };
            if status != ERROR_SUCCESS {
                break;
            }
            out.push(text(&name));
        }
        out
    }
}

#[cfg(not(windows))]
mod reg {
    #[derive(Clone, Copy)]
    pub enum Root {
        User,
        Machine,
    }

    pub fn string(_: Root, _: &str, _: &str) -> Option<String> {
        None
    }

    pub fn values(_: Root, _: &str) -> Vec<(String, String)> {
        Vec::new()
    }

    pub fn subkeys(_: Root, _: &str) -> Vec<String> {
        Vec::new()
    }

    pub fn set_run(_: &str, _: Option<&str>) -> Result<(), String> {
        Err("Only on Windows".into())
    }
}
