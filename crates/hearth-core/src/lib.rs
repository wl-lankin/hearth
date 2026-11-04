//! The Hearth core: paths, settings and the local IPC server every frontend talks to.

pub mod atomic;
pub mod cli_install;
pub mod db;
pub mod debug;
pub mod dns;
pub mod dotenv;
pub mod env;
pub mod error;
pub mod herd;
pub mod insights;
pub mod ipc;
pub mod jobs;
pub mod mail;
pub mod node;
pub mod path_env;
pub mod paths;
pub mod php;
pub mod processes;
pub mod profile;
pub mod services;
pub mod settings;
pub mod sites;
pub mod supervisor;
#[cfg(test)]
mod supervisor_tests;
pub mod tls;
pub mod web;
pub mod workspaces;

pub use error::CoreError;
pub use paths::HearthPaths;
pub use settings::{Settings, Theme};
