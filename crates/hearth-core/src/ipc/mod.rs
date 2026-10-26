//! The local IPC every frontend (app UI, `hearth` CLI, MCP server) uses to talk to the core.

mod client;
pub(crate) mod endpoint;
mod lines;
mod peer;
pub mod protocol;
mod server;

use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};

pub use client::call;
pub use endpoint::endpoint_name;
pub use protocol::{RpcError, INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR};
pub use server::{serve, Handler, HandlerFuture, ServerHandle};

use crate::jobs::Jobs;
use crate::php::manifest::{CATALOG_PUBLIC_KEY, CATALOG_URL};
use crate::supervisor::Supervisor;
use crate::web::service::WebStatus;
use crate::HearthPaths;

/// Where the PHP catalog comes from and which key must have signed it.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub url: String,
    pub public_key: String,
}

impl Default for Catalog {
    fn default() -> Self {
        Catalog { url: CATALOG_URL.into(), public_key: CATALOG_PUBLIC_KEY.into() }
    }
}

/// What the core knows about itself. Cheap to clone: the job list is shared.
#[derive(Clone)]
pub struct CoreState {
    pub paths: HearthPaths,
    pub started: Instant,
    pub jobs: Arc<Jobs>,
    pub catalog: Catalog,
    /// One writer at a time for PHP installs, php.json, php.ini and the shims.
    pub php_lock: Arc<std::sync::Mutex<()>>,
    /// One download at a time of the shared tools (Composer, the CA bundle).
    pub tools_lock: Arc<tokio::sync::Mutex<()>>,
    /// One writer at a time for sites.json, and one web server sync at a time.
    pub sites_lock: Arc<std::sync::Mutex<()>>,
    /// Caddy and the PHP pools.
    pub supervisor: Arc<Supervisor>,
    /// The latest web server sync result, for `web.status`.
    pub web_status: Arc<std::sync::Mutex<Option<WebStatus>>>,
    /// Sync the web server after site and PHP changes. Off by default (tests, CLI-only setups);
    /// the app turns it on.
    pub web_sync: bool,
    /// Where site names are pushed so `<name>.test` resolves: the Windows helper's pipe (on macOS
    /// any value checks the resolver file). `None`, the default, leaves the system alone (tests);
    /// the app sets it.
    pub dns_pipe: Option<String>,
    /// Set while Hearth quits: no new syncs (they could start Caddy or PHP again).
    pub quitting: Arc<std::sync::atomic::AtomicBool>,
}

impl CoreState {
    pub fn new(paths: HearthPaths) -> CoreState {
        let supervisor = Arc::new(
            // Only fails when Windows refuses a Job Object, which leaves Hearth unable to run anything safely.
            Supervisor::new_in(Some(paths.run.clone())).expect("Hearth could not set up its process supervisor"),
        );
        CoreState {
            paths,
            started: Instant::now(),
            jobs: Arc::default(),
            catalog: Catalog::default(),
            php_lock: Arc::default(),
            tools_lock: Arc::default(),
            sites_lock: Arc::default(),
            supervisor,
            web_status: Arc::default(),
            web_sync: false,
            dns_pipe: None,
            quitting: Arc::default(),
        }
    }
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
    Arc::new(move |method, params| {
        let state = state.clone();
        Box::pin(async move {
            match method.as_str() {
                "core.ping" => Ok(json!("pong")),
                "core.status" => Ok(status_of(&state)),
                other => match crate::php::api::handle(&state, other, &params).await {
                    Some(result) => result,
                    None => match crate::sites::api::handle(&state, other, &params).await {
                        Some(result) => result,
                        None => Err(RpcError::new(METHOD_NOT_FOUND, format!("unknown method: {other}"))),
                    },
                },
            }
        })
    })
}

#[cfg(test)]
mod tests;
