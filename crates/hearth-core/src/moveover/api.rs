//! The moveover.* IPC methods.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::runtime::Handle;

use super::check::{node_from, others};
use super::machine::RealMachine;
use super::plan::plan;
use super::probe::{probe_all, targets};
use super::record::Record;
use super::scan::{scan, HerdHome, Machine};
use super::state;
use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR};
use crate::jobs::JobId;
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
    let mut st = state::load(&paths);
    // A move that stopped halfway keeps its recording: Herd is not started again (it would take
    // its ports back), and how the sites answer now is no longer how they answered under Herd.
    if !Record::load(&paths).steps.is_empty() {
        return Ok(json!({ "plan": plan, "baseline": st.baseline, "startedHerd": false }));
    }
    let started_herd = ensure_herd_answers(&inv).await;
    let baseline = probe_all(&targets(&inv), 80, 443).await;
    st.baseline = baseline.clone();
    state::save(&paths, &st).map_err(failed)?;
    Ok(json!({ "plan": plan, "baseline": baseline, "startedHerd": started_herd }))
}

/// Runs `work` as a background job on a blocking thread, or joins the job with this key that is
/// running already; the job's id. `work` gets the core, the runtime and the job's id.
pub(crate) fn in_background<F>(core: &CoreState, key: &str, label: &str, work: F) -> JobId
where
    F: FnOnce(CoreState, Handle, JobId) -> Result<(), CoreError> + Send + 'static,
{
    // `start` says whether the job is new; a job that runs already is joined, not started twice.
    let (id, new) = core.jobs.start("moveover", key, label);
    if !new {
        return id;
    }
    let core2 = core.clone();
    let runtime = Handle::current();
    tokio::spawn(async move {
        let jobs = core2.jobs.clone();
        let result = tokio::task::spawn_blocking(move || work(core2, runtime, id))
            .await
            .unwrap_or_else(|e| Err(CoreError::Php(e.to_string())));
        jobs.finish(id, result.map_err(|e| e.to_string()));
    });
    id
}

/// Starts the move as a background job (or joins the one already running) and returns its id.
fn start_apply(core: &CoreState) -> Result<Value, RpcError> {
    let herd = HerdHome::find().ok_or_else(|| RpcError::new(INTERNAL_ERROR, "Herd is not installed here"))?;
    let id = in_background(core, "moveover.apply", "Moving over from Herd", move |core2, runtime, id| {
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
    });
    Ok(json!({ "jobId": id }))
}

/// Answers a moveover.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let result = match method {
        "moveover.state" => {
            let st = state::load(&core.paths);
            // Started but not finished: the wizard offers to continue or to go back.
            let recorded = !st.moved && !Record::load(&core.paths).steps.is_empty();
            Ok(json!({
                "herd": HerdHome::find().is_some(),
                "moved": st.moved,
                "uninstalled": st.uninstalled,
                "recorded": recorded,
            }))
        }
        "moveover.scan" => dry_run(core).await,
        "moveover.apply" => start_apply(core),
        "moveover.check" => check(core).await,
        "moveover.accept" => accept(core, params),
        "moveover.undo" => start_undo(core),
        "moveover.uninstallHerd" => uninstall_herd(core).await,
        _ => return None,
    };
    Some(result)
}

/// Asks every site again through Hearth and keeps the result for the uninstall.
async fn check(core: &CoreState) -> Result<Value, RpcError> {
    let paths = core.paths.clone();
    let mut st = state::load(&paths);
    let config = crate::sites::registry::load(&paths);
    let sites = super::check::check_sites(&st.baseline, config.http_port, config.https_port).await;
    let services: Vec<(String, bool)> = crate::services::store::load(&paths)
        .into_iter()
        .map(|i| (i.name, !crate::services::ports::port_free(i.port)))
        .collect();
    let machine = RealMachine;
    let hearth_bin = paths.bin.to_string_lossy().into_owned();
    let has_node = |dir: &str| {
        let dir = Path::new(dir.trim());
        ["node.exe", "node.bat", "node.cmd", "node"].iter().any(|name| dir.join(name).is_file())
    };
    let node_from_hearth = node_from(&machine.machine_path(), &machine.user_path(), &hearth_bin, has_node);
    st.last_check = sites.iter().cloned().chain(others(&services, node_from_hearth)).collect();
    st.checked = st.moved;
    state::save(&paths, &st).map_err(failed)?;
    let services: Vec<Value> =
        services.iter().map(|(name, running)| json!({ "name": name, "running": running })).collect();
    Ok(
        json!({ "sites": sites, "services": services, "node": { "fromHearth": node_from_hearth }, "accepted": st.accepted }),
    )
}

fn accept(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let key = params
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(crate::ipc::INVALID_PARAMS, "key is required"))?;
    let mut st = state::load(&core.paths);
    if !st.accepted.iter().any(|k| k == key) {
        st.accepted.push(key.to_string());
    }
    state::save(&core.paths, &st).map_err(failed)?;
    Ok(json!({ "accepted": st.accepted }))
}

/// Undo as a background job; its id.
fn start_undo(core: &CoreState) -> Result<Value, RpcError> {
    let id = in_background(core, "moveover.undo", "Going back to Herd", |core2, runtime, _| {
        let paths = core2.paths.clone();
        let mut rev = super::real::RealMover::new(core2, runtime);
        super::undo::undo(&paths, &mut rev, &mut super::machine::RealWrite)
    });
    Ok(json!({ "jobId": id }))
}

/// Runs Herd's uninstaller (once the last check allows it), then checks again.
async fn uninstall_herd(core: &CoreState) -> Result<Value, RpcError> {
    let herd = HerdHome::find().ok_or_else(|| RpcError::new(INTERNAL_ERROR, "Herd is not installed here"))?;
    let paths = core.paths.clone();
    let inv = tokio::task::spawn_blocking(move || scan(&herd, &RealMachine))
        .await
        .map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))?
        .map_err(failed)?;
    let uninstaller =
        inv.uninstaller.ok_or_else(|| RpcError::new(INTERNAL_ERROR, "Herd's uninstaller could not be found"))?;
    let p2 = paths.clone();
    let done = tokio::task::spawn_blocking(move || {
        let mut real = super::real::RealUninstaller { paths: p2.clone() };
        super::uninstall::uninstall(&p2, &uninstaller.program, &uninstaller.args, &mut real)
    })
    .await
    .map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))?
    .map_err(failed)?;
    let after = check(core).await?;
    Ok(json!({ "exitCode": done.code, "trustError": done.trust_error, "check": after }))
}
