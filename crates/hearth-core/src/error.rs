use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("the home directory of the current user could not be found")]
    NoHomeDir,
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("Hearth is already running")]
    AlreadyRunning,
    #[error("Hearth core is not running")]
    NotRunning,
    #[error("the Hearth endpoint is held by a process of another user")]
    ForeignEndpoint,
    #[error("protocol error: {0}")]
    Protocol(String),
    /// A PHP install, catalog or configuration problem, worded for the user.
    #[error("{0}")]
    Php(String),
    #[error("{message} (code {code})")]
    Rpc { code: i64, message: String },
}

impl CoreError {
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        CoreError::Io { path: path.into(), source }
    }
}
