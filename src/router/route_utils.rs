use std::collections::HashMap;

use anchor_lang::AccountDeserialize;
use anyhow::Result;
use solana_account_decoder_client_types::UiAccountEncoding;
use solana_client::{
  rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
  rpc_filter::RpcFilterType,
};
use solana_sdk::{epoch_info::EpochInfo, pubkey::Pubkey};

use crate::{
  constants::{BYREAL_CLMM_PROGRAM_ID, SOL_MINT, WSOL_MINT},
  nacos_config::entrance::get_nacos_config,
};

use super::{
  clmm_pool_utils::{self, ComputeAmountOutResult},
  types::{AllRoutePathInfo, POOL_VERSION_CLMM, PoolBaseInfo, PoolInfo, RoutePathItem, TokenBaseInfo},
};
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

    // todo: 获取 mint program id 和 decimal， 以及 token2022 的 transfer-fee

    pool_infos.push(PoolBaseInfo {
      id: pk,
      version: POOL_VERSION_CLMM,
      tick_spacing: account_data.tick_spacing,
      mint_a: account_data.token_mint_0,
      mint_a_info: TokenBaseInfo::default(),
      mint_b: account_data.token_mint_1,
      mint_b_info: TokenBaseInfo::default(),
      ..Default::default()
    });
  }

  Ok(pool_infos)
}

/// 计算所有的路由路径
/// 参数校验在外部进行
pub async fn get_all_route_path(
  input_mint: &Pubkey,
  output_mint: &Pubkey,
  clmm_pools: &Vec<PoolInfo>,
  epoch_info: &EpochInfo,
) -> Result<AllRoutePathInfo> {
  // 转换 input_mint 和 output_mint 为 Pubkey(同时将SOL换为WSOL)
  let input_mint = if input_mint.eq(&SOL_MINT) { WSOL_MINT.clone() } else { input_mint.clone() };
  let output_mint = if output_mint.eq(&SOL_MINT) { WSOL_MINT.clone() } else { output_mint.clone() };

  let mut direct_path = Vec::new();
  let mut route_path_map = HashMap::new();

  // 遍历所有的池，计算路由路径
  for pool in clmm_pools {
    if pool.base_info.mint_a.eq(&input_mint) && pool.base_info.mint_b.eq(&output_mint) {
      // 直接路由
      direct_path.push(pool.clone());
    } else if pool.base_info.mint_a.eq(&output_mint) && pool.base_info.mint_b.eq(&input_mint) {
      // 直接路由
      direct_path.push(pool.clone());
    } else {
      // 间接路由
      if pool.base_info.mint_a.eq(&input_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.base_info.mint_b.to_string()).or_default();
        route_path_item.route_mint = pool.base_info.mint_b.clone();
        route_path_item.input_mint_pools.push(pool.clone());
      } else if pool.base_info.mint_b.eq(&input_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.base_info.mint_a.to_string()).or_default();
        route_path_item.route_mint = pool.base_info.mint_a.clone();
        route_path_item.input_mint_pools.push(pool.clone());
      } else if pool.base_info.mint_a.eq(&output_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.base_info.mint_b.to_string()).or_default();
        route_path_item.route_mint = pool.base_info.mint_b.clone();
        route_path_item.output_mint_pools.push(pool.clone());
      } else if pool.base_info.mint_b.eq(&output_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.base_info.mint_a.to_string()).or_default();
        route_path_item.route_mint = pool.base_info.mint_a.clone();
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
  epoch_info: &EpochInfo,
) -> Result<RouteInformationType> {
  let result = clmm_pool_utils::compute_amount_out(pool, &input_mint.mint, input_amount, epoch_info, None)?;

  Ok(RouteInformationType::DirectRoute { pool: pool.clone(), swap_result: result })
}

/// 计算单跳间接路由的输出金额,
/// 如果有多个单跳路径，返回 output_amount 最大的路径
pub async fn compute_one_hop_indirect_route_amount_out(
  route: &RoutePathItem,
  input_mint: &TokenBaseInfo,
  input_amount: u64,
  epoch_info: &EpochInfo,
) -> Result<RouteInformationType> {
  let mut route_information = None;
  for first_idx in 0..route.input_mint_pools.len() {
    for second_idx in 0..route.output_mint_pools.len() {
      let pool_1 = &route.input_mint_pools[first_idx];
      let result_1 = clmm_pool_utils::compute_amount_out(pool_1, &input_mint.mint, input_amount, epoch_info, None)?;

      let pool_2 = &route.output_mint_pools[second_idx];
      let result_2 = clmm_pool_utils::compute_amount_out(pool_2, &result_1.output_mint, result_1.amount_calculated, epoch_info, None)?;

      route_information = match route_information {
        Some(RouteInformationType::OneHopIndirectRoute { pools, swap_results }) => {
          if result_2.amount_calculated > swap_results[1].amount_calculated {
            Some(RouteInformationType::OneHopIndirectRoute { pools: [pool_1.clone(), pool_2.clone()], swap_results: [result_1, result_2] })
          } else {
            Some(RouteInformationType::OneHopIndirectRoute { pools, swap_results })
          }
        }
        None => {
          Some(RouteInformationType::OneHopIndirectRoute { pools: [pool_1.clone(), pool_2.clone()], swap_results: [result_1, result_2] })
        }
        _ => {
          // 其他类型的路由信息
          // 不可能的路径
          None
        }
      }
    }
  }

  Ok(route_information.unwrap())
}

pub enum RouteInformationType {
  /// 直接路由
  DirectRoute { pool: PoolInfo, swap_result: ComputeAmountOutResult },
  /// 单跳间接路由
  OneHopIndirectRoute { pools: [PoolInfo; 2], swap_results: [ComputeAmountOutResult; 2] },
}

// #[cfg(test)]
// #[allow(unused_imports)]
// #[allow(unused_variables)]
// #[allow(dead_code)]
// mod tests {
//   use super::*;
//   use crate::nacos_config::entrance::init_nacos;

// }
