use std::collections::HashMap;

use anchor_lang::prelude::*;
use raydium_amm_v3::states::{TickArrayBitmapExtension, TickArrayState};
use spl_token_2022::extension::transfer_fee::TransferFeeConfig;

pub const POOL_VERSION_CLMM: u8 = 6; // Constant for CLMM pool version
pub const POOL_VERSION_CPMM: u8 = 7; // Constant for CPMM pool version

/// 池子的静态信息
#[derive(Debug, Default, Clone, Copy)]
pub struct PoolBaseInfo {
  pub id: Pubkey,  // Unique identifier for the pool
  pub version: u8, // Version of the pool (6 for CLMM, 7 for CPMM)

  // Which config the pool belongs, 拼装交易时需要
  pub amm_config: Pubkey,

  pub tick_spacing: u16,
  // todo, mint_a 替换为 TokenBaseInfo
  pub mint_a: Pubkey,             // Token A's mint address
  pub mint_a_info: TokenBaseInfo, // Token A's mint info
  pub mint_b: Pubkey,             // Token B's mint address
  pub mint_b_info: TokenBaseInfo, // Token B's mint info

  /// Token pair vault
  pub token_vault_a: Pubkey,
  pub token_vault_b: Pubkey,

  /// observation account key, 价格预言机的账户，拼装交易时需要, mut
  pub observation_key: Pubkey,

  // Key for the tick array bitmap extension,
  // 固定计算所得，可以存下来使用，
  pub tick_array_bitmap_extension_key: Pubkey,

  /// The trade fee, denominated in hundredths of a bip (10^-6)
  pub trade_fee_rate: u32,
}

/// 池子的动态信息
#[derive(Default, Clone)]
pub struct PoolDynamicInfo {
  pub id: Pubkey, // Unique identifier for the pool

  /// The currently in range liquidity available to the pool.
  pub liquidity: u128,
  /// The current price of the pool as a sqrt(token_1/token_0) Q64.64 value
  pub sqrt_price_x64: u128,
  /// The current tick of the pool, i.e. according to the last tick transition that was run.
  pub tick_current: i32,

  /// Packed initialized tick array state
  pub tick_array_bitmap: [u64; 16],

  // todo, 没想好要放在哪里的 pool 相关动态数据解构，都先放在这里，后面统一规划
  pub tick_array_bitmap_extension: TickArrayBitmapExtension,

  // todo: 关于tick-array的获取：现在直接从链上获取，将来从redis中获取后，存储在本地数组中
  // todo: 启动时获取所有的tick-array
  pub all_tick_array_state: Vec<TickArrayState>, // All tick arrays in the pool
}

/// 池子完整信息，包含静态和动态信息
#[derive(Default, Clone)]
pub struct PoolInfo {
  pub base_info: PoolBaseInfo,       // Static information about the pool
  pub dynamic_info: PoolDynamicInfo, // Dynamic information about the pool
}

// todo: poolInfo缓存在本地， 新增时，链解析后端要推送
// todo: token 信息也要在redis中缓存

/// 代币的基本信息
#[derive(Debug, Default, Clone, Copy)]
pub struct TokenBaseInfo {
  pub mint: Pubkey,       // Token mint address
  pub program_id: Pubkey, // Program ID of the token
  pub decimal: u8,        // Decimal precision of the token

  pub transfer_fee_config: Option<TransferFeeConfig>, // Optional transfer fee configuration
}

/// 路由跳转的路径信息
#[derive(Default, Clone)]
pub struct RoutePathItem {
  /// 路由跳转mint
  pub route_mint: Pubkey,

  /// 路由跳转mint的 program id
  pub mint_program_id: Pubkey,

  /// 所有 input mint 和 route mint 的 pool
  pub input_mint_pools: Vec<PoolInfo>,

  /// 所有 output mint 和 route mint 的 pool
  pub output_mint_pools: Vec<PoolInfo>,

  /// 精度
  pub decimal: u8,
}

/// 计算路由返回的信息
#[derive(Default, Clone)]
pub struct AllRoutePathInfo {
  /// 直接路径
  pub direct_path: Vec<PoolInfo>,

  /// 间接路径, 即中转路径
  /// hashmap key 是中转路径的 mint
  pub route_path_map: HashMap<String, RoutePathItem>,
}
