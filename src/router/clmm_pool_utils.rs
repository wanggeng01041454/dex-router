use anyhow::Result;

use raydium_amm_v3::libraries::{MAX_SQRT_PRICE_X64, MIN_SQRT_PRICE_X64, MulDiv};
use solana_sdk::{epoch_info::EpochInfo, pubkey::Pubkey};
use spl_token_2022::extension::transfer_fee::TransferFeeConfig;

use super::types::PoolInfo;

/// 根据输入的mint和数量计算输出的mint和数量
pub fn compute_amount_out(
  pool: &PoolInfo,
  input_mint: &Pubkey,
  input_amount: u64,
  epoch_info: &EpochInfo,
  sqrt_price_x64_limit: Option<u128>,
) -> Result<()> {
  // Check if the input mint is either mint_a or mint_b
  let mint_a_is_input = pool.base_info.mint_a == *input_mint;
  let (base_fee_config, out_fee_config) = if mint_a_is_input {
    (&pool.base_info.mint_a_info.transfer_fee_config, &pool.base_info.mint_b_info.transfer_fee_config)
  } else {
    (&pool.base_info.mint_b_info.transfer_fee_config, &pool.base_info.mint_a_info.transfer_fee_config)
  };

  // 调整价格限制值
  let sqrt_price_x64_limit =
    sqrt_price_x64_limit.unwrap_or_else(|| if mint_a_is_input { MIN_SQRT_PRICE_X64 + 1 } else { MAX_SQRT_PRICE_X64 - 1 });

  // 计算手续费
  let GetTransferAmountFeeResult { amount: real_amount_in, .. } = get_transfer_amount_fee(input_amount, base_fee_config, epoch_info, false);

  // 真正的计算

  let mut all_needs_accounts = Vec::new();

  // 获取第一个初始化的tickArray, 根据swap方向
  let (is_exist, first_tick_array_start_index) =
    pool.get_first_initialized_tick_array(&pool.dynamic_info.tickarray_bitmap_extension, mint_a_is_input)?;
  if is_exist {
    all_needs_accounts.push(pool.get_pda_tick_array_address(first_tick_array_start_index));
  }

  // swap-compute

  Ok(())
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
