//! Node.js versions from the signed catalog, the default version and the `current` link.

pub mod install;
pub mod resolve;
#[cfg(test)]
mod resolve_tests;
pub mod store;
#[cfg(test)]
mod tests;
