//! Caught mails on disk: `<data>/mail/<id>.eml` as received, and `<id>.json` with what the list
//! shows. Ids sort by arrival.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine;
use mail_parser::{Message, MessageParser, MessagePart, PartKind};
use serde::{Deserialize, Serialize};

use crate::{atomic, CoreError, StokePaths};

/// How many mails are kept; the oldest go first.
pub const KEEP: usize = 1000;
/// How much of a mail's source `raw` returns.
const RAW_LIMIT: usize = 1024 * 1024;

/// What the SMTP conversation said about a mail, apart from the mail itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The SMTP username: the app the mail came from (`MAIL_USERNAME`), if it logged in.
    pub site: Option<String>,
    pub from: String,
    pub to: Vec<String>,
}

/// A mail as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub site: Option<String>,
    pub from: String,
    /// The envelope recipients, so Bcc shows too.
    pub to: Vec<String>,
    pub subject: String,
    /// Unix seconds.
    pub received: u64,
    pub size: u64,
    pub attachments: u32,
    pub seen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub index: u32,
    pub name: String,
    pub content_type: String,
    pub size: u64,
}

/// An opened mail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mail {
    #[serde(flatten)]
    pub summary: Summary,
    pub cc: Vec<String>,
    /// The HTML body, with inline (`cid:`) images as data URLs; `None` for text-only mails.
    pub html: Option<String>,
    /// The text body (made from the HTML when the mail has none).
    pub text: Option<String>,
    /// The attachment files (`attachments` is their count, from the summary).
    pub files: Vec<Attachment>,
}

fn dir(paths: &StokePaths) -> PathBuf {
    paths.data.join("mail")
}

/// Ids come from the UI and the CLI: only `<13 digits>-<4 digits>` ever names a file.
fn checked(id: &str) -> Result<&str, CoreError> {
    let b = id.as_bytes();
    let ok = b.len() == 18 && b[13] == b'-' && b.iter().enumerate().all(|(i, c)| i == 13 || c.is_ascii_digit());
    if ok {
        Ok(id)
    } else {
        Err(CoreError::Mail(format!("{id:?} is not a mail id")))
    }
}

fn file(paths: &StokePaths, id: &str, ext: &str) -> Result<PathBuf, CoreError> {
    Ok(dir(paths).join(format!("{}.{ext}", checked(id)?)))
}

fn read(path: &Path) -> Result<Vec<u8>, CoreError> {
    std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => CoreError::Mail("This mail is gone".into()),
        _ => CoreError::io(path, e),
    })
}

/// The next id: milliseconds since 1970 and a counter, always later than the one before.
fn next_id() -> String {
    static LAST: Mutex<(u64, u32)> = Mutex::new((0, 0));
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    *last = if now > last.0 {
        (now, 0)
    } else if last.1 < 9999 {
        (last.0, last.1 + 1)
    } else {
        (last.0 + 1, 0)
    };
    format!("{:013}-{:04}", last.0, last.1)
}

fn mailbox_text(name: Option<&str>, address: Option<&str>) -> Option<String> {
    match (name.filter(|n| !n.trim().is_empty()), address.filter(|a| !a.is_empty())) {
        (Some(n), Some(a)) => Some(format!("{n} <{a}>")),
        (None, Some(a)) => Some(a.to_string()),
        (Some(n), None) => Some(n.to_string()),
        (None, None) => None,
    }
}

/// The parts the HTML shows inline through `cid:`; they are not listed as attachments.
fn cid_parts<'m>(message: &'m Message<'m>, html: Option<&str>) -> Vec<MessagePart<'m>> {
    let Some(html) = html else { return vec![] };
    message
        .parts()
        .filter(|p| p.content_id().is_some_and(|id| !id.is_empty() && html.contains(&format!("cid:{id}"))))
        .collect()
}

/// The real HTML of a mail: `None` when its HTML body only stands in for a text part.
fn html_of(message: &Message) -> Option<String> {
    let root = message.root();
    let parts: Vec<String> = root
        .html_body()
        .filter(|p| matches!(p.kind(), PartKind::Html))
        .filter_map(|p| p.text().map(|t| t.into_owned()))
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn attachments_of<'m>(message: &'m Message<'m>, html: Option<&str>) -> Vec<MessagePart<'m>> {
    let inline: Vec<_> = cid_parts(message, html).iter().map(|p| p.id()).collect();
    message.root().attachments().filter(|p| !inline.contains(&p.id())).collect()
}

fn content_type(part: &MessagePart) -> String {
    match part.content_type() {
        Some(ct) => match ct.subtype() {
            Some(sub) => format!("{}/{sub}", ct.ctype()).to_ascii_lowercase(),
            None => ct.ctype().to_ascii_lowercase(),
        },
        None => "application/octet-stream".into(),
    }
}

/// Keeps a mail; afterwards only the newest `KEEP` remain.
pub fn save(paths: &StokePaths, envelope: &Envelope, raw: &[u8]) -> Result<Summary, CoreError> {
    let folder = dir(paths);
    std::fs::create_dir_all(&folder).map_err(|e| CoreError::io(&folder, e))?;
    let id = next_id();
    let parsed = MessageParser::new().parse(raw);
    let (subject, from, attachments) = match &parsed {
        Some(m) => {
            let from = m.from().and_then(|l| l.first()).and_then(|b| mailbox_text(b.name(), b.address()));
            let html = html_of(m);
            (m.subject().map(str::trim).map(String::from), from, attachments_of(m, html.as_deref()).len())
        }
        None => (None, None, 0),
    };
    let summary = Summary {
        id: id.clone(),
        site: envelope.site.clone(),
        from: from.unwrap_or_else(|| envelope.from.clone()),
        to: envelope.to.clone(),
        subject: subject.filter(|s| !s.is_empty()).unwrap_or_else(|| "(no subject)".into()),
        received: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        size: raw.len() as u64,
        attachments: attachments as u32,
        seen: false,
    };
    atomic::write(&folder.join(format!("{id}.eml")), raw)?;
    atomic::write(&folder.join(format!("{id}.json")), &serde_json::to_vec(&summary).unwrap_or_default())?;
    prune(paths);
    Ok(summary)
}

/// Removes the oldest mails over `KEEP`.
fn prune(paths: &StokePaths) {
    let mut ids = ids(paths);
    if ids.len() <= KEEP {
        return;
    }
    ids.sort();
    for id in &ids[..ids.len() - KEEP] {
        remove(paths, id);
    }
}

/// Every stored id, unsorted.
fn ids(paths: &StokePaths) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir(paths)) else { return vec![] };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".eml")).map(String::from))
        .filter(|id| checked(id).is_ok())
        .collect()
}

fn remove(paths: &StokePaths, id: &str) {
    for ext in ["eml", "json"] {
        let _ = std::fs::remove_file(dir(paths).join(format!("{id}.{ext}")));
    }
}

/// The kept mails, newest first. A summary that cannot be read is left out.
pub fn list(paths: &StokePaths) -> Vec<Summary> {
    let mut all: Vec<Summary> = ids(paths)
        .into_iter()
        .filter_map(|id| std::fs::read(dir(paths).join(format!("{id}.json"))).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect();
    all.sort_by(|a, b| b.id.cmp(&a.id));
    all
}

fn summary(paths: &StokePaths, id: &str) -> Result<Summary, CoreError> {
    let path = file(paths, id, "json")?;
    serde_json::from_slice(&read(&path)?).map_err(|e| CoreError::Mail(format!("{}: {e}", path.display())))
}

/// Opens a mail and marks it read.
pub fn get(paths: &StokePaths, id: &str) -> Result<Mail, CoreError> {
    let mut summary = summary(paths, id)?;
    let raw = read(&file(paths, id, "eml")?)?;
    if !summary.seen {
        summary.seen = true;
        atomic::write(&file(paths, id, "json")?, &serde_json::to_vec(&summary).unwrap_or_default())?;
    }
    let Some(message) = MessageParser::new().parse(&raw) else {
        let text = (!raw.is_empty()).then(|| String::from_utf8_lossy(&raw).into_owned());
        return Ok(Mail { summary, cc: vec![], html: None, text, files: vec![] });
    };
    let cc = message
        .cc()
        .map(|l| l.mailboxes().filter_map(|b| mailbox_text(b.name(), b.address())).collect())
        .unwrap_or_default();
    let mut html = html_of(&message);
    if let Some(h) = html.as_mut() {
        for part in cid_parts(&message, Some(h.as_str())) {
            let id = part.content_id().unwrap_or_default();
            let data = base64::engine::general_purpose::STANDARD.encode(part.decoded());
            *h = h.replace(&format!("cid:{id}"), &format!("data:{};base64,{data}", content_type(&part)));
        }
    }
    let text = message.body_text(0).map(|t| t.into_owned()).filter(|t| !t.trim().is_empty());
    let files = attachments_of(&message, html_of(&message).as_deref())
        .iter()
        .enumerate()
        .map(|(i, p)| Attachment {
            index: i as u32,
            name: attachment_name(p, i),
            content_type: content_type(p),
            size: p.decoded_len() as u64,
        })
        .collect();
    Ok(Mail { summary, cc, html, text, files })
}

fn attachment_name(part: &MessagePart, index: usize) -> String {
    match part.attachment_name().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => name.to_string(),
        None if matches!(part.kind(), PartKind::Message(_)) => format!("attachment-{}.eml", index + 1),
        None => format!("attachment-{}", index + 1),
    }
}

/// A mail's source as text: the first megabyte, and whether there is more.
pub fn raw(paths: &StokePaths, id: &str) -> Result<(String, bool), CoreError> {
    let raw = read(&file(paths, id, "eml")?)?;
    let cut = raw.len() > RAW_LIMIT;
    Ok((String::from_utf8_lossy(&raw[..raw.len().min(RAW_LIMIT)]).into_owned(), cut))
}

/// The name and decoded bytes of attachment `index`, numbered as in `Mail::files`.
pub fn attachment(paths: &StokePaths, id: &str, index: u32) -> Result<(String, Vec<u8>), CoreError> {
    let raw = read(&file(paths, id, "eml")?)?;
    let message = MessageParser::new().parse(&raw);
    let found = message.as_ref().and_then(|m| {
        let parts = attachments_of(m, html_of(m).as_deref());
        parts.get(index as usize).map(|p| (attachment_name(p, index as usize), p.decoded().into_owned()))
    });
    found.ok_or_else(|| CoreError::Mail(format!("This mail has no attachment {}", index + 1)))
}

pub fn delete(paths: &StokePaths, id: &str) -> Result<(), CoreError> {
    let eml = file(paths, id, "eml")?;
    if !eml.exists() {
        return Err(CoreError::Mail("This mail is gone".into()));
    }
    remove(paths, id);
    Ok(())
}

/// Deletes all mails (`None`), those without a username (`Some(None)`) or one app's (`Some(Some(app))`).
pub fn clear(paths: &StokePaths, site: Option<Option<&str>>) -> Result<(), CoreError> {
    match site {
        None => {
            let folder = dir(paths);
            let Ok(entries) = std::fs::read_dir(&folder) else { return Ok(()) };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    std::fs::remove_file(&path).map_err(|e| CoreError::io(&path, e))?;
                }
            }
        }
        Some(wanted) => {
            for s in list(paths).into_iter().filter(|s| s.site.as_deref() == wanted) {
                remove(paths, &s.id);
            }
        }
    }
    Ok(())
}
