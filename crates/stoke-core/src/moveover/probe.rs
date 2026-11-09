//! How a site answers: its status and where it redirects, asked at 127.0.0.1 with the site's name,
//! so the same request goes to Herd before the move and to Stoke after it.

use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub host: String,
    pub scheme: String,
    pub status: Option<u16>,
    pub location: Option<String>,
    pub error: Option<String>,
}

/// Asks `host` once, without following a redirect. Local `.test` certificates are not checked:
/// the answer is what matters, and Herd's and Stoke's certificates differ.
pub async fn probe(host: &str, secure: bool, http_port: u16, https_port: u16) -> Answer {
    let (scheme, port) = if secure { ("https", https_port) } else { ("http", http_port) };
    let answer = |status, location, error| Answer { host: host.into(), scheme: scheme.into(), status, location, error };
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .resolve(host, SocketAddr::from(([127, 0, 0, 1], port)))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => return answer(None, None, Some(e.to_string())),
    };
    match client.get(format!("{scheme}://{host}:{port}/")).send().await {
        Ok(response) => {
            let location =
                response.headers().get(reqwest::header::LOCATION).and_then(|l| l.to_str().ok()).map(str::to_string);
            answer(Some(response.status().as_u16()), location, None)
        }
        Err(e) => answer(None, None, Some(if e.is_timeout() { "No answer within 10 s".into() } else { e.to_string() })),
    }
}

/// Every site to ask: over HTTP, and over HTTPS too where it is secured.
pub fn targets(inv: &super::scan::Inventory) -> Vec<(String, bool)> {
    let mut targets = Vec::new();
    for host in super::scan::site_hosts(&inv.sites) {
        let name = host.trim_end_matches(".test");
        targets.push((host.clone(), false));
        if inv.sites.secured.contains(name) {
            targets.push((host, true));
        }
    }
    targets
}

/// Asks every target, eight at a time.
pub async fn probe_all(targets: &[(String, bool)], http_port: u16, https_port: u16) -> Vec<Answer> {
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
    let mut tasks = tokio::task::JoinSet::new();
    for (index, (host, secure)) in targets.iter().cloned().enumerate() {
        let gate = gate.clone();
        tasks.spawn(async move {
            let _permit = gate.acquire_owned().await;
            (index, probe(&host, secure, http_port, https_port).await)
        });
    }
    let mut answers: Vec<(usize, Answer)> = tasks.join_all().await;
    answers.sort_by_key(|(index, _)| *index);
    answers.into_iter().map(|(_, a)| a).collect()
}
