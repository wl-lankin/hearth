//! The Hearth core: paths, settings and the local IPC server every frontend talks to.

pub mod error;
pub mod paths;

pub use error::CoreError;
pub use paths::HearthPaths;
