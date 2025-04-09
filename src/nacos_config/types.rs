use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;

/// nacos中存储的配置
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NacosConfig {
  /// rpc地址列表
  pub rpcs: Vec<String>,
}

impl NacosConfig {
  /// 随机获取一个 rpc_client 信息
  pub fn get_rand_rpc(&self) -> RpcClient {
    let idx = rand::random::<u32>() as usize % self.rpcs.len();

    RpcClient::new(self.rpcs[idx].clone())
  }
}
