//! Services Hearth runs next to the sites: databases, Redis, search and S3 storage.

pub mod api;
#[cfg(test)]
mod api_tests;
pub mod install;
#[cfg(test)]
mod install_tests;
pub mod kinds;
pub mod ports;
#[cfg(test)]
mod ports_tests;
pub mod run;
#[cfg(test)]
mod run_tests;
pub mod store;
#[cfg(test)]
mod tests;

/// Tests that start service processes on real ports run one at a time, across modules.
#[cfg(test)]
pub(crate) static PROCESS_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
