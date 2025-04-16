use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;

/// nacos中存储的配置
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NacosConfig {
  /// rpc地址列表
  pub rpcs: Vec<String>,

  /// cu 模拟计算后的增加的系数， 基点， 1代表 1/10000
  pub cu_factor_basis: u32,
}

impl NacosConfig {
  /// 随机获取一个 rpc_client 信息
  pub fn get_rand_rpc(&self) -> RpcClient {
    let idx = rand::random::<u32>() as usize % self.rpcs.len();

    RpcClient::new(self.rpcs[idx].clone())
  }

  /// 随机获取一个 rpc 地址
  pub fn get_rand_rpc_str(&self) -> String {
    let idx = rand::random::<u32>() as usize % self.rpcs.len();

    self.rpcs[idx].clone()
  }

  /// 获取cu_factor,
  /// 已经 +上了1.0的cu_factor值
  pub fn get_cu_factor(&self) -> f64 {
    self.cu_factor_basis as f64 / 10000.0 + 1.0
  }
}
