use solana_client::rpc_client::RpcClient;
use crate::config::types::AppConfig;  // 假设 AppConfig 已经在模块中定义
use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

    #[test]
    fn test_get_rand_rpc() {
        // 模拟的 Solana RPC 地址
        let rpcs = vec![
            "https://api.mainnet-beta.solana.com".to_string(),
            "https://api.testnet.solana.com".to_string(),
            "https://api.devnet.solana.com".to_string(),
        ];

        // 创建 AppConfig 实例
        let config = AppConfig {
            rpcs,
            cu_factor_basis: 10000,
        };

        // 通过 get_rand_rpc 随机获取 RPC 客户端
        let rpc_client = config.get_rand_rpc();

        // 检查返回的 RPC 地址是否匹配
        let valid_rpc_addresses = vec![
            "https://api.mainnet-beta.solana.com",
            "https://api.testnet.solana.com",
            "https://api.devnet.solana.com",
        ];

        assert!(valid_rpc_addresses.contains(&rpc_client.url().as_str()));
    }

    #[test]
    fn test_get_rand_rpc_multiple_calls() {
        // 模拟的 Solana RPC 地址
        let rpcs = vec![
            "https://api.mainnet-beta.solana.com".to_string(),
            "https://api.testnet.solana.com".to_string(),
            "https://api.devnet.solana.com".to_string(),
        ];

        let config = AppConfig {
            rpcs,
            cu_factor_basis: 10000,
        };

        // 模拟多次调用 get_rand_rpc，并确保返回的 RPC 是有效的
        let mut valid_rpc_addresses = vec![
            "https://api.mainnet-beta.solana.com",
            "https://api.testnet.solana.com",
            "https://api.devnet.solana.com",
        ];

        for _ in 0..10 {
            let rpc_client = config.get_rand_rpc();
            assert!(valid_rpc_addresses.contains(&rpc_client.url().as_str()));
        }
    }
}