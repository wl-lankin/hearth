//! The local web server: Caddy plus PHP FastCGI pools, kept in step with the sites.

pub mod caddy_install;
#[cfg(test)]
mod caddy_install_tests;
pub mod pools;
#[cfg(test)]
mod pools_tests;
pub mod service;
#[cfg(test)]
mod service_tests;
