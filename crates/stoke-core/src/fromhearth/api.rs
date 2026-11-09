//! The move from Hearth over IPC: its record for the app's note, "Try again", and closing the note.

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};

use super::real::RealOutside;
use super::{finish, Record};
use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR};
use crate::StokePaths;

/// True while the steps after the start run (they ask for UAC and wait for an uninstaller).
static RUNNING: AtomicBool = AtomicBool::new(false);

fn status(paths: &StokePaths) -> Value {
    match Record::load(paths) {
        Some(record) => json!({
            "moved": true,
            "pending": record.pending(),
            "running": RUNNING.load(Ordering::SeqCst),
            "errors": record.errors,
            "seen": record.seen,
        }),
        None => json!({ "moved": false }),
    }
}

/// Whether the record is of the real Hearth home. Only then do the steps change this computer
/// (PATH, login entry, MCP entries, helper, Hearth's app): a test move of a copy never does.
fn real_move(paths: &StokePaths) -> bool {
    let default = dirs::home_dir().map(|h| h.join(".config").join("hearth"));
    Record::load(paths).is_some_and(|r| Some(r.legacy) == default)
}

/// Carries out what is left of the move, once at a time; `false` when it was already running or
/// the move was a test move.
pub fn finish_now(paths: &StokePaths) -> bool {
    if !real_move(paths) || RUNNING.swap(true, Ordering::SeqCst) {
        return false;
    }
    finish(paths, &mut RealOutside);
    RUNNING.store(false, Ordering::SeqCst);
    true
}

/// Starts what is left of the move on its own thread, when anything is.
pub fn finish_in_background(paths: &StokePaths) {
    if Record::load(paths).is_some_and(|r| r.pending()) {
        let paths = paths.clone();
        std::thread::spawn(move || finish_now(&paths));
    }
}

pub async fn handle(core: &CoreState, method: &str, _params: &Value) -> Option<Result<Value, RpcError>> {
    let paths = core.paths.clone();
    let result = match method {
        "fromhearth.status" => Ok(status(&paths)),
        "fromhearth.retry" => {
            let done = paths.clone();
            match tokio::task::spawn_blocking(move || finish_now(&done)).await {
                Ok(_) => Ok(status(&paths)),
                Err(e) => Err(RpcError::new(INTERNAL_ERROR, e.to_string())),
            }
        }
        "fromhearth.seen" => match Record::load(&paths) {
            Some(mut record) => {
                record.seen = true;
                record.save(&paths).map(|()| status(&paths)).map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))
            }
            None => Ok(status(&paths)),
        },
        _ => return None,
    };
    Some(result)
}
