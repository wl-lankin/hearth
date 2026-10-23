//! HTTP downloads: small files into memory, archives streamed to disk with their SHA-256 checked.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::CoreError;

const USER_AGENT: &str = concat!("Hearth/", env!("CARGO_PKG_VERSION"), " (+https://github.com/wl-lankin/hearth)");

pub(crate) fn client() -> Result<reqwest::Client, CoreError> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| CoreError::Php(format!("Could not start downloads: {e}")))
}

fn network(url: &str, e: impl std::fmt::Display) -> CoreError {
    CoreError::Php(format!("Could not download {url}: {e}"))
}

/// A small file (catalog, signature) into memory.
pub(crate) async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, CoreError> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| network(url, e))?;
    Ok(response.bytes().await.map_err(|e| network(url, e))?.to_vec())
}

/// Streams `url` to `dest`, hashing while it writes, and keeps it only when the SHA-256 matches.
/// `progress(received, total)` follows the download; `label` names it in errors ("PHP 8.4.27").
pub async fn download(
    url: &str,
    dest: &Path,
    sha256: &str,
    label: &str,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), CoreError> {
    let part = {
        let mut name = dest.as_os_str().to_owned();
        name.push(".part");
        PathBuf::from(name)
    };
    let result = async {
        let response =
            client()?.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| network(url, e))?;
        let total = response.content_length();
        let mut file = tokio::fs::File::create(&part).await.map_err(|e| CoreError::io(&part, e))?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| network(url, e))?;
            hasher.update(&chunk);
            file.write_all(&chunk).await.map_err(|e| CoreError::io(&part, e))?;
            received += chunk.len() as u64;
            progress(received, total);
        }
        file.flush().await.map_err(|e| CoreError::io(&part, e))?;
        drop(file);
        let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !actual.eq_ignore_ascii_case(sha256) {
            return Err(CoreError::Php(format!("The download of {label} is damaged; try again")));
        }
        std::fs::rename(&part, dest).map_err(|e| CoreError::io(dest, e))
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::php::manifest::tests::serve;

    fn hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    #[tokio::test]
    async fn a_matching_download_is_kept_and_reports_progress() {
        let body = vec![7u8; 300_000];
        let (base, _s) = serve(vec![("/php.zip", body.clone())]);
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("php.zip");
        let mut last = (0, None);
        download(&format!("{base}/php.zip"), &dest, &hex(&body), "PHP 8.4.27", |got, total| last = (got, total))
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(last, (300_000, Some(300_000)));
        assert!(!d.path().join("php.zip.part").exists());
    }

    #[tokio::test]
    async fn a_hash_mismatch_keeps_nothing() {
        let (base, _s) = serve(vec![("/php.zip", b"tampered".to_vec())]);
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("php.zip");
        let err =
            download(&format!("{base}/php.zip"), &dest, &"0".repeat(64), "PHP 8.4.27", |_, _| {}).await.unwrap_err();
        assert_eq!(err.to_string(), "The download of PHP 8.4.27 is damaged; try again");
        assert!(!dest.exists());
        assert!(!d.path().join("php.zip.part").exists());
    }

    #[tokio::test]
    async fn a_missing_file_is_a_download_error() {
        let (base, _s) = serve(vec![]);
        let d = tempfile::tempdir().unwrap();
        let err = download(&format!("{base}/nope.zip"), &d.path().join("x.zip"), &"0".repeat(64), "PHP", |_, _| {})
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("Could not download"), "{err}");
        assert!(!d.path().join("x.zip.part").exists());
    }
}
