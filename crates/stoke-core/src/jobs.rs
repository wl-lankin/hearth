//! Long-running work (downloads, installs) the frontends follow by polling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

pub type JobId = u64;

/// Finished jobs stay visible this long, so a frontend that polls late still sees the outcome.
const KEEP_FINISHED: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub id: JobId,
    pub kind: String,
    /// Identifies the work; one running job per key (e.g. "php.install:8.4").
    pub key: String,
    pub label: String,
    pub progress: f64,
    pub message: String,
    pub state: JobState,
    pub error: Option<String>,
    #[serde(skip)]
    finished: Option<Instant>,
}

#[derive(Default)]
pub struct Jobs {
    next: AtomicU64,
    jobs: Mutex<HashMap<JobId, JobStatus>>,
}

impl Jobs {
    /// Starts a job, or returns the running one with the same key. The bool is true for a new job.
    pub fn start(&self, kind: &str, key: &str, label: &str) -> (JobId, bool) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|_, j| j.finished.is_none_or(|t| t.elapsed() < KEEP_FINISHED));
        if let Some(running) = jobs.values().find(|j| j.key == key && j.state == JobState::Running) {
            return (running.id, false);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        jobs.insert(
            id,
            JobStatus {
                id,
                kind: kind.into(),
                key: key.into(),
                label: label.into(),
                progress: 0.0,
                message: String::new(),
                state: JobState::Running,
                error: None,
                finished: None,
            },
        );
        (id, true)
    }

    pub fn progress(&self, id: JobId, progress: f64, message: &str) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(&id) {
            job.progress = progress.clamp(0.0, 1.0);
            job.message = message.into();
        }
    }

    pub fn finish(&self, id: JobId, result: Result<(), String>) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(&id) {
            job.finished = Some(Instant::now());
            match result {
                Ok(()) => {
                    job.state = JobState::Done;
                    job.progress = 1.0;
                }
                Err(e) => {
                    job.state = JobState::Failed;
                    job.error = Some(e);
                }
            }
        }
    }

    pub fn get(&self, id: JobId) -> Option<JobStatus> {
        self.jobs.lock().unwrap().get(&id).cloned()
    }

    /// All known jobs, oldest first.
    pub fn list(&self) -> Vec<JobStatus> {
        let mut list: Vec<JobStatus> = self.jobs.lock().unwrap().values().cloned().collect();
        list.sort_by_key(|j| j.id);
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_start_with_the_same_key_returns_the_running_job() {
        let jobs = Jobs::default();
        let (a, new_a) = jobs.start("php.install", "php.install:8.4", "Installing PHP 8.4");
        let (b, new_b) = jobs.start("php.install", "php.install:8.4", "Installing PHP 8.4");
        assert_eq!((a, new_a, new_b), (b, true, false));
        let (c, _) = jobs.start("php.install", "php.install:8.3", "Installing PHP 8.3");
        assert_ne!(a, c);
    }

    #[test]
    fn a_finished_job_frees_its_key_and_keeps_its_outcome() {
        let jobs = Jobs::default();
        let (a, _) = jobs.start("php.install", "k", "x");
        jobs.progress(a, 0.5, "Downloading");
        assert_eq!(jobs.get(a).unwrap().progress, 0.5);
        jobs.finish(a, Err("The download is damaged".into()));
        let failed = jobs.get(a).unwrap();
        assert_eq!((failed.state, failed.error.as_deref()), (JobState::Failed, Some("The download is damaged")));
        let (b, new) = jobs.start("php.install", "k", "x");
        assert!(new && b != a);
        jobs.finish(b, Ok(()));
        assert_eq!(jobs.get(b).unwrap().progress, 1.0);
    }
}
