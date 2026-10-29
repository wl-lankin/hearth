//! Node.js versions from the signed catalog, the default version and the `current` link.

pub mod api;
#[cfg(test)]
mod api_tests;
pub mod install;
pub mod launch;
#[cfg(test)]
mod launch_tests;
pub mod resolve;
#[cfg(test)]
mod resolve_tests;
pub mod store;
#[cfg(test)]
mod tests;
