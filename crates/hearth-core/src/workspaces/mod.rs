//! Workspaces: a branch in its own git worktree, served at its own .test host, with its own .env
//! and, if wanted, its own database.

pub mod api;
pub mod database;
pub mod env_file;
pub mod git;
pub mod names;
pub mod registry;
pub mod service;

#[cfg(test)]
mod database_tests;
#[cfg(test)]
pub(crate) mod git_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;
