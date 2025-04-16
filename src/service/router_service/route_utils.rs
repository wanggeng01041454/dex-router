use rust_decimal::{Decimal, MathematicalOps, prelude::FromPrimitive};
use std::{collections::HashMap, fmt};

use anchor_lang::AccountDeserialize;
use anyhow::Result;
use raydium_amm_v3::libraries::tick_math;
use solana_account_decoder_client_types::UiAccountEncoding;
use solana_client::{
  nonblocking::rpc_client::RpcClient,
  rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
  rpc_filter::RpcFilterType,
};
use solana_sdk::{epoch_info::EpochInfo, pubkey::Pubkey};

use crate::{
  constants::{BYREAL_CLMM_PROGRAM_ID, SOL_MINT, WSOL_MINT},
  service::{
    core::{account_puller::AccountPuller, types::MintAccountBaseInfo},
    pb::{base, router::swap_v1_out::RoutePlan},
  },
};

use super::{
  clmm_pool_utils::{self, OneStepSwapResult},
  types::{AllRoutePathInfo, POOL_VERSION_CLMM, PoolBaseInfo, PoolDynamicInfo, PoolInfo, RoutePathItem},
};
use crate::service::router_service::types::{OutputTickLiquidityInfo, TICK_ARRAY_SIZE, TickLiquidityInfo};
use raydium_amm_v3::states::{AmmConfig, PoolState, TickArrayBitmapExtension, TickArrayState};

/// 从网络（redis） 或者 本地缓存获取mint信息
pub async fn get_mint_info(
  mint: &Pubkey,
  account_puller: &AccountPuller<'_>,
  local_cache: &mut HashMap<String, MintAccountBaseInfo>,
) -> Result<MintAccountBaseInfo> {
  if local_cache.contains_key(&mint.to_string()) {
    return Ok(local_cache.get(&mint.to_string()).unwrap().clone());
  }

  let mint_account = account_puller.get_one_mint_account_with_extension_info(mint).await?;

  local_cache.insert(mint.to_string(), mint_account.clone());
  Ok(mint_account)
}

// todo: 暂时从链上获取，后续修改为从redis获取
/// 获取所有 CLMM 池的基本信息
pub async fn fetch_all_clmm_pools(rpc_client: &RpcClient) -> Result<Vec<PoolInfo>> {
  let account_puller = AccountPuller::new(rpc_client);

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
  let mut local_mint_cache = HashMap::new();

  for (pool_id, account) in clmm_pool_datas {
    let mut data_slice = account.data.as_slice();
    let pool_account_data: PoolState = PoolState::try_deserialize(&mut data_slice)?;

    // todo: 都是需要从redis获取的
    let mint_a_info = get_mint_info(&pool_account_data.token_mint_0, &account_puller, &mut local_mint_cache).await?;
    let mint_b_info = get_mint_info(&pool_account_data.token_mint_1, &account_puller, &mut local_mint_cache).await?;
    let amm_config: AmmConfig = account_puller.get_one_account_data(&pool_account_data.amm_config).await?;

    let tick_array_bitmap_extension_key = PoolInfo::tick_array_bitmap_extension_key(&pool_id);
    let base_info = PoolBaseInfo {
      id: pool_id.clone(),
      version: POOL_VERSION_CLMM,
      amm_config: pool_account_data.amm_config,
      open_time: pool_account_data.open_time,
      tick_spacing: pool_account_data.tick_spacing,
      mint_a_info: mint_a_info,
      mint_b_info: mint_b_info,
      token_vault_a: pool_account_data.token_vault_0,
      token_vault_b: pool_account_data.token_vault_1,
      observation_key: pool_account_data.observation_key,
      tick_array_bitmap_extension_key: tick_array_bitmap_extension_key,
      trade_fee_rate: amm_config.trade_fee_rate,
    };

    let tick_array_bitmap_extension: TickArrayBitmapExtension =
      account_puller.get_one_account_data(&tick_array_bitmap_extension_key).await?;

    let tick_array_bitmap = pool_account_data.tick_array_bitmap;
    let tick_array_keys =
      PoolInfo::calculate_all_tick_array_keys(&pool_id, base_info.tick_spacing, &tick_array_bitmap, &tick_array_bitmap_extension);
    let tick_arrays = account_puller
      .get_multi_account_data::<TickArrayState>(&tick_array_keys)
      .await?
      .iter()
      .map(|(_, data)| data.unwrap())
      .collect::<Vec<_>>();

    let dynamic_info = PoolDynamicInfo {
      id: pool_id.clone(),
      liquidity: pool_account_data.liquidity,
      sqrt_price_x64: pool_account_data.sqrt_price_x64,
      tick_current: pool_account_data.tick_current,
      tick_array_bitmap: tick_array_bitmap,
      // todo: 这里需要从redis获取
      tick_array_bitmap_extension: tick_array_bitmap_extension,

      // todo: 这里需要从redis获取
      all_tick_array_state: tick_arrays,
    };

    pool_infos.push(PoolInfo { base_info: base_info, dynamic_info: dynamic_info });
  }

  Ok(pool_infos)
}

/// 计算所有的路由路径
/// 参数校验在外部进行
pub async fn get_all_route_path(input_mint: &Pubkey, output_mint: &Pubkey, clmm_pools: &Vec<PoolInfo>) -> Result<AllRoutePathInfo> {
  // 转换 input_mint 和 output_mint 为 Pubkey(同时将SOL换为WSOL)
  let input_mint = if input_mint.eq(&SOL_MINT) { WSOL_MINT.clone() } else { input_mint.clone() };
  let output_mint = if output_mint.eq(&SOL_MINT) { WSOL_MINT.clone() } else { output_mint.clone() };

  let mut direct_path = Vec::new();
  let mut route_path_map = HashMap::new();

  // 遍历所有的池，计算路由路径
  for pool in clmm_pools {
    if pool.base_info.mint_a_info.mint.eq(&input_mint) && pool.base_info.mint_b_info.mint.eq(&output_mint) {
      // 直接路由
      direct_path.push(pool.clone());
    } else if pool.base_info.mint_a_info.mint.eq(&output_mint) && pool.base_info.mint_b_info.mint.eq(&input_mint) {
      // 直接路由
      direct_path.push(pool.clone());
    } else {
      // 间接路由
      if pool.base_info.mint_a_info.mint.eq(&input_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.base_info.mint_b_info.mint.to_string()).or_default();
        route_path_item.route_mint = pool.base_info.mint_b_info.clone();
        route_path_item.input_mint_pools.push(pool.clone());
      } else if pool.base_info.mint_b_info.mint.eq(&input_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.base_info.mint_a_info.mint.to_string()).or_default();
        route_path_item.route_mint = pool.base_info.mint_a_info.clone();
        route_path_item.input_mint_pools.push(pool.clone());
      } else if pool.base_info.mint_a_info.mint.eq(&output_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.base_info.mint_b_info.mint.to_string()).or_default();
        route_path_item.route_mint = pool.base_info.mint_b_info.clone();
        route_path_item.output_mint_pools.push(pool.clone());
      } else if pool.base_info.mint_b_info.mint.eq(&output_mint) {
        let route_path_item: &mut RoutePathItem = route_path_map.entry(pool.base_info.mint_a_info.mint.to_string()).or_default();
        route_path_item.route_mint = pool.base_info.mint_a_info.clone();
        route_path_item.output_mint_pools.push(pool.clone());
      }
    }
  }

  Ok(AllRoutePathInfo { direct_paths: direct_path, route_paths_map: route_path_map })
}

pub async fn compute_best_route(
  all_route_paths: AllRoutePathInfo,
  input_mint: &Pubkey,
  base_input: bool,
  specified_amount: u64,
  epoch_info: &EpochInfo,
) -> Result<RouteInformationType> {
  let mut best_route = RouteInformationType::default();

  for direct_path in all_route_paths.direct_paths {
    let result_route = compute_direct_route(&direct_path, input_mint, base_input, specified_amount, &epoch_info).await?;
    best_route = RouteInformationType::better(best_route, result_route);
  }

  for (_, route_path) in all_route_paths.route_paths_map {
    // todo: 优化

    let result_route = if base_input {
      compute_one_hop_indirect_route_base_input(&route_path, input_mint, specified_amount, &epoch_info).await?
    } else {
      compute_one_hop_indirect_route_base_output(&route_path, input_mint, specified_amount, &epoch_info).await?
    };

    best_route = RouteInformationType::better(best_route, result_route);
  }

  Ok(best_route)
}

/// 计算直接路由的输出金额
pub async fn compute_direct_route(
  pool: &PoolInfo,
  input_mint: &Pubkey,
  base_input: bool,
  specified_amount: u64,
  epoch_info: &EpochInfo,
) -> Result<RouteInformationType> {
  let result = clmm_pool_utils::compute_another_amount(pool, &input_mint, base_input, specified_amount, epoch_info, None)?;

  Ok(RouteInformationType::DirectRoute { pool: pool.clone(), swap_result: result })
}

// todo: 优化
// todo: 单跳路由时，如果中间代币没有被消耗完，还要再将它置换成 input 代币 或者 output 代币, 要保证中间代币的数量是0

/// 计算单跳间接路由的输出金额,
/// 如果有多个单跳路径，返回 output_amount 最大的路径
pub async fn compute_one_hop_indirect_route_base_input(
  route: &RoutePathItem,
  input_mint: &Pubkey,
  input_amount: u64,
  epoch_info: &EpochInfo,
) -> Result<RouteInformationType> {
  // 默认的路由信息，
  let mut route_information = RouteInformationType::default();

  for first_idx in 0..route.input_mint_pools.len() {
    for second_idx in 0..route.output_mint_pools.len() {
      let pool_1 = &route.input_mint_pools[first_idx];
      let result_1 = clmm_pool_utils::compute_another_amount(pool_1, &input_mint, true, input_amount, epoch_info, None)?;

      assert_eq!(result_1.output_mint, route.route_mint.mint);

      let pool_2 = &route.output_mint_pools[second_idx];
      let result_2 =
        clmm_pool_utils::compute_another_amount(pool_2, &route.route_mint.mint, true, result_1.amount_calculated, epoch_info, None)?;

      let new_route_information =
        RouteInformationType::OneHopIndirectRoute { pools: [pool_1.clone(), pool_2.clone()], swap_results: [result_1, result_2] };
      route_information = RouteInformationType::better(route_information, new_route_information);
    }
  }

  Ok(route_information)
}

/// 基于output-amount, 计算 input 的量
pub async fn compute_one_hop_indirect_route_base_output(
  route: &RoutePathItem,
  input_mint: &Pubkey,
  output_amount: u64,
  epoch_info: &EpochInfo,
) -> Result<RouteInformationType> {
  // 默认的路由信息，
  let mut route_information = RouteInformationType::default();

  // 先根据输出，计算第2步的路由，再返回第1步进行计算
  for first_idx in 0..route.input_mint_pools.len() {
    for second_idx in 0..route.output_mint_pools.len() {
      let pool_2 = &route.output_mint_pools[second_idx];
      let result_2 = clmm_pool_utils::compute_another_amount(pool_2, &route.route_mint.mint, false, output_amount, epoch_info, None)?;

      let pool_1 = &route.input_mint_pools[first_idx];
      let result_1 = clmm_pool_utils::compute_another_amount(pool_1, &input_mint, false, result_2.amount_calculated, epoch_info, None)?;

      let new_route_information =
        RouteInformationType::OneHopIndirectRoute { pools: [pool_1.clone(), pool_2.clone()], swap_results: [result_1, result_2] };
      route_information = RouteInformationType::better(route_information, new_route_information);
    }
  }

  Ok(route_information)
}

pub enum RouteInformationType {
  /// 直接路由
  DirectRoute { pool: PoolInfo, swap_result: OneStepSwapResult },
  /// 单跳间接路由
  OneHopIndirectRoute { pools: [PoolInfo; 2], swap_results: [OneStepSwapResult; 2] },
}

impl Default for RouteInformationType {
  fn default() -> Self {
    RouteInformationType::DirectRoute { pool: PoolInfo::default(), swap_result: OneStepSwapResult::default() }
  }
}

impl fmt::Display for RouteInformationType {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      RouteInformationType::DirectRoute { pool, swap_result } => {
        write!(f, "DirectRoute: Pool: {}, SwapResult: {}", pool.base_info.id, swap_result)
      }
      RouteInformationType::OneHopIndirectRoute { pools, swap_results } => {
        write!(
          f,
          "OneHopIndirectRoute: Pools: [{}, {}], SwapResults: [{}, {}]",
          pools[0].base_info.id, pools[1].base_info.id, swap_results[0], swap_results[1]
        )
      }
    }
  }
}

impl RouteInformationType {
  /// 两个路由中较好的那个
  /// 哪条路径计算所得的 another_amount 更大, 哪条路径更优
  pub fn better(first: Self, second: Self) -> Self {
    let first_another_amount = if first.is_base_input() { first.get_amount_out() } else { first.get_amount_in() };
    let second_another_amount = if second.is_base_input() { second.get_amount_out() } else { second.get_amount_in() };
    if first_another_amount > second_another_amount { first } else { second }
  }

  pub fn is_base_input(&self) -> bool {
    match self {
      RouteInformationType::DirectRoute { swap_result, .. } => swap_result.base_input,
      RouteInformationType::OneHopIndirectRoute { swap_results, .. } => swap_results[0].base_input,
    }
  }

  pub fn get_input_mint(&self) -> Pubkey {
    match self {
      RouteInformationType::DirectRoute { pool, .. } => pool.base_info.mint_a_info.mint,
      RouteInformationType::OneHopIndirectRoute { pools, .. } => pools[0].base_info.mint_a_info.mint,
    }
  }

  pub fn get_output_mint(&self) -> Pubkey {
    match self {
      RouteInformationType::DirectRoute { pool, .. } => pool.base_info.mint_b_info.mint,
      RouteInformationType::OneHopIndirectRoute { pools, .. } => pools[1].base_info.mint_b_info.mint,
    }
  }

  /// 返回实际使用的（实际得到的） amount-in
  pub fn get_amount_in(&self) -> u64 {
    match self {
      RouteInformationType::DirectRoute { swap_result, .. } => {
        if swap_result.base_input {
          swap_result.specified_amount - swap_result.amount_specified_remaining
        } else {
          swap_result.amount_calculated
        }
      }
      RouteInformationType::OneHopIndirectRoute { swap_results, .. } => {
        if swap_results[0].base_input {
          swap_results[0].specified_amount - swap_results[0].amount_specified_remaining
        } else {
          swap_results[0].amount_calculated
        }
      }
    }
  }

  /// 返回实际使用的（实际得到的） amount-out
  pub fn get_amount_out(&self) -> u64 {
    match self {
      RouteInformationType::DirectRoute { swap_result, .. } => {
        if swap_result.base_input {
          swap_result.amount_calculated
        } else {
          swap_result.specified_amount - swap_result.amount_specified_remaining
        }
      }
      RouteInformationType::OneHopIndirectRoute { swap_results, .. } => {
        if swap_results[1].base_input {
          swap_results[1].amount_calculated
        } else {
          swap_results[1].specified_amount - swap_results[1].amount_specified_remaining
        }
      }
    }
  }

  pub fn get_pool_open_time(&self) -> u64 {
    match self {
      RouteInformationType::DirectRoute { pool, .. } => pool.base_info.open_time,
      // 多跳路由，返回最晚的时间
      RouteInformationType::OneHopIndirectRoute { pools, .. } => {
        let mut open_time = 0;
        for pool in pools {
          if pool.base_info.open_time > open_time {
            open_time = pool.base_info.open_time;
          }
        }
        open_time
      }
    }
  }

  pub fn into_route_plan_vec(&self) -> Vec<RoutePlan> {
    match self {
      RouteInformationType::DirectRoute { pool, swap_result } => {
        vec![RoutePlan {
          pool_id: pool.base_info.id.to_string(),
          input_mint: swap_result.input_mint.to_string(),
          output_mint: swap_result.output_mint.to_string(),
          fee_mint: if swap_result.base_input {
            pool.base_info.mint_a_info.mint.to_string()
          } else {
            pool.base_info.mint_b_info.mint.to_string()
          },
          //todo
          fee_rate: pool.base_info.trade_fee_rate as i32,
          // todo
          fee_amount: swap_result.fee_amount.to_string(),
          remaining_accounts: swap_result.tick_array_keys.iter().map(|x| x.to_string()).collect(),
          // todo: 这个字段的意义， 是交易前池子的价格，还是交易后池子的价格？
          // todo: 显示格式？
          last_pool_price_x64: swap_result.before_sqrt_price_x64.to_string(),
        }]
      }
      RouteInformationType::OneHopIndirectRoute { pools, swap_results } => {
        vec![
          RoutePlan {
            pool_id: pools[0].base_info.id.to_string(),
            input_mint: swap_results[0].input_mint.to_string(),
            output_mint: swap_results[0].output_mint.to_string(),
            fee_mint: if swap_results[0].base_input {
              pools[0].base_info.mint_a_info.mint.to_string()
            } else {
              pools[0].base_info.mint_b_info.mint.to_string()
            },
            //todo
            fee_rate: pools[0].base_info.trade_fee_rate as i32,
            // todo
            fee_amount: swap_results[0].fee_amount.to_string(),
            remaining_accounts: swap_results[0].tick_array_keys.iter().map(|x| x.to_string()).collect(),
            // todo: 这个字段的意义， 是交易前池子的价格，还是交易后池子的价格？
            // todo: 显示格式？
            last_pool_price_x64: swap_results[0].before_sqrt_price_x64.to_string(),
          },
          RoutePlan {
            pool_id: pools[1].base_info.id.to_string(),
            input_mint: swap_results[1].input_mint.to_string(),
            output_mint: swap_results[1].output_mint.to_string(),
            fee_mint: if swap_results[1].base_input {
              pools[1].base_info.mint_a_info.mint.to_string()
            } else {
              pools[1].base_info.mint_b_info.mint.to_string()
            },
            //todo
            fee_rate: pools[1].base_info.trade_fee_rate as i32,
            // todo
            fee_amount: swap_results[1].fee_amount.to_string(),
            remaining_accounts: swap_results[1].tick_array_keys.iter().map(|x| x.to_string()).collect(),
            // todo: 这个字段的意义， 是交易前池子的价格，还是交易后池子的价格？
            // todo: 显示格式？
            last_pool_price_x64: swap_results[1].before_sqrt_price_x64.to_string(),
          },
        ]
      }
    }
  }
}

// #[cfg(test)]
// #[allow(unused_imports)]
// #[allow(unused_variables)]
// #[allow(dead_code)]
// mod tests {
//   use super::*;
//   use crate::nacos_config::entrance::init_nacos;

// }

pub async fn get_pool_tick_liquidity<'a>(account_puller: &'a AccountPuller<'a>, pool_id: &Pubkey) -> Result<Vec<OutputTickLiquidityInfo>> {
  let pool_account_data: PoolState = account_puller.get_one_account_data(pool_id).await?;

  let tick_array_bitmap_extension_key = PoolInfo::tick_array_bitmap_extension_key(pool_id);

  let tick_array_bitmap_extension: TickArrayBitmapExtension = account_puller.get_one_account_data(&tick_array_bitmap_extension_key).await?;

  let tick_array_bitmap = pool_account_data.tick_array_bitmap;
  let tick_array_keys =
    PoolInfo::calculate_all_tick_array_keys(pool_id, pool_account_data.tick_spacing, &tick_array_bitmap, &tick_array_bitmap_extension);
  let tick_liquidity_list = account_puller
    .get_multi_account_data::<TickArrayState>(&tick_array_keys)
    .await?
    .iter()
    .map(|(_, data)| Box::new(data.unwrap())) // 将TickArrayState包装在Box中
    .collect::<Vec<_>>();

  let tick_liquidity_infos = generate_tick_liquidity_info(tick_liquidity_list);

  let mut output_tick_liquidity_infos: Vec<OutputTickLiquidityInfo> = Vec::new();

  for tick_liquidity_info in tick_liquidity_infos.iter() {
    let sqrt_price = tick_math::get_sqrt_price_at_tick(tick_liquidity_info.tick_index)?;
    let tick_price = calculate_price(sqrt_price, pool_account_data.mint_decimals_0, pool_account_data.mint_decimals_1);

    output_tick_liquidity_infos.push(OutputTickLiquidityInfo {
      tick_index: tick_liquidity_info.tick_index,
      tick_price: tick_price.to_string(),
      liquidity: tick_liquidity_info.liquidity,
    });
  }
  Ok(output_tick_liquidity_infos)
}

/// 生成每个 tick 的流动性信息
///
pub fn generate_tick_liquidity_info(tick_array_accounts: Vec<Box<TickArrayState>>) -> Vec<Box<TickLiquidityInfo>> {
  let mut tick_liquidity_infos: Vec<Box<TickLiquidityInfo>> = Vec::new();

  // 首先遍历每个 tick_array_account,
  // 同时检查每个 tick_array_account 的 start_tick_index 是严格递增的
  let mut last_tick_index = i32::MIN;

  // 开始检查时的流动性为0，
  // 流动性为0时，不记录对应tick的流动性
  let mut cur_liquidity: i128 = 0;

  for tick_array_account in tick_array_accounts {
    let tick_array_account = tick_array_account.as_ref();

    // 检查 start_tick_index 是否严格递增
    if tick_array_account.start_tick_index <= last_tick_index {
      // println!("LastTickIndex: {}, CurrentTickArrayAccount: {:?}", last_tick_index, tick_array_account);
      panic!("Start tick index not match");
    }
    last_tick_index = tick_array_account.start_tick_index;

    // 逐个检查每个 tick 的流动性
    for i in 0..TICK_ARRAY_SIZE {
      let tick_state = tick_array_account.ticks[i as usize];

      if tick_state.liquidity_net == 0 {
        continue;
      }
      // 从左向右穿过该tick的流动性
      let pre_tick_liquidity = cur_liquidity;
      cur_liquidity += tick_state.liquidity_net;
      if cur_liquidity != 0 {
        tick_liquidity_infos.push(Box::new(TickLiquidityInfo { tick_index: tick_state.tick, liquidity: cur_liquidity as u128 }));
      }

      // 如果出现了流动性从非0到0的情况，记录前1个tick的流动性
      if pre_tick_liquidity != 0 && cur_liquidity == 0 {
        tick_liquidity_infos.push(Box::new(TickLiquidityInfo { tick_index: tick_state.tick - 1, liquidity: pre_tick_liquidity as u128 }));
      }
    }
  }

  tick_liquidity_infos
}

/// 计算价格，计算价格时需要
fn calculate_price(sqrt_price_x64: u128, token0_decimals: u8, token1_decimals: u8) -> Decimal {
  let denom = Decimal::from_u128(1_u128 << 64).unwrap();
  let sqrt_price = Decimal::from_u128(sqrt_price_x64).unwrap().checked_div(denom).unwrap();

  let price0 = sqrt_price.powu(2);
  let exponent = Decimal::from(10).powi(token0_decimals as i64 - token1_decimals as i64);

  price0.checked_mul(exponent).unwrap()
}
