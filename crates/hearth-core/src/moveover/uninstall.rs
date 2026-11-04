//! Uninstalling Herd: only once the check passed (or its red rows were accepted), by Herd's own
//! uninstaller. Afterwards Hearth makes sure the CA it took over from Herd is still trusted.

use std::path::Path;

use super::check::{gate, SiteCheck};
use super::state;
use crate::{CoreError, HearthPaths};

/// Runs the uninstaller and handles trust; the real one is this computer, tests pass their own.
pub trait Uninstaller {
    /// Runs the uninstaller and waits for it; its exit code.
    fn run(&mut self, program: &Path, args: &[String]) -> Result<i32, CoreError>;
    fn ca_trusted(&self) -> bool;
    fn trust(&mut self) -> Result<(), CoreError>;
}

/// Runs Herd's uninstaller once `checks` allow it. A non-zero exit (cancelled, failed) leaves Herd
/// and Undo as they were.
pub fn uninstall(
    paths: &HearthPaths,
    checks: &[SiteCheck],
    program: &Path,
    args: &[String],
    u: &mut dyn Uninstaller,
) -> Result<i32, CoreError> {
    let mut st = state::load(paths);
    gate(checks, &st.accepted).map_err(CoreError::Php)?;
    let code = u.run(program, args)?;
    if code != 0 {
        return Ok(code);
    }
    st.uninstalled = true;
    state::save(paths, &st)?;
    // Herd's uninstaller may take its CA, now Hearth's, out of the trust store.
    if !u.ca_trusted() {
        u.trust()?;
    }
    Ok(code)
}
