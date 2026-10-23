//! HTTP downloads: small files into memory, archives streamed to disk with their SHA-256 checked.

use std::time::Duration;

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
