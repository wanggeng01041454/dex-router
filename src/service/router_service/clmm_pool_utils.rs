use std::fmt;

use anyhow::Result;

use raydium_amm_v3::libraries::{MAX_SQRT_PRICE_X64, MIN_SQRT_PRICE_X64, MulDiv};
use solana_sdk::{epoch_info::EpochInfo, pubkey::Pubkey};
use spl_token_2022::extension::transfer_fee::TransferFeeConfig;

use super::types::PoolInfo;

/// 计算 swap 的结果集合
#[derive(Debug, Default)]
pub struct OneStepSwapResult {
  pub input_mint: Pubkey,
  pub output_mint: Pubkey,

  // 指定的数量是基于 input 还是 output
  pub base_input: bool,

  // 指定的数量
  pub specified_amount: u64,

  // the amount remaining to be swapped in/out of the input/output asset
  pub amount_specified_remaining: u64,
  // the amount already swapped out/in of the output/input asset
  pub amount_calculated: u64,

  // swap前的价格，流动性信息
  pub before_sqrt_price_x64: u128,
  pub before_tick: i32,
  pub before_liquidity: u128,

  // current sqrt(price), after swap
  pub after_sqrt_price_x64: u128,
  // the tick associated with the current price
  pub after_tick: i32,
  // the current liquidity in range, after swap
  pub after_liquidity: u128,

  // swap过程中，fee 的累加值
  pub fee_amount: u64,

  /// 计算时涉及的 tick-array 的 pubkey
  pub tick_array_keys: Vec<Pubkey>,
}

impl fmt::Display for OneStepSwapResult {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "OneStepSwapResult {{ input_mint: {}, output_mint: {}, base_input: {}, specified_amount: {}, amount_specified_remaining: {}, amount_calculated: {}, before_sqrt_price_x64: {}, before_tick: {}, before_liquidity: {}, after_sqrt_price_x64: {}, after_tick: {}, after_liquidity: {}, fee_amount: {} }}",
      self.input_mint,
      self.output_mint,
      self.base_input,
      self.specified_amount,
      self.amount_specified_remaining,
      self.amount_calculated,
      self.before_sqrt_price_x64,
      self.before_tick,
      self.before_liquidity,
      self.after_sqrt_price_x64,
      self.after_tick,
      self.after_liquidity,
      self.fee_amount
    )
  }
}

// todo: 可能还需要expiration_time
/// 根据输入的mint和数量计算输出的mint和数量
/// `base_input` specified_amount 是 input-amount 还是 output-amount
pub fn compute_another_amount(
  pool: &PoolInfo,
  input_mint: &Pubkey,
  base_input: bool,
  specified_amount: u64,
  epoch_info: &EpochInfo,
  sqrt_price_x64_limit: Option<u128>,
) -> Result<OneStepSwapResult> {
  // Check if the input mint is either mint_a or mint_b
  let zero_for_one = pool.base_info.mint_a_info.mint == *input_mint;
  let (base_fee_config, out_fee_config) = if zero_for_one {
    (&pool.base_info.mint_a_info.transfer_fee_config, &pool.base_info.mint_b_info.transfer_fee_config)
  } else {
    (&pool.base_info.mint_b_info.transfer_fee_config, &pool.base_info.mint_a_info.transfer_fee_config)
  };

  // 调整价格限制值
  let sqrt_price_x64_limit =
    sqrt_price_x64_limit.unwrap_or_else(|| if zero_for_one { MIN_SQRT_PRICE_X64 + 1 } else { MAX_SQRT_PRICE_X64 - 1 });

  // 指定 input-amount时，需要扣除 transfer-fee, 成为实际的 input_amount
  // 指定 output-amount时，需要添加 transfer-fee, 成为实际的 output_amount
  let add_fee = !base_input;
  let GetTransferAmountFeeResult { amount: real_amount_specified, .. } =
    get_transfer_amount_fee(specified_amount, base_fee_config, epoch_info, add_fee);

  // 真正的计算
  // 获取第一个初始化的tickArray, 根据swap方向
  let (is_exist, first_tick_array_start_index) =
    pool.get_first_initialized_tick_array(&pool.dynamic_info.tick_array_bitmap_extension, zero_for_one)?;
  if !is_exist {
    // todo: 报错
  }

  // 准备tick-array的dequeue
  let mut tick_array_dequeue = pool.get_tick_array_dequeue(first_tick_array_start_index, zero_for_one)?;

  // swap-compute
  let (swap_state, tick_array_start_index_vec) = pool.swap_compute(
    zero_for_one,
    true,
    is_exist,
    real_amount_specified,
    first_tick_array_start_index,
    sqrt_price_x64_limit,
    &pool.dynamic_info.tick_array_bitmap_extension,
    &mut tick_array_dequeue,
  )?;

  // 指定 input-amount时，需要扣除 transfer-fee, 成为实际的 input_amount
  // 指定 output-amount时，需要添加 transfer-fee, 成为实际的 output_amount
  let GetTransferAmountFeeResult { amount: real_amount_out, .. } =
    get_transfer_amount_fee(swap_state.amount_calculated, out_fee_config, epoch_info, add_fee);

  let tick_array_keys =
    tick_array_start_index_vec.into_iter().map(|index| PoolInfo::get_pda_tick_array_address(&pool.base_info.id, index)).collect();

  Ok(OneStepSwapResult {
    input_mint: if zero_for_one { pool.base_info.mint_a_info.mint } else { pool.base_info.mint_b_info.mint },
    output_mint: if zero_for_one { pool.base_info.mint_b_info.mint } else { pool.base_info.mint_a_info.mint },
    base_input: base_input,
    specified_amount,
    amount_specified_remaining: swap_state.amount_specified_remaining,
    amount_calculated: real_amount_out,
    before_sqrt_price_x64: pool.dynamic_info.sqrt_price_x64,
    before_tick: pool.dynamic_info.tick_current,
    before_liquidity: pool.dynamic_info.liquidity,
    after_sqrt_price_x64: swap_state.sqrt_price_x64,
    after_tick: swap_state.tick,
    after_liquidity: swap_state.liquidity,
    fee_amount: swap_state.fee_amount,
    tick_array_keys: tick_array_keys,
  })
}

#[derive(Debug, Default)]
pub struct GetTransferAmountFeeResult {
  /// 考虑了手续费的amount
  /// 如果是添加手续费，则是 amount + fee
  /// 如果是扣除手续费，则是 amount - fee
  amount: u64,
  /// 转账手续费
  fee: u64,
  /// 超过该时间，则计算无效， 主要取决于 transfer_fee_config 的 epoch信息
  expiration_time: Option<u64>,
}

/// 计算添加或扣除转账手续费之后的转账金额
pub fn get_transfer_amount_fee(
  amount: u64,
  fee_config: &Option<TransferFeeConfig>,
  epoch_info: &EpochInfo,
  add_fee: bool,
) -> GetTransferAmountFeeResult {
  match fee_config {
    Some(config) => {
      let newer_epoch: u64 = config.newer_transfer_fee.epoch.into();
      let expiration_time = if epoch_info.epoch < newer_epoch {
        // 按 400ms 一个 slot 计算
        Some((newer_epoch * epoch_info.slots_in_epoch - epoch_info.absolute_slot) * 400 / 1000)
      } else {
        None
      };

      let fee = config.calculate_epoch_fee(epoch_info.epoch, amount).unwrap_or(0);

      let amount = if add_fee { amount + fee } else { amount - fee };

      GetTransferAmountFeeResult { amount, fee, expiration_time }
    }
    None => GetTransferAmountFeeResult { amount: amount, ..Default::default() },
  }
}
