use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use interprocess::local_socket::tokio::{prelude::*, Stream};
use interprocess::local_socket::ListenerOptions;
use serde_json::Value;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::lines::read_line;
use super::protocol::{Request, Response, RpcError, PARSE_ERROR};
use super::{client, endpoint};
use crate::{CoreError, HearthPaths};

pub type HandlerFuture = Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send>>;

/// Answers one request: method name and params in, result or RPC error out.
pub type Handler = Arc<dyn Fn(String, Value) -> HandlerFuture + Send + Sync>;

pub struct ServerHandle {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl ServerHandle {
    /// Stops accepting connections and releases the endpoint (the socket file on Unix).
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

/// Starts the core server for this Hearth home. `AlreadyRunning` when a live core answers there.
pub async fn serve(paths: &HearthPaths, handler: Handler) -> Result<ServerHandle, CoreError> {
    match client::call(paths, "core.ping", Value::Null).await {
        Ok(_) => return Err(CoreError::AlreadyRunning),
        Err(CoreError::ForeignEndpoint) => return Err(CoreError::ForeignEndpoint),
        Err(_) => {}
    }
    let name = endpoint::name(paths).map_err(|e| CoreError::io(&paths.run, e))?;
    // A socket file nobody answers on is left over from a crash: take its place.
    let options = ListenerOptions::new().name(name).try_overwrite(true);
    let options = restrict_to_current_user(options, paths)?;
    // On Windows the first pipe instance is created exclusively, so a pre-created pipe of the same
    // name makes this fail instead of sharing it.
    let listener = options.create_tokio().map_err(|e| CoreError::io(&paths.run, e))?;

    let (stop, mut stopped) = watch::channel(false);
    let task = tokio::spawn(async move {
        let mut connections = Vec::new();
        loop {
            tokio::select! {
                _ = stopped.changed() => break,
                accepted = listener.accept() => match accepted {
                    Ok(stream) => connections.push(tokio::spawn(handle_connection(stream, handler.clone()))),
                    Err(_) => continue,
                },
            }
            connections.retain(|c: &JoinHandle<()>| !c.is_finished());
        }
        for connection in connections {
            connection.abort();
        }
        // Dropping the listener releases the name (reclaim_name deletes the socket file on Unix).
        drop(listener);
    });
    Ok(ServerHandle { stop, task })
}

#[cfg(windows)]
fn restrict_to_current_user<'n>(
    options: ListenerOptions<'n>,
    paths: &HearthPaths,
) -> Result<ListenerOptions<'n>, CoreError> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;

    let denied = |what: &str| CoreError::io(&paths.run, std::io::Error::other(what.to_string()));
    let sddl = super::peer::pipe_sddl().ok_or_else(|| denied("the current user could not be determined"))?;
    let sddl = widestring::U16CString::from_str(sddl).map_err(|_| denied("invalid security descriptor"))?;
    let descriptor = SecurityDescriptor::deserialize(&sddl).map_err(|e| CoreError::io(&paths.run, e))?;
    Ok(options.security_descriptor(descriptor))
}

#[cfg(unix)]
fn restrict_to_current_user<'n>(
    options: ListenerOptions<'n>,
    paths: &HearthPaths,
) -> Result<ListenerOptions<'n>, CoreError> {
    use interprocess::os::unix::local_socket::ListenerOptionsExt;
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(&paths.run, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| CoreError::io(&paths.run, e))?;
    Ok(options.mode(0o600))
}

async fn handle_connection(stream: Stream, handler: Handler) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    loop {
        let line = match read_line(&mut reader).await {
            Ok(Some(line)) => line,
            Ok(None) => return,
            Err(_) => {
                let _ = send(&mut write, &Response::err(None, RpcError::new(PARSE_ERROR, "line too long"))).await;
                return;
            }
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let response = match serde_json::from_slice::<Request>(&line) {
            Ok(request) => match handler(request.method, request.params).await {
                Ok(result) => Response::ok(request.id, result),
                Err(error) => Response::err(Some(request.id), error),
            },
            Err(e) => Response::err(None, RpcError::new(PARSE_ERROR, e.to_string())),
        };
        if send(&mut write, &response).await.is_err() {
            return;
        }
    }
}

async fn send<W: AsyncWriteExt + Unpin>(write: &mut W, response: &Response) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(response).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    write.write_all(&bytes).await?;
    write.flush().await
}
