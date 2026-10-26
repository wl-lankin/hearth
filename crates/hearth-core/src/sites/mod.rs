//! Local sites: parked folders, linked folders and proxies, and their web server config.

pub mod api;
#[cfg(test)]
mod api_tests;
pub mod caddyfile;
#[cfg(test)]
mod caddyfile_tests;
pub mod detect;
pub mod registry;
#[cfg(test)]
mod tests;
