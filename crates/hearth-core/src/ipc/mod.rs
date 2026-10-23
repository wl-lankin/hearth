//! The local IPC every frontend (app UI, `hearth` CLI, MCP server) uses to talk to the core.

mod client;
pub(crate) mod endpoint;
mod lines;
pub mod protocol;
mod server;

use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};

pub use client::call;
pub use endpoint::endpoint_name;
pub use protocol::{RpcError, INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR};
pub use server::{serve, Handler, HandlerFuture, ServerHandle};

use crate::HearthPaths;

/// What the core knows about itself.
#[derive(Debug, Clone)]
pub struct CoreState {
    pub paths: HearthPaths,
    pub started: Instant,
}

/// The `core.status` object, shared by the IPC method and in-process callers (the app UI).
pub fn status_of(state: &CoreState) -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
        "uptimeSecs": state.started.elapsed().as_secs(),
        "home": state.paths.root.to_string_lossy(),
    })
}

/// The handler for the methods the core answers itself.
pub fn router(state: CoreState) -> Handler {
    let state = Arc::new(state);
    Arc::new(move |method, _params| {
        let state = state.clone();
        Box::pin(async move {
            match method.as_str() {
                "core.ping" => Ok(json!("pong")),
                "core.status" => Ok(status_of(&state)),
                other => Err(RpcError::new(METHOD_NOT_FOUND, format!("unknown method: {other}"))),
            }
        })
    })
}

#[cfg(test)]
mod tests;
