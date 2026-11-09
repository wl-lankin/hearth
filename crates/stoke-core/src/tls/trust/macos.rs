//! The login keychain, through the `security` tool.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{macos_check_args, macos_trust_args};
use crate::CoreError;

fn login_keychain() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("Library").join("Keychains").join("login.keychain-db")
}

pub fn is_trusted(crt: &Path) -> bool {
    Command::new("security").args(macos_check_args(crt)).output().is_ok_and(|o| o.status.success())
}

pub fn trust(crt: &Path) -> Result<(), CoreError> {
    let output = Command::new("security")
        .args(macos_trust_args(crt, &login_keychain()))
        .output()
        .map_err(|e| CoreError::Tls(format!("Could not run security: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    // Cancelling the password prompt fails the command too.
    Err(CoreError::Tls("Trust was not confirmed".into()))
}

pub fn untrust(crt: &Path) {
    let _ = Command::new("security").arg("remove-trusted-cert").arg(crt).output();
}
