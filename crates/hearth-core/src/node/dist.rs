//! Node releases straight from nodejs.org, for versions the catalog does not carry. The hash comes
//! from the release's own SHASUMS256.txt (over HTTPS, as nvm and fnm do), not from the catalog.

use serde::Deserialize;

use crate::php::download::{client, get_bytes};
use crate::php::manifest::{compare_versions, Arch, Format, NodeBuild, Os};
use crate::CoreError;

use super::resolve::{matches, Spec};

pub const NODE_DIST_URL: &str = "https://nodejs.org/dist";

/// A release that has a build for the asked-for computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub lts: Option<String>,
}

#[derive(Deserialize)]
struct Entry {
    version: String,
    /// `false`, or the LTS codename.
    lts: serde_json::Value,
    files: Vec<String>,
}

/// nodejs.org's names for a target: in index.json's `files`, and in the archive's name.
fn target(os: Os, arch: Arch) -> (&'static str, &'static str, Format) {
    match (os, arch) {
        (Os::Macos, Arch::Arm64) => ("osx-arm64-tar", "darwin-arm64", Format::TarGz),
        (Os::Macos, _) => ("osx-x64-tar", "darwin-x64", Format::TarGz),
        (Os::Windows, _) => ("win-x64-zip", "win-x64", Format::Zip),
    }
}

/// The newest release in `index` (nodejs.org's index.json) that `spec` asks for and that has a
/// build for `os`/`arch`; `None` when there is none.
pub fn pick(index: &[u8], spec: &Spec, os: Os, arch: Arch) -> Result<Option<Release>, CoreError> {
    let entries: Vec<Entry> = serde_json::from_slice(index)
        .map_err(|e| CoreError::Php(format!("nodejs.org's list of releases could not be read: {e}")))?;
    let (key, _, _) = target(os, arch);
    let mut found: Vec<Release> = entries
        .into_iter()
        .filter(|e| e.files.iter().any(|f| f == key))
        .map(|e| Release {
            version: e.version.trim_start_matches('v').to_string(),
            lts: e.lts.as_str().map(String::from),
        })
        .filter(|r| matches(spec, &r.version, r.lts.as_deref()))
        .collect();
    found.sort_by(|a, b| compare_versions(&b.version, &a.version));
    Ok(found.into_iter().next())
}

/// The SHA-256 that `shasums` (a SHASUMS256.txt) lists for `file`.
pub fn hash_of(shasums: &str, file: &str) -> Option<String> {
    shasums.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == file && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// The nodejs.org build of the newest release `spec` asks for, for this computer; `None` when
/// nodejs.org has none. `dist` is nodejs.org's dist folder (tests use their own server).
pub async fn find(dist: &str, spec: &Spec) -> Result<Option<NodeBuild>, CoreError> {
    let (os, arch) = (Os::current(), Arch::current());
    let client = client()?;
    let index = get_bytes(&client, &format!("{dist}/index.json")).await?;
    let Some(release) = pick(&index, spec, os, arch)? else {
        return Ok(None);
    };
    let (_, suffix, format) = target(os, arch);
    let version = &release.version;
    let root = format!("node-v{version}-{suffix}");
    let file = format!("{root}.{}", if format == Format::Zip { "zip" } else { "tar.gz" });
    let shasums = get_bytes(&client, &format!("{dist}/v{version}/SHASUMS256.txt")).await?;
    let sha256 = hash_of(&String::from_utf8_lossy(&shasums), &file)
        .ok_or_else(|| CoreError::Php(format!("nodejs.org does not list a checksum for {file}")))?;
    Ok(Some(NodeBuild {
        major: version.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0),
        version: version.clone(),
        lts: release.lts,
        os,
        arch,
        url: format!("{dist}/v{version}/{file}"),
        sha256,
        format,
        root,
    }))
}
