//! The mail catcher: mail sent by local apps is kept for the Mail page instead of being delivered.

pub mod api;
#[cfg(test)]
mod api_tests;
pub mod smtp;
#[cfg(test)]
mod smtp_tests;
pub mod store;
#[cfg(test)]
mod store_tests;
