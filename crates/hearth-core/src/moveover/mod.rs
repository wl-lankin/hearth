//! Moving over from Herd: read all of Herd (scan), show what would happen (plan), carry it out
//! with a record of every change outside Hearth's home (apply), then check, undo or uninstall Herd.

pub mod machine;
pub mod scan;

#[cfg(test)]
pub(crate) mod scan_tests;
