use serde::{Deserialize, Serialize};

/// nacos中存储的配置
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NacosConfig {
  /// rpc地址列表
  pub rpcs: Vec<String>,
}
