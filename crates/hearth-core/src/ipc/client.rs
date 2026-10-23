use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::time::timeout;

use super::lines::read_line;
use super::protocol::{Request, Response};
use super::{endpoint, peer};
use crate::{CoreError, HearthPaths};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Calls `method` on the core of this Hearth home. `NotRunning` when nothing listens.
pub async fn call(paths: &HearthPaths, method: &str, params: Value) -> Result<Value, CoreError> {
    let stream = match timeout(CONNECT_TIMEOUT, endpoint::connect(paths)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(_)) | Err(_) => return Err(CoreError::NotRunning),
    };
    if !peer::is_trusted(&stream).unwrap_or(false) {
        return Err(CoreError::ForeignEndpoint);
    }
    let (read, mut write) = tokio::io::split(stream);

    let mut line =
        serde_json::to_vec(&Request::new(1, method, params)).map_err(|e| CoreError::Protocol(e.to_string()))?;
    line.push(b'\n');

    let exchange = async {
        write.write_all(&line).await.map_err(|e| CoreError::Protocol(e.to_string()))?;
        write.flush().await.map_err(|e| CoreError::Protocol(e.to_string()))?;
        let mut reader = BufReader::new(read);
        match read_line(&mut reader).await {
            Ok(Some(reply)) => Ok(reply),
            Ok(None) => Err(CoreError::Protocol("the core closed the connection".into())),
            Err(e) => Err(CoreError::Protocol(e.to_string())),
        }
    };
    let reply = timeout(RESPONSE_TIMEOUT, exchange)
        .await
        .map_err(|_| CoreError::Protocol("the core did not answer in time".into()))??;

    let response: Response = serde_json::from_slice(&reply).map_err(|e| CoreError::Protocol(e.to_string()))?;
    match (response.result, response.error) {
        (_, Some(error)) => Err(CoreError::Rpc { code: error.code, message: error.message }),
        (Some(result), None) => Ok(result),
        (None, None) => Ok(Value::Null),
    }
}
