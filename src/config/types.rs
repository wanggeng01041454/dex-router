use rand::{Rng};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
  /// Solana RPC 地址列表
  pub rpcs: Vec<String>,

  /// CU 增益因子，基点制（1 表示 1/10000）
  pub cu_factor_basis: u32,
}

impl AppConfig {
  /// 随机返回一个 Solana RPC 客户端
  pub fn get_rand_rpc(&self) -> RpcClient {
    let len = self.rpcs.len();
    assert!(len > 0, "RPC 列表为空");
    let idx = rand::rng().random_range(0..len);
    RpcClient::new(self.rpcs[idx].clone())
  }
}
