//! The machine environment (`HKLM\...\Session Manager\Environment`), as the helper service changes it
//! for the move from Herd. Only reachable through `protocol::handle`, which checks every value first.

use std::path::PathBuf;

use super::protocol::{MachineEnv, Removed};

/// The machine environment, with the record of what was taken out in `store`: a file the helper
/// keeps next to itself, where only administrators can write.
pub struct RealMachineEnv {
    pub store: PathBuf,
}

/// No machine environment: every change is refused (for callers without the helper's record).
pub struct NoMachineEnv;

impl MachineEnv for NoMachineEnv {
    fn path(&self) -> String {
        String::new()
    }
    fn set_path(&mut self, _: &str) -> Result<(), String> {
        Err("The machine environment is only changed by Hearth's helper service".into())
    }
    fn var(&self, _: &str) -> Option<String> {
        None
    }
    fn set_var(&mut self, _: &str, _: Option<&str>) -> Result<(), String> {
        Err("The machine environment is only changed by Hearth's helper service".into())
    }
    fn removed(&self) -> Removed {
        Removed::default()
    }
    fn set_removed(&mut self, _: &Removed) -> Result<(), String> {
        Err("The machine environment is only changed by Hearth's helper service".into())
    }
}

#[cfg(windows)]
mod win {
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    const KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: opened by `open` and closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open() -> Result<Key, String> {
        let mut key: HKEY = null_mut();
        // SAFETY: valid NUL-terminated path and out pointer.
        let status = unsafe {
            RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide(KEY).as_ptr(), 0, KEY_QUERY_VALUE | KEY_SET_VALUE, &mut key)
        };
        if status != ERROR_SUCCESS {
            return Err(format!("Could not open the machine environment (Windows error {status})"));
        }
        Ok(Key(key))
    }

    pub fn path() -> String {
        get("Path").unwrap_or_default()
    }

    pub fn get(value_name: &str) -> Option<String> {
        let (path, name) = (wide(KEY), wide(value_name));
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut size = 0u32;
        // SAFETY: a size query with a null buffer.
        let status = unsafe {
            RegGetValueW(HKEY_LOCAL_MACHINE, path.as_ptr(), name.as_ptr(), flags, null_mut(), null_mut(), &mut size)
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        // SAFETY: the buffer holds `size` bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                path.as_ptr(),
                name.as_ptr(),
                flags,
                null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    pub fn set(name: &str, value: Option<&str>) -> Result<(), String> {
        let key = open()?;
        let wname = wide(name);
        let status = match value {
            Some(v) => {
                let data = wide(v);
                // SAFETY: NUL-terminated UTF-16 data with its byte length.
                unsafe {
                    RegSetValueExW(
                        key.0,
                        wname.as_ptr(),
                        0,
                        REG_EXPAND_SZ,
                        data.as_ptr().cast(),
                        (data.len() * 2) as u32,
                    )
                }
            }
            // SAFETY: valid key and NUL-terminated name.
            None => match unsafe { RegDeleteValueW(key.0, wname.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                other => other,
            },
        };
        if status != ERROR_SUCCESS {
            return Err(format!("Could not change {name} in the machine environment (Windows error {status})"));
        }
        // New programs and terminals see the change.
        let env = wide("Environment");
        // SAFETY: a broadcast with a NUL-terminated string and a timeout.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                env.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                2000,
                null_mut(),
            )
        };
        Ok(())
    }
}

impl MachineEnv for RealMachineEnv {
    fn path(&self) -> String {
        #[cfg(windows)]
        return win::path();
        #[cfg(not(windows))]
        String::new()
    }

    fn set_path(&mut self, value: &str) -> Result<(), String> {
        #[cfg(windows)]
        return win::set("Path", Some(value));
        #[cfg(not(windows))]
        {
            let _ = value;
            Err("Only on Windows".into())
        }
    }

    fn var(&self, name: &str) -> Option<String> {
        #[cfg(windows)]
        return win::get(name);
        #[cfg(not(windows))]
        {
            let _ = name;
            None
        }
    }

    fn set_var(&mut self, name: &str, value: Option<&str>) -> Result<(), String> {
        #[cfg(windows)]
        return win::set(name, value);
        #[cfg(not(windows))]
        {
            let _ = (name, value);
            Err("Only on Windows".into())
        }
    }

    fn removed(&self) -> Removed {
        std::fs::read(&self.store).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn set_removed(&mut self, removed: &Removed) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(removed).map_err(|e| e.to_string())?;
        std::fs::write(&self.store, bytes).map_err(|e| format!("Could not keep the record of Herd's entries: {e}"))
    }
}
