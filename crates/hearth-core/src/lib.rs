//! The Hearth core: paths, settings and the local IPC server every frontend talks to.

pub mod atomic;
pub mod error;
pub mod paths;
pub mod settings;

pub use error::CoreError;
pub use paths::HearthPaths;
pub use settings::{Settings, Theme};
