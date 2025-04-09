use std::collections::HashMap;

use anchor_lang::AccountDeserialize;
use anyhow::Result;
use solana_account_decoder_client_types::UiAccountEncoding;
use solana_client::{
  rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
  rpc_filter::RpcFilterType,
};
use solana_sdk::pubkey::Pubkey;

use crate::{
  constants::{BYREAL_CLMM_PROGRAM_ID, SOL_MINT, WSOL_MINT},
  nacos_config::entrance::get_nacos_config,
};

use super::types::{AllRoutePathInfo, POOL_VERSION_CLMM, PoolBaseInfo, PoolInfo, RoutePathItem, TokenBaseInfo};
use raydium_amm_v3::states::PoolState;

/// 获取 CLMM 池的基本信息
/// fixme: 暂时从链上获取，后续修改为从redis获取
pub async fn fetch_clmm_pool_basic_info() -> Result<Vec<PoolBaseInfo>> {
  let rpc_client = get_nacos_config().await.get_rand_rpc();
  let clmm_pool_datas = rpc_client
    .get_program_accounts_with_config(
      &BYREAL_CLMM_PROGRAM_ID,
      RpcProgramAccountsConfig {
        filters: Some(vec![RpcFilterType::DataSize(PoolState::LEN as u64)]),
        account_config: RpcAccountInfoConfig { encoding: Some(UiAccountEncoding::Base64), ..Default::default() },
        with_context: None,
        sort_results: None,
      },
    )
    .await?;

  let mut pool_infos = Vec::new();
  for (pk, account) in clmm_pool_datas {
    let mut data_slice = account.data.as_slice();
    let account_data: PoolState = PoolState::try_deserialize(&mut data_slice)?;

    pool_infos.push(PoolBaseInfo {
      id: pk,
      version: POOL_VERSION_CLMM,
      tick_spacing: account_data.tick_spacing,
      mint_a: account_data.token_mint_0,
      mint_b: account_data.token_mint_1,
    });
  }

  Ok(pool_infos)
}

/// 计算所有的路由路径
/// 参数校验在外部进行
pub async fn get_all_route_path(input_mint: &Pubkey, output_mint: &Pubkey, clmm_pools: &Vec<PoolBaseInfo>) -> Result<AllRoutePathInfo> {
  // 转换 input_mint 和 output_mint 为 Pubkey(同时将SOL换为WSOL)
  let input_mint = if input_mint.eq(&SOL_MINT) { WSOL_MINT.clone() } else { input_mint.clone() };
  let output_mint = if output_mint.eq(&SOL_MINT) { WSOL_MINT.clone() } else { output_mint.clone() };

  let mut direct_path = Vec::new();
  let mut route_path_map = HashMap::new();

  // 遍历所有的池，计算路由路径
  for pool in clmm_pools {
    if pool.mint_a.eq(&input_mint) && pool.mint_b.eq(&output_mint) {
      // 直接路由
      direct_path.push(pool.clone());
    } else if pool.mint_a.eq(&output_mint) && pool.mint_b.eq(&input_mint) {
      // 直接路由
      direct_path.push(pool.clone());
    } else {
      // 间接路由
      if pool.mint_a.eq(&input_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.mint_b.to_string()).or_default();
        route_path_item.route_mint = pool.mint_b.clone();
        route_path_item.input_mint_pools.push(pool.clone());
      } else if pool.mint_b.eq(&input_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.mint_a.to_string()).or_default();
        route_path_item.route_mint = pool.mint_a.clone();
        route_path_item.input_mint_pools.push(pool.clone());
      } else if pool.mint_a.eq(&output_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.mint_b.to_string()).or_default();
        route_path_item.route_mint = pool.mint_b.clone();
        route_path_item.output_mint_pools.push(pool.clone());
      } else if pool.mint_b.eq(&output_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.mint_a.to_string()).or_default();
        route_path_item.route_mint = pool.mint_a.clone();
        route_path_item.output_mint_pools.push(pool.clone());
      }
    }
  }

  // todo: 间接路由的 mint program id、 decimal 还没有填充

  Ok(AllRoutePathInfo { direct_path: direct_path, route_path_map: route_path_map })
}

/// 计算直接路由的输出金额
pub async fn compute_direct_route_amount_out(
  pool: &PoolInfo,
  input_mint: &TokenBaseInfo,
  input_amount: u64,
  output_mint: &TokenBaseInfo,
) -> () {
}

// #[cfg(test)]
// #[allow(unused_imports)]
// #[allow(unused_variables)]
// #[allow(dead_code)]
// mod tests {
//   use super::*;
//   use crate::nacos_config::entrance::init_nacos;

// }
