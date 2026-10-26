//! Making `<site>.test` resolve: a managed block in the hosts file (Windows) or a small resolver
//! (macOS).

use std::collections::BTreeSet;

use serde::Serialize;

use crate::CoreError;

#[cfg(windows)]
pub mod client;
pub mod hosts;
pub mod protocol;
pub mod resolver;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsStatus {
    /// `<site>.test` resolves to this computer.
    pub ready: bool,
    /// Why not, worded for the user.
    pub problem: Option<String>,
}

/// Makes `names` (full host names) resolve. Windows: through the helper on `pipe`. macOS: the
/// responder answers every `.test` name, so this only checks the resolver file.
pub fn sync_names(pipe: &str, names: &BTreeSet<String>) -> DnsStatus {
    #[cfg(windows)]
    let result = client::set_names_on(pipe, names);
    #[cfg(target_os = "macos")]
    let result = {
        let _ = (pipe, names);
        resolver::check()
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let result: Result<(), CoreError> = {
        let _ = (pipe, names);
        Err(CoreError::Php(".test names are not supported here".into()))
    };
    match result {
        Ok(()) => DnsStatus { ready: true, problem: None },
        Err(e) => DnsStatus { ready: false, problem: Some(e.to_string()) },
    }
}

/// The one-time setup, after the OS asks the user (Windows: UAC; macOS: the admin password).
pub fn setup() -> Result<(), CoreError> {
    #[cfg(windows)]
    return client::install();
    #[cfg(target_os = "macos")]
    return resolver::install();
    #[cfg(not(any(windows, target_os = "macos")))]
    Err(CoreError::Php(".test names are not supported here".into()))
}
