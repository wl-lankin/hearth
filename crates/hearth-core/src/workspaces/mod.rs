//! Workspaces: a branch in its own git worktree, served at its own .test host, with its own .env
//! and, if wanted, its own database.

pub mod api;
pub mod database;
pub mod env_file;
pub mod git;
pub mod names;
pub mod registry;
pub mod service;

/// A program run without a console window flashing up: the core runs inside the app, a windowed
/// program, and a create runs git and database tools a dozen times.
pub(crate) fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(test)]
mod database_tests;
#[cfg(test)]
pub(crate) mod git_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;
