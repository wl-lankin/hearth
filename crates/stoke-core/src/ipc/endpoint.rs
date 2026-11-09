//! Where the core listens: a named pipe on Windows, a Unix socket in `<run>` elsewhere.

use std::io;

use interprocess::local_socket::tokio::{prelude::*, Stream};
use interprocess::local_socket::Name;
use sha2::{Digest, Sha256};

use crate::StokePaths;

/// `stoke-<user>-<first 10 hex chars of sha256(root)>`: one endpoint per user and Stoke home, so
/// a test or dev build with its own `STOKE_HOME` never talks to the installed app.
pub fn endpoint_name(paths: &StokePaths) -> String {
    let root = std::fs::canonicalize(&paths.root).unwrap_or_else(|_| paths.root.clone());
    let digest = Sha256::digest(root.to_string_lossy().as_bytes());
    let hash: String = digest.iter().take(5).map(|b| format!("{b:02x}")).collect();
    let user: String = whoami::username().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("stoke-{}-{hash}", if user.is_empty() { "user" } else { &user })
}

#[cfg(windows)]
pub(crate) fn name(paths: &StokePaths) -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};
    endpoint_name(paths).to_ns_name::<GenericNamespaced>()
}

#[cfg(unix)]
pub(crate) fn socket_path(paths: &StokePaths) -> std::path::PathBuf {
    paths.run.join("stoke.sock")
}

#[cfg(unix)]
pub(crate) fn name(paths: &StokePaths) -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};
    socket_path(paths).to_fs_name::<GenericFilePath>()
}

pub(crate) async fn connect(paths: &StokePaths) -> io::Result<Stream> {
    Stream::connect(name(paths)?).await
}
