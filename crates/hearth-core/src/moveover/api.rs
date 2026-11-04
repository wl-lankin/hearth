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

/// Starts the move as a background job (or joins the one already running) and returns its id.
fn start_apply(core: &CoreState) -> Result<Value, RpcError> {
    let herd = HerdHome::find().ok_or_else(|| RpcError::new(INTERNAL_ERROR, "Herd is not installed here"))?;
    let (id, joined) = core.jobs.start("moveover", "moveover.apply", "Moving over from Herd");
    if joined {
        return Ok(json!({ "jobId": id }));
    }
    let core2 = core.clone();
    let runtime = tokio::runtime::Handle::current();
    tokio::spawn(async move {
        let jobs = core2.jobs.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
            let paths = core2.paths.clone();
            let inv = scan(&herd, &RealMachine)?;
            let hearth_php: Vec<String> = crate::php::store::installed(&paths).into_iter().map(|p| p.minor).collect();
            let plan = plan(&paths, &inv, &hearth_php);
            let mut record = super::record::Record::load(&paths);
            let jobs = core2.jobs.clone();
            let mut mover = super::real::RealMover::new(core2, runtime);
            let mut write = super::machine::RealWrite;
            super::apply::run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut |share, what| {
                jobs.progress(id, share, what)
            })
        })
        .await
        .unwrap_or_else(|e| Err(CoreError::Php(e.to_string())));
        jobs.finish(id, result.map_err(|e| e.to_string()));
    });
    Ok(json!({ "jobId": id }))
}

/// Answers a moveover.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, _params: &Value) -> Option<Result<Value, RpcError>> {
    let result = match method {
        "moveover.state" => {
            let st = state::load(&core.paths);
            Ok(json!({ "herd": HerdHome::find().is_some(), "moved": st.moved, "uninstalled": st.uninstalled }))
        }
        "moveover.scan" => dry_run(core).await,
        "moveover.apply" => start_apply(core),
        _ => return None,
    };
    Some(result)
}
