//! Undo: walks the record backwards and puts every change back, then starts Herd again. Possible until
//! Herd is uninstalled.

use super::outside::{undo_outside, MachineWrite};
use super::record::{Done, Record};
use super::state;
use crate::herd::import::ImportPlan;
use crate::{CoreError, HearthPaths};

/// What Undo asks of Hearth; the real one is the same as the move's, tests pass their own.
pub trait Reverter {
    fn start_herd(&mut self, app: &str) -> Result<(), CoreError>;
    /// Unparks, unlinks, unproxies, unisolates and unsecures what the import added.
    fn remove_sites(&mut self, plan: &ImportPlan) -> Result<(), CoreError>;
    /// Hearth's own CA back (or none, so Hearth makes a new one when it needs it).
    fn restore_ca(&mut self, previous: Option<&(Vec<u8>, Vec<u8>)>) -> Result<(), CoreError>;
    fn set_ports(&mut self, http: u16, https: u16) -> Result<(), CoreError>;
    /// Nothing to do (not an error) when the version is not installed.
    fn uninstall_php(&mut self, minor: &str) -> Result<(), CoreError>;
    fn set_php_default(&mut self, minor: Option<&str>) -> Result<(), CoreError>;
    fn set_ini_user(&mut self, minor: &str, user: &str) -> Result<(), CoreError>;
    /// Nothing to do when the service is gone already.
    fn delete_service(&mut self, id: &str) -> Result<(), CoreError>;
    fn set_smtp(&mut self, port: u16) -> Result<(), CoreError>;
    /// Nothing to do when the version is not installed.
    fn uninstall_node(&mut self, version: &str) -> Result<(), CoreError>;
    fn set_node_default(&mut self, version: Option<&str>) -> Result<(), CoreError>;
    fn remove_forwarders(&mut self) -> Result<(), CoreError>;
    fn sync(&mut self);
}

/// Puts back everything the record holds, newest first, and forgets the record. Each step leaves
/// the record once it is put back, so an Undo that fails halfway continues there the next time.
pub fn undo(paths: &HearthPaths, rev: &mut dyn Reverter, outside: &mut dyn MachineWrite) -> Result<(), CoreError> {
    let mut st = state::load(paths);
    if st.uninstalled {
        return Err(CoreError::Php("Herd is uninstalled: there is nothing to go back to".into()));
    }
    let mut record = Record::load(paths);
    while let Some(step) = record.steps.last().cloned() {
        match &step {
            Done::UserPath { .. } | Done::MachineEnv { .. } | Done::Autostart { .. } | Done::Mcp { .. } => {
                undo_outside(outside, &step)?
            }
            Done::Forwarders { .. } => rev.remove_forwarders()?,
            Done::Node { installed, default_before } => {
                for version in installed {
                    rev.uninstall_node(version)?;
                }
                rev.set_node_default(default_before.as_deref())?;
            }
            Done::Smtp { previous } => rev.set_smtp(*previous)?,
            Done::Redis { id, created: true } => rev.delete_service(id)?,
            // There before the move: it stays.
            Done::Redis { created: false, .. } => {}
            Done::Php { installed, default_before, ini_before } => {
                for (minor, user) in ini_before {
                    rev.set_ini_user(minor, user)?;
                }
                rev.set_php_default(default_before.as_deref())?;
                for minor in installed {
                    rev.uninstall_php(minor)?;
                }
            }
            Done::Ports { http, https } => rev.set_ports(*http, *https)?,
            Done::Ca { previous } => rev.restore_ca(previous.as_ref())?,
            Done::Imported { plan } => rev.remove_sites(plan)?,
            Done::HerdStopped { app } => {
                if let Some(app) = app {
                    rev.start_herd(app)?;
                }
            }
        }
        record.pop(paths)?;
    }
    rev.sync();
    Record::clear(paths)?;
    st.moved = false;
    st.accepted.clear();
    state::save(paths, &st)
}
