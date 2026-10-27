//! Services Hearth runs next to the sites: databases, Redis, search and S3 storage.

pub mod install;
#[cfg(test)]
mod install_tests;
pub mod kinds;
pub mod ports;
#[cfg(test)]
mod ports_tests;
pub mod store;
#[cfg(test)]
mod tests;
