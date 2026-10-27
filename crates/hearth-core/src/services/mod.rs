//! Services Hearth runs next to the sites: databases, Redis, search and S3 storage.

pub mod install;
#[cfg(test)]
mod install_tests;
pub mod kinds;
pub mod store;
#[cfg(test)]
mod tests;
