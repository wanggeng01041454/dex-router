use serde::{Deserialize, Serialize};

// JSON-RPC 相关结构
#[derive(Deserialize)]
pub struct JsonRpcRequest {
  pub jsonrpc: String,
  pub method: String,
  pub params: serde_json::Value,
  pub id: serde_json::Value,
}

#[derive(Serialize)]
pub struct JsonRpcResponse {
  pub jsonrpc: String,
  pub result: serde_json::Value,
  pub id: serde_json::Value,
}
