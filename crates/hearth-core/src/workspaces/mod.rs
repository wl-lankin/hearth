//! Workspaces: a branch in its own git worktree, served at its own .test host, with its own .env
//! and, if wanted, its own database.

pub mod env_file;
pub mod names;

#[cfg(test)]
mod tests;
