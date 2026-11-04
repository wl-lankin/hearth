//! The moveover.* IPC methods.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::machine::RealMachine;
use super::plan::plan;
use super::probe::{probe_all, targets};
use super::scan::{scan, HerdHome};
use super::state;
use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR};
use crate::CoreError;

fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

fn port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

/// Starts Herd's app so its sites answer for the recording; true when it had to be started.
async fn ensure_herd_answers(inv: &super::scan::Inventory) -> bool {
    if port_open(80) {
        return false;
    }
    let Some(app) = inv.uninstaller.as_ref().and_then(|u| u.program.parent()).map(|d| d.join("Herd.exe")) else {
        return false;
    };
    if crate::workspaces::command(&app).spawn().is_err() {
        return false;
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline && !port_open(80) {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    true
}

async fn dry_run(core: &CoreState) -> Result<Value, RpcError> {
    let herd = HerdHome::find().ok_or_else(|| RpcError::new(INTERNAL_ERROR, "Herd is not installed here"))?;
    let paths = core.paths.clone();
    let inv = tokio::task::spawn_blocking(move || scan(&herd, &RealMachine))
        .await
        .map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))?
        .map_err(failed)?;
    let hearth_php: Vec<String> = crate::php::store::installed(&paths).into_iter().map(|p| p.minor).collect();
    let plan = plan(&paths, &inv, &hearth_php);
    let started_herd = ensure_herd_answers(&inv).await;
    let baseline = probe_all(&targets(&inv), 80, 443).await;
    let mut st = state::load(&paths);
    st.baseline = baseline.clone();
    state::save(&paths, &st).map_err(failed)?;
    Ok(json!({ "plan": plan, "baseline": baseline, "startedHerd": started_herd }))
}

/// Answers a moveover.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, _params: &Value) -> Option<Result<Value, RpcError>> {
    let result = match method {
        "moveover.state" => {
            let st = state::load(&core.paths);
            Ok(json!({ "herd": HerdHome::find().is_some(), "moved": st.moved, "uninstalled": st.uninstalled }))
        }
        "moveover.scan" => dry_run(core).await,
        _ => return None,
    };
    Some(result)
}
