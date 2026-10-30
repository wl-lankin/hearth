//! The PHP catalog published by the hearth-php repo: manifest.json plus its minisign signature.
//! Nothing from an unverified catalog is ever used.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::{atomic, CoreError, HearthPaths};

/// The catalog's public key (Tauri's base64-wrapped minisign format).
pub const CATALOG_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDNDNkZDRkY5M0ExMkM3NjIKUldSaXh4STYrYzl2UElncy9hMzZUczhTNmdPQldKNUlIYWZsL3JnamRianhXb2ZYM0VQaXBpc24K";

pub const CATALOG_URL: &str = "https://github.com/wl-lankin/hearth-php/releases/download/manifest/manifest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Macos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arch {
    #[serde(rename = "x64")]
    X64,
    #[serde(rename = "arm64")]
    Arm64,
    #[serde(rename = "x86_64")]
    X86_64,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Windows
        }
    }
}

impl Arch {
    /// The catalog's name for this machine: Windows builds are "x64", macOS ones "arm64" / "x86_64".
    pub fn current() -> Arch {
        match (Os::current(), cfg!(target_arch = "aarch64")) {
            (Os::Macos, true) => Arch::Arm64,
            (Os::Macos, false) => Arch::X86_64,
            (Os::Windows, _) => Arch::X64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExtensionSource {
    /// Compiled into PHP.
    Builtin,
    /// Windows: a zip at `url` holding `file`, loaded with `extension=`.
    Dll { version: String, url: String, sha256: String, file: String },
    /// Windows: like `Dll`, loaded with `zend_extension=`.
    ZendDll { version: String, url: String, sha256: String, file: String },
    /// macOS: `file` inside the PHP archive, loaded with `zend_extension=`.
    ZendSo { file: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhpBuild {
    pub minor: String,
    pub version: String,
    pub channel: String,
    pub os: Os,
    pub arch: Arch,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
    pub format: Format,
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub extensions: BTreeMap<String, ExtensionSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// A program Hearth runs (Caddy), one entry per OS and architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub os: Os,
    pub arch: Arch,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub format: Format,
    /// The program's path inside the archive.
    pub binary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tools {
    #[serde(default)]
    pub caddy: Vec<Tool>,
}

/// How a service build is packed: an archive, or a single program file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceFormat {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "binary")]
    Binary,
}

/// One build of a service (database, Redis, search, S3) for one OS and architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceBuild {
    /// mysql, mariadb, postgresql, redis, meilisearch, typesense, silo (kept as text: a newer
    /// catalog may list kinds this Hearth does not know yet).
    pub kind: String,
    /// The version line a user picks ("8.4", "18"), newest patch inside it.
    pub line: String,
    pub version: String,
    pub os: Os,
    pub arch: Arch,
    pub url: String,
    pub sha256: String,
    pub format: ServiceFormat,
    /// The archive's top folder ('' when its files sit at the top).
    pub root: String,
}

/// One Node.js release (the newest of its major) for one OS and architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeBuild {
    pub major: u32,
    pub version: String,
    /// The LTS codename ("Jod"), none for a current release.
    pub lts: Option<String>,
    pub os: Os,
    pub arch: Arch,
    pub url: String,
    pub sha256: String,
    pub format: Format,
    /// The archive's top folder.
    pub root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub generated: String,
    pub php: Vec<PhpBuild>,
    pub composer: Artifact,
    pub cacert: Artifact,
    /// Added in M2; older catalogs have none.
    #[serde(default)]
    pub tools: Tools,
    /// Added in M5; older catalogs have none.
    #[serde(default)]
    pub services: Vec<ServiceBuild>,
    /// Added in M6; older catalogs have none.
    #[serde(default)]
    pub node: Vec<NodeBuild>,
}

fn invalid(message: impl std::fmt::Display) -> CoreError {
    CoreError::Php(format!("The PHP catalog is invalid: {message}"))
}

/// Parses a catalog whose signature was already checked.
pub(crate) fn parse(json: &[u8]) -> Result<Manifest, CoreError> {
    #[derive(Deserialize)]
    struct Head {
        schema: u32,
    }
    let head: Head = serde_json::from_slice(json).map_err(invalid)?;
    if head.schema != 1 {
        return Err(CoreError::Php("This Hearth is too old for the PHP catalog; update Hearth".into()));
    }
    serde_json::from_slice(json).map_err(invalid)
}

/// Decodes Tauri's wrapper: the key and signature files are base64 of the minisign text.
fn unwrap_base64(text: &str) -> Result<String, CoreError> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(text.trim()).map_err(invalid)?;
    String::from_utf8(bytes).map_err(invalid)
}

/// Verifies the minisign signature of `json`, then parses it.
pub fn parse_signed(json: &[u8], sig: &[u8], public_key: &str) -> Result<Manifest, CoreError> {
    let bad_signature = || CoreError::Php("The PHP catalog's signature is not valid".into());
    let key = minisign_verify::PublicKey::decode(&unwrap_base64(public_key)?).map_err(|_| bad_signature())?;
    let sig_text = unwrap_base64(std::str::from_utf8(sig).map_err(|_| bad_signature())?)?;
    let signature = minisign_verify::Signature::decode(&sig_text).map_err(|_| bad_signature())?;
    key.verify(json, &signature, false).map_err(|_| bad_signature())?;
    parse(json)
}

/// Compares PHP versions numerically; a release sorts after its pre-releases (8.6.0 > 8.6.0RC4).
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    fn split(v: &str) -> (Vec<u64>, Option<&str>) {
        let end = v.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(v.len());
        let numbers = v[..end].split('.').filter_map(|n| n.parse().ok()).collect();
        (numbers, (end < v.len()).then(|| &v[end..]))
    }
    let (na, pa) = split(a);
    let (nb, pb) = split(b);
    na.cmp(&nb).then_with(|| match (pa, pb) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.cmp(y),
    })
}

/// The builds for one OS and architecture, newest first.
pub fn builds_for(m: &Manifest, os: Os, arch: Arch) -> Vec<&PhpBuild> {
    let mut builds: Vec<&PhpBuild> = m.php.iter().filter(|b| b.os == os && b.arch == arch).collect();
    builds.sort_by(|a, b| compare_versions(&b.version, &a.version));
    builds
}

fn cache_paths(paths: &HearthPaths) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = paths.data.join("php");
    (dir.join("manifest.json"), dir.join("manifest.json.sig"))
}

/// Downloads and verifies the catalog and caches it. When the download fails, does not verify (a
/// release caught mid-update) or is older than the newest catalog seen (rollback protection), the
/// cached copy is used, verified again; without one that error is returned.
pub async fn fetch(paths: &HearthPaths, url: &str, public_key: &str) -> Result<Manifest, CoreError> {
    let (json_path, sig_path) = cache_paths(paths);
    let fresh = download_and_accept(&json_path, &sig_path, url, public_key).await;
    match fresh {
        Ok(manifest) => Ok(manifest),
        Err(error) => {
            let (Ok(json), Ok(sig)) = (std::fs::read(&json_path), std::fs::read(&sig_path)) else {
                return Err(error);
            };
            parse_signed(&json, &sig, public_key)
        }
    }
}

async fn download_and_accept(
    json_path: &std::path::Path,
    sig_path: &std::path::Path,
    url: &str,
    public_key: &str,
) -> Result<Manifest, CoreError> {
    let client = crate::php::download::client()?;
    let json = crate::php::download::get_bytes(&client, url).await?;
    let sig = crate::php::download::get_bytes(&client, &format!("{url}.sig")).await?;
    let manifest = parse_signed(&json, &sig, public_key)?;
    // Rollback protection: never accept a signed catalog older than the newest one seen, so an old
    // catalog (pointing at old PHP builds) cannot be replayed. `generated` is UTC RFC 3339, which
    // compares correctly as text.
    let newest = json_path.with_file_name("newest-generated");
    let seen = std::fs::read_to_string(&newest).unwrap_or_default();
    if manifest.generated.as_str() < seen.trim() {
        return Err(CoreError::Php(
            "The downloaded PHP catalog is older than the one Hearth already has; try again later".into(),
        ));
    }
    std::fs::create_dir_all(json_path.parent().unwrap()).map_err(|e| CoreError::io(json_path, e))?;
    atomic::write(json_path, &json)?;
    atomic::write(sig_path, &sig)?;
    atomic::write(&newest, manifest.generated.as_bytes())?;
    Ok(manifest)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/manifest.json");
    const FIXTURE_SIG: &[u8] = include_bytes!("../../tests/fixtures/manifest.json.sig");
    const TEST_KEY: &str = include_str!("../../tests/fixtures/test-catalog.pub");

    #[test]
    fn a_good_signature_parses() {
        let m = parse_signed(FIXTURE, FIXTURE_SIG, TEST_KEY.trim()).unwrap();
        assert_eq!(m.schema, 1);
        assert!(m.php.iter().any(|b| b.minor == "8.4" && b.os == Os::Windows));
        assert!(m.composer.url.starts_with("https://getcomposer.org/"));
    }

    #[test]
    fn a_changed_byte_fails_the_signature() {
        let mut tampered = FIXTURE.to_vec();
        let i = tampered.iter().position(|&b| b == b'8').unwrap();
        tampered[i] = b'9';
        let err = parse_signed(&tampered, FIXTURE_SIG, TEST_KEY.trim()).unwrap_err();
        assert!(err.to_string().contains("signature"), "{err}");
    }

    #[test]
    fn the_real_catalog_key_does_not_accept_test_signatures() {
        // The compiled-in key must be a valid key, so the rejection below is a real verification failure.
        assert!(minisign_verify::PublicKey::decode(&unwrap_base64(CATALOG_PUBLIC_KEY).unwrap()).is_ok());
        assert!(parse_signed(FIXTURE, FIXTURE_SIG, CATALOG_PUBLIC_KEY).is_err());
    }

    #[test]
    fn an_unknown_schema_asks_for_a_hearth_update() {
        let err = parse(br#"{"schema":2,"php":[]}"#).unwrap_err();
        assert!(err.to_string().contains("update Hearth"), "{err}");
    }

    #[test]
    fn builds_for_filters_by_os_and_arch_newest_first() {
        let m = parse_signed(FIXTURE, FIXTURE_SIG, TEST_KEY.trim()).unwrap();
        let minors: Vec<&str> = builds_for(&m, Os::Windows, Arch::X64).iter().map(|b| b.minor.as_str()).collect();
        assert_eq!(minors, ["8.6", "8.5", "8.4", "8.3", "8.2", "8.1"]);
        assert!(builds_for(&m, Os::Macos, Arch::Arm64).is_empty());
    }

    /// Serves `files` (path -> body) on 127.0.0.1 until the returned server is dropped; 404 otherwise.
    pub(crate) fn serve(files: Vec<(&'static str, Vec<u8>)>) -> (String, std::sync::Arc<tiny_http::Server>) {
        let server = std::sync::Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let base = format!("http://{}", server.server_addr().to_ip().unwrap());
        let s = server.clone();
        std::thread::spawn(move || {
            for request in s.incoming_requests() {
                let body = files.iter().find(|(p, _)| *p == request.url()).map(|(_, b)| b.clone());
                let _ = match body {
                    Some(b) => request.respond(tiny_http::Response::from_data(b).with_chunked_threshold(usize::MAX)),
                    None => request.respond(tiny_http::Response::empty(404)),
                };
            }
        });
        (base, server)
    }

    #[tokio::test]
    async fn fetch_caches_and_falls_back_offline() {
        let dir = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(dir.path());
        paths.ensure().unwrap();
        let (base, server) =
            serve(vec![("/manifest.json", FIXTURE.to_vec()), ("/manifest.json.sig", FIXTURE_SIG.to_vec())]);
        let url = format!("{base}/manifest.json");
        let online = fetch(&paths, &url, TEST_KEY.trim()).await.unwrap();
        server.unblock();
        drop(server);
        let offline = fetch(&paths, "http://127.0.0.1:9/manifest.json", TEST_KEY.trim()).await.unwrap();
        assert_eq!(online, offline);
    }

    #[tokio::test]
    async fn an_older_catalog_never_replaces_a_newer_cached_one() {
        // Rollback protection: a validly signed but older catalog (e.g. replayed by an attacker) is ignored.
        let dir = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(dir.path());
        paths.ensure().unwrap();
        let (base, _server) =
            serve(vec![("/manifest.json", FIXTURE.to_vec()), ("/manifest.json.sig", FIXTURE_SIG.to_vec())]);
        let url = format!("{base}/manifest.json");
        let served = fetch(&paths, &url, TEST_KEY.trim()).await.unwrap();
        // Pretend the cache holds a newer catalog: same signed bytes, but remembered as newer.
        std::fs::write(paths.data.join("php").join("newest-generated"), "2999-01-01T00:00:00Z").unwrap();
        // The older download is refused; the verified cached copy is used instead.
        let used = fetch(&paths, &url, TEST_KEY.trim()).await.unwrap();
        assert_eq!(used, served);
        let mark = std::fs::read_to_string(paths.data.join("php").join("newest-generated")).unwrap();
        assert_eq!(mark, "2999-01-01T00:00:00Z", "the high-water mark never goes down");
    }

    #[tokio::test]
    async fn a_bad_download_falls_back_to_the_verified_cache() {
        // A catalog caught mid-update (new json, old signature) must not block installs.
        let dir = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(dir.path());
        paths.ensure().unwrap();
        let (good, _s1) =
            serve(vec![("/manifest.json", FIXTURE.to_vec()), ("/manifest.json.sig", FIXTURE_SIG.to_vec())]);
        let cached = fetch(&paths, &format!("{good}/manifest.json"), TEST_KEY.trim()).await.unwrap();
        let mut tampered = FIXTURE.to_vec();
        tampered.push(b' ');
        let (bad, _s2) = serve(vec![("/manifest.json", tampered), ("/manifest.json.sig", FIXTURE_SIG.to_vec())]);
        assert_eq!(fetch(&paths, &format!("{bad}/manifest.json"), TEST_KEY.trim()).await.unwrap(), cached);
    }

    #[tokio::test]
    async fn fetch_without_cache_reports_the_download_error() {
        let dir = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(dir.path());
        paths.ensure().unwrap();
        let err = fetch(&paths, "http://127.0.0.1:9/manifest.json", TEST_KEY.trim()).await.unwrap_err();
        assert!(err.to_string().starts_with("Could not download"), "{err}");
    }

    #[tokio::test]
    async fn a_tampered_download_is_never_cached() {
        let dir = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(dir.path());
        paths.ensure().unwrap();
        let mut tampered = FIXTURE.to_vec();
        tampered.push(b' ');
        let (base, _server) = serve(vec![("/manifest.json", tampered), ("/manifest.json.sig", FIXTURE_SIG.to_vec())]);
        assert!(fetch(&paths, &format!("{base}/manifest.json"), TEST_KEY.trim()).await.is_err());
        assert!(!paths.data.join("php").join("manifest.json").exists());
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(compare_versions("8.10.0", "8.9.9").is_gt());
        assert!(compare_versions("8.6.0", "8.6.0RC4").is_gt());
        assert!(compare_versions("8.4.27", "8.4.27").is_eq());
    }
}
