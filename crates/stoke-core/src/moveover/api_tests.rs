use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::api::in_background;
use crate::ipc::CoreState;
use crate::jobs::JobState;
use crate::StokePaths;

async fn wait_done(core: &CoreState, id: u64) -> JobState {
    for _ in 0..200 {
        let state = core.jobs.get(id).map(|j| j.state).expect("the job");
        if state != JobState::Running {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    JobState::Running
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_job_runs_its_work_and_a_second_start_joins_it() {
    let d = tempfile::tempdir().unwrap();
    let core = CoreState::new(StokePaths::at(d.path().join("stoke")));
    let runs = Arc::new(AtomicUsize::new(0));
    let (gate_tx, gate_rx) = std::sync::mpsc::channel::<()>();

    let counted = runs.clone();
    let first = in_background(&core, "moveover.apply", "Moving over from Herd", move |_, _, _| {
        counted.fetch_add(1, Ordering::SeqCst);
        let _ = gate_rx.recv_timeout(Duration::from_secs(5));
        Ok(())
    });
    let again = runs.clone();
    let second = in_background(&core, "moveover.apply", "Moving over from Herd", move |_, _, _| {
        again.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    assert_eq!(first, second, "a second start joins the running job");
    gate_tx.send(()).unwrap();
    assert_eq!(wait_done(&core, first).await, JobState::Done);
    assert_eq!(runs.load(Ordering::SeqCst), 1, "the work ran, once");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_job_says_why() {
    let d = tempfile::tempdir().unwrap();
    let core = CoreState::new(StokePaths::at(d.path().join("stoke")));
    let id = in_background(&core, "moveover.undo", "Going back to Herd", |_, _, _| {
        Err(crate::CoreError::Php("PHP 8.1 could not be removed".into()))
    });
    assert_eq!(wait_done(&core, id).await, JobState::Failed);
    assert_eq!(core.jobs.get(id).and_then(|j| j.error).as_deref(), Some("PHP 8.1 could not be removed"));
}
