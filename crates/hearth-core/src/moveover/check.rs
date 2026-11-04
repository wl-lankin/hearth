//! The check after the move: each site asked again, now through Hearth, and compared with how it
//! answered under Herd before the move.

use serde::{Deserialize, Serialize};

use super::probe::{probe_all, Answer};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum Verdict {
    /// Answers as under Herd (also when it failed the same way there).
    Same,
    /// Failed under Herd, answers now.
    Better,
    /// Answers, but differently: another status class or another redirect.
    Changed(String),
    /// Answered under Herd, does not now.
    Broken(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteCheck {
    pub host: String,
    pub scheme: String,
    pub verdict: Verdict,
}

impl SiteCheck {
    /// `https://shop.test`: how a check is named (and accepted).
    pub fn key(&self) -> String {
        format!("{}://{}", self.scheme, self.host)
    }
}

fn describe(a: &Answer) -> String {
    match (a.status, &a.location) {
        (Some(s), Some(l)) => format!("{s} to {l}"),
        (Some(s), None) => s.to_string(),
        (None, _) => a.error.clone().unwrap_or_else(|| "no answer".into()),
    }
}

/// How `after` (Hearth) compares with `before` (Herd): by status class and redirect target, not by
/// page content.
pub fn compare(before: &Answer, after: &Answer) -> Verdict {
    match (before.status, after.status) {
        (None, None) => Verdict::Same,
        (None, Some(_)) => Verdict::Better,
        (Some(_), None) => Verdict::Broken(format!("{} (was {})", describe(after), describe(before))),
        (Some(b), Some(a)) if b / 100 != a / 100 || (b / 100 == 3 && before.location != after.location) => {
            Verdict::Changed(format!("{} instead of {}", describe(after), describe(before)))
        }
        _ => Verdict::Same,
    }
}

/// The uninstall may go ahead: every check answers as before (or better), or was accepted.
pub fn gate(checks: &[SiteCheck], accepted: &[String]) -> Result<(), String> {
    for check in checks {
        let detail = match &check.verdict {
            Verdict::Changed(d) | Verdict::Broken(d) => d,
            _ => continue,
        };
        if !accepted.contains(&check.key()) {
            return Err(format!("{} still needs a look: {detail}", check.key()));
        }
    }
    Ok(())
}

/// Asks every site of the recording again, now on Hearth's ports, and compares.
pub async fn check_sites(baseline: &[Answer], http_port: u16, https_port: u16) -> Vec<SiteCheck> {
    let targets: Vec<(String, bool)> = baseline.iter().map(|a| (a.host.clone(), a.scheme == "https")).collect();
    let now = probe_all(&targets, http_port, https_port).await;
    baseline
        .iter()
        .zip(now.iter())
        .map(|(before, after)| SiteCheck {
            host: before.host.clone(),
            scheme: before.scheme.clone(),
            verdict: compare(before, after),
        })
        .collect()
}
