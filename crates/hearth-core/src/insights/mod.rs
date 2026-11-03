//! What PHP apps report to Hearth while they run: dumps (and, later, requests and queries).

pub mod api;
#[cfg(test)]
mod api_tests;
pub mod collector;
#[cfg(test)]
mod collector_tests;
pub mod logs;
#[cfg(test)]
mod logs_tests;
pub mod prepend;
#[cfg(test)]
mod prepend_tests;
pub mod requests;
#[cfg(test)]
mod requests_tests;
