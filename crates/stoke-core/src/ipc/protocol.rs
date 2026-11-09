//! JSON-RPC 2.0, one JSON object per line.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PARSE_ERROR: i64 = -32700;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// The longest line either side accepts, so a broken peer cannot make the other buffer forever.
pub const MAX_LINE: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError { code, message: message.into() }
    }
}

impl Request {
    pub fn new(id: u64, method: &str, params: Value) -> Self {
        Request { jsonrpc: "2.0".into(), id, method: method.into(), params }
    }
}

impl Response {
    pub fn ok(id: u64, result: Value) -> Self {
        Response { jsonrpc: "2.0".into(), id: Some(id), result: Some(result), error: None }
    }

    pub fn err(id: Option<u64>, error: RpcError) -> Self {
        Response { jsonrpc: "2.0".into(), id, result: None, error: Some(error) }
    }
}
