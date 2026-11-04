//! Moving over from Herd: read all of Herd (scan), show what would happen (plan), carry it out
//! with a record of every change outside Hearth's home (apply), then check, undo or uninstall Herd.

pub mod api;
pub mod machine;
pub mod outside;
pub mod plan;
pub mod probe;
pub mod record;
pub mod scan;
pub mod state;

#[cfg(test)]
mod outside_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
pub(crate) mod scan_tests;
