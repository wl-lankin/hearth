//! Uninstalling Herd: only after the move and a check that passed (or whose red rows were
//! accepted), by Herd's own uninstaller. Whether Herd is gone is read from the computer afterwards,
//! not from the uninstaller's exit code (an uninstaller that hands over to a copy of itself exits 0
//! at once). Afterwards Stoke makes sure the CA it took over from Herd is still trusted, and
//! forgets what Undo would have needed.

use std::path::Path;

use super::check::gate;
use super::record::Record;
use super::state;
use crate::{CoreError, StokePaths};

/// Runs the uninstaller and handles trust; the real one is this computer, tests pass their own.
pub trait Uninstaller {
    /// Runs the uninstaller and waits for it; its exit code.
    fn run(&mut self, program: &Path, args: &[String]) -> Result<i32, CoreError>;
    /// Whether Herd is still installed after the uninstaller ran (`program` is its uninstaller).
    fn still_there(&self, program: &Path) -> bool;
    fn ca_trusted(&self) -> bool;
    fn trust(&mut self) -> Result<(), CoreError>;
    /// Lets the helper forget the machine environment entries it took out of Herd's.
    fn forget_machine_env(&mut self) -> Result<(), CoreError>;
}

/// How the uninstall went: `code` 0 when Herd is gone; `trust_error` when Stoke's CA could not be
/// trusted again afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uninstalled {
    pub code: i32,
    pub trust_error: Option<String>,
}

/// The code for an uninstaller that exited 0 while Herd stayed (cancelled in its window).
const STILL_THERE: i32 = 1;

/// Runs Herd's uninstaller once the last check allows it. If Herd stays (cancelled, failed), Herd
/// and Undo are left as they were.
pub fn uninstall(
    paths: &StokePaths,
    program: &Path,
    args: &[String],
    u: &mut dyn Uninstaller,
) -> Result<Uninstalled, CoreError> {
    let mut st = state::load(paths);
    if !st.moved {
        return Err(CoreError::Php("Nothing has moved over yet".into()));
    }
    if !st.checked {
        return Err(CoreError::Php("Check the sites first".into()));
    }
    gate(&st.last_check, &st.accepted).map_err(CoreError::Php)?;
    let code = u.run(program, args)?;
    if code != 0 {
        return Ok(Uninstalled { code, trust_error: None });
    }
    if u.still_there(program) {
        return Ok(Uninstalled { code: STILL_THERE, trust_error: None });
    }
    st.uninstalled = true;
    state::save(paths, &st)?;
    // Nothing to go back to: the record (with Stoke's CA key from before) and the helper's go.
    Record::clear(paths)?;
    let _ = u.forget_machine_env();
    // Herd's uninstaller may take its CA, now Stoke's, out of the trust store.
    let trust_error = if u.ca_trusted() { None } else { u.trust().err().map(|e| e.to_string()) };
    Ok(Uninstalled { code: 0, trust_error })
}
