//! Making the OS trust Hearth's CA, once: the Windows user certificate store (Windows asks the user
//! to confirm, no admin needed) or the macOS login keychain (asks for the password).

use std::ffi::OsString;
use std::path::Path;

use crate::tls::Ca;
use crate::CoreError;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// Whether browsers on this computer trust `ca` (stored at `crt`).
pub fn is_trusted(ca: &Ca, crt: &Path) -> bool {
    // Each OS needs only one of the two.
    let _ = (ca, crt);
    #[cfg(windows)]
    return windows::is_trusted(ca.der());
    #[cfg(target_os = "macos")]
    return macos::is_trusted(crt);
    #[cfg(not(any(windows, target_os = "macos")))]
    false
}

/// Adds `ca` (stored at `crt`) to the trusted roots. The OS asks the user first.
pub fn trust(ca: &Ca, crt: &Path) -> Result<(), CoreError> {
    // Each OS needs only one of the two.
    let _ = (ca, crt);
    #[cfg(windows)]
    return windows::trust(ca.der());
    #[cfg(target_os = "macos")]
    return macos::trust(crt);
    #[cfg(not(any(windows, target_os = "macos")))]
    Err(CoreError::Tls("Trusting the CA is not supported here".into()))
}

/// Removes `ca` from the trusted roots again.
pub fn untrust(ca: &Ca, crt: &Path) {
    #[cfg(windows)]
    windows::untrust(ca.der());
    #[cfg(target_os = "macos")]
    macos::untrust(crt);
    let _ = (ca, crt);
}

/// `security` arguments that trust `crt` as a root in `keychain`.
pub fn macos_trust_args(crt: &Path, keychain: &Path) -> Vec<OsString> {
    vec!["add-trusted-cert".into(), "-r".into(), "trustRoot".into(), "-k".into(), keychain.into(), crt.into()]
}

/// `security` arguments that check whether `crt` is trusted.
pub fn macos_check_args(crt: &Path) -> Vec<OsString> {
    vec!["verify-cert".into(), "-c".into(), crt.into(), "-p".into(), "basic".into()]
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::{ca_cert_path, ensure_ca};
    use crate::HearthPaths;

    #[test]
    fn a_fresh_ca_is_not_trusted() {
        let d = tempfile::tempdir().unwrap();
        let p = HearthPaths::at(d.path());
        let ca = ensure_ca(&p).unwrap();
        assert!(!is_trusted(&ca, &ca_cert_path(&p)));
    }

    #[test]
    fn the_macos_commands_are_exact() {
        let crt = Path::new("/Users/j/.config/hearth/data/tls/ca.crt");
        let keychain = Path::new("/Users/j/Library/Keychains/login.keychain-db");
        assert_eq!(
            macos_trust_args(crt, keychain),
            [
                "add-trusted-cert",
                "-r",
                "trustRoot",
                "-k",
                "/Users/j/Library/Keychains/login.keychain-db",
                "/Users/j/.config/hearth/data/tls/ca.crt"
            ]
            .map(OsString::from)
        );
        assert_eq!(
            macos_check_args(crt),
            ["verify-cert", "-c", "/Users/j/.config/hearth/data/tls/ca.crt", "-p", "basic"].map(OsString::from)
        );
    }
}
