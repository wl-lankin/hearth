//! The Hearth core: paths, settings and the local IPC server every frontend talks to.

pub mod atomic;
pub mod cli_install;
pub mod dns;
pub mod error;
pub mod herd;
pub mod ipc;
pub mod jobs;
pub mod node;
pub mod path_env;
pub mod paths;
pub mod php;
pub mod services;
pub mod settings;
pub mod sites;
pub mod supervisor;
#[cfg(test)]
mod supervisor_tests;
pub mod tls;
pub mod web;

pub use error::CoreError;
pub use paths::HearthPaths;
pub use settings::{Settings, Theme};
