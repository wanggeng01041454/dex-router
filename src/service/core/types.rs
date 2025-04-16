use solana_sdk::pubkey::Pubkey;
use spl_token_2022::extension::transfer_fee::TransferFeeConfig;

/// 代币的基本信息
#[derive(Debug, Default, Clone, Copy)]
pub struct MintAccountBaseInfo {
  pub mint: Pubkey,       // Token mint address
  pub program_id: Pubkey, // Program ID of the token
  pub decimal: u8,        // Decimal precision of the token

  pub transfer_fee_config: Option<TransferFeeConfig>, // Optional transfer fee configuration
}
