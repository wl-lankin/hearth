//! Which Node a folder uses: the nearest `.nvmrc` or `.node-version` up from it, else the default.
//! A version that is not installed is an error naming the file; another version is never used
//! in its place.

use std::path::{Path, PathBuf};

use crate::CoreError;

use super::store::Installed;

/// What a version file asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    /// "22", "v22.12", "22.12.0": the leading parts that must match.
    Version(Vec<u64>),
    /// "lts/*" (any LTS) or "lts/jod" (that codename, lower case).
    Lts(Option<String>),
    /// "node", "latest", "current", "stable": the newest.
    Latest,
    /// "system": the default version.
    Default,
}

impl std::fmt::Display for Spec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Spec::Version(parts) => f.write_str(&parts.iter().map(u64::to_string).collect::<Vec<_>>().join(".")),
            Spec::Lts(None) => f.write_str("lts/*"),
            Spec::Lts(Some(name)) => write!(f, "lts/{name}"),
            Spec::Latest => f.write_str("latest"),
            Spec::Default => f.write_str("system"),
        }
    }
}

/// Reads a version file's text (or a version typed on the command line): the first line that is
/// not empty once a `#` comment, a BOM and whitespace are taken off.
pub fn parse_spec(text: &str) -> Result<Spec, String> {
    let word = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .find(|l| !l.is_empty())
        .ok_or_else(|| "names no Node version".to_string())?;
    let lower = word.to_ascii_lowercase();
    let unknown = || format!("asks for \"{word}\", which Hearth does not understand");
    match lower.as_str() {
        "node" | "latest" | "current" | "stable" => return Ok(Spec::Latest),
        "system" => return Ok(Spec::Default),
        "lts/*" => return Ok(Spec::Lts(None)),
        _ => {}
    }
    if let Some(name) = lower.strip_prefix("lts/") {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(unknown());
        }
        return Ok(Spec::Lts(Some(name.to_string())));
    }
    let digits = lower.strip_prefix('v').unwrap_or(&lower);
    let parts: Vec<u64> =
        digits.split('.').map(|p| p.parse::<u64>()).collect::<Result<_, _>>().map_err(|_| unknown())?;
    if parts.len() > 3 {
        return Err(unknown());
    }
    Ok(Spec::Version(parts))
}

/// A version file and what it asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub spec: Spec,
    pub file: PathBuf,
}

/// A version file's text: UTF-8, or UTF-16 with a BOM (what `echo 20 > .nvmrc` writes in Windows
/// PowerShell 5.1).
fn decode(bytes: &[u8]) -> Option<String> {
    let utf16 = |rest: &[u8], unit: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| unit([c[0], c[1]])).collect();
        String::from_utf16(&units).ok()
    };
    match bytes {
        [0xff, 0xfe, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xfe, 0xff, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ => String::from_utf8(bytes.to_vec()).ok(),
    }
}

/// The nearest `.nvmrc` (first) or `.node-version` in `cwd` or a folder above it; `None` when there
/// is none up to the root.
pub fn find_spec(cwd: &Path) -> Option<Result<Found, CoreError>> {
    for dir in cwd.ancestors() {
        for name in [".nvmrc", ".node-version"] {
            let file = dir.join(name);
            let bytes = match std::fs::read(&file) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Some(Err(CoreError::io(&file, e))),
            };
            let Some(text) = decode(&bytes) else {
                return Some(Err(CoreError::Php(format!("{} is not a text file Hearth can read", file.display()))));
            };
            return Some(
                parse_spec(&text)
                    .map(|spec| Found { spec, file: file.clone() })
                    .map_err(|why| CoreError::Php(format!("{} {why}", file.display()))),
            );
        }
    }
    None
}

/// Whether `version` (with its LTS codename) is what `spec` asks for. `Default` matches nothing:
/// it is answered by the default, not by a version.
pub fn matches(spec: &Spec, version: &str, lts: Option<&str>) -> bool {
    match spec {
        Spec::Version(parts) => {
            let have: Vec<u64> = version.split('.').filter_map(|p| p.parse().ok()).collect();
            have.len() >= parts.len() && have[..parts.len()] == parts[..]
        }
        Spec::Lts(None) => lts.is_some(),
        Spec::Lts(Some(name)) => lts.is_some_and(|l| l.eq_ignore_ascii_case(name)),
        Spec::Latest => true,
        Spec::Default => false,
    }
}

/// Why a version was picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    File(PathBuf),
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub version: String,
    pub dir: PathBuf,
    pub why: Why,
}

/// The installed version for `wanted` (a spec and the file it came from), or the default without
/// one. `installed` is newest first.
pub fn resolve(
    installed: &[Installed],
    default: Option<&str>,
    wanted: Option<(&Spec, &Path)>,
) -> Result<Choice, CoreError> {
    let choice = |i: &Installed, why: Why| Choice { version: i.version.clone(), dir: i.dir.clone(), why };
    match wanted {
        Some((spec, file)) if *spec != Spec::Default => installed
            .iter()
            .find(|i| matches(spec, &i.version, i.lts.as_deref()))
            .map(|i| choice(i, Why::File(file.to_path_buf())))
            .ok_or_else(|| {
                CoreError::Php(format!(
                    "This project wants Node {spec} ({}), which is not installed. Install it with: hearth node:install {spec}",
                    file.display()
                ))
            }),
        _ => installed
            .iter()
            .find(|i| Some(i.version.as_str()) == default)
            .or_else(|| installed.first())
            .map(|i| choice(i, Why::Default))
            .ok_or_else(|| CoreError::Php("No Node version is installed in Hearth".into())),
    }
}
