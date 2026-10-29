//! What `node`, `npm`, `npx` and `corepack` run in a folder: the Node of its version file (or the
//! default) and, for the last three, their script inside that Node.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::{CoreError, HearthPaths};

use super::resolve::{find_spec, resolve};
use super::store::{bin_dir, default_version, installed, node_program};

/// The commands Hearth answers for Node.
pub const COMMANDS: [&str; 4] = ["node", "npm", "npx", "corepack"];

/// A program to start: the arguments go before the user's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub version: String,
    /// Goes first on the child's PATH, so scripts that start `node` again (`npm run`) get the same
    /// version, and that version's global tools.
    pub path_first: PathBuf,
}

/// Where npm and corepack keep their scripts in an install (macOS has them under `lib`).
fn modules(install: &Path) -> PathBuf {
    if cfg!(windows) {
        install.join("node_modules")
    } else {
        install.join("lib").join("node_modules")
    }
}

/// What `command` runs in `cwd`.
pub fn launch(paths: &HearthPaths, command: &str, cwd: &Path) -> Result<Launch, CoreError> {
    let script = match command {
        "node" => None,
        "npm" => Some(["npm", "bin", "npm-cli.js"]),
        "npx" => Some(["npm", "bin", "npx-cli.js"]),
        "corepack" => Some(["corepack", "dist", "corepack.js"]),
        other => return Err(CoreError::Php(format!("Hearth does not run {other}"))),
    };
    let found = find_spec(cwd).transpose()?;
    let wanted = found.as_ref().map(|f| (&f.spec, f.file.as_path()));
    let choice = resolve(&installed(paths), default_version(paths).as_deref(), wanted)?;
    let mut args = Vec::new();
    if let Some(parts) = script {
        let file = parts.iter().fold(modules(&choice.dir), |p, part| p.join(part));
        if !file.is_file() {
            return Err(CoreError::Php(format!("Node {} comes without {command}", choice.version)));
        }
        args.push(file.into_os_string());
    }
    Ok(Launch { program: node_program(&choice.dir), args, path_first: bin_dir(&choice.dir), version: choice.version })
}
