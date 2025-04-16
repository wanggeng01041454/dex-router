use anyhow::Result;
use raydium_amm_v3::states::tick_array;
use std::{str::FromStr, sync::Arc};

use anchor_client::{Client, Cluster};
use anchor_lang::prelude::*;
use anchor_spl::memo::Memo;
use borsh::{BorshDeserialize, BorshSerialize};

use solana_sdk::{signature::Keypair, system_program, transaction::VersionedTransaction};
use spl_associated_token_account::get_associated_token_address;

use crate::{
  constants::{BYREAL_CLMM_PROGRAM_ID, BYREAL_CLMM_ROUTING_PROGRAM_ID},
  nacos_config::{entrance::get_nacos_config, types::NacosConfig},
};

use super::build_tx::{self, TransactionBuilder};

/// 自定义的RoutingV3指令参数结构
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct RoutingV3Args {
  pub amount_in: u64,
  pub amount_out_minimum: u64,
  pub swap_account_counts: Vec<u8>,
}

pub async fn build_route_tx(
  payer: &Pubkey,
  input_token_mint: &Pubkey,
  input_token_account: &Pubkey,
  amount_in: u64,
  amount_out_minimum: u64,
  swap_infos: &[SwapRouteInfo],
  cu_price: u64,
) -> Result<VersionedTransaction> {
  // 准备核心账户列表
  let mut accounts = Vec::new();

  // 添加主要账户
  accounts.push(AccountMeta::new_readonly(BYREAL_CLMM_PROGRAM_ID, false)); // clmm_program
  accounts.push(AccountMeta::new(*payer, true)); // payer (signer)
  accounts.push(AccountMeta::new(*input_token_account, false)); // input_token_account
  accounts.push(AccountMeta::new_readonly(*input_token_mint, false)); // input_token_mint
  accounts.push(AccountMeta::new_readonly(spl_token::id(), false)); // token_program
  accounts.push(AccountMeta::new_readonly(spl_token_2022::id(), false)); // token_program_2022
  accounts.push(AccountMeta::new_readonly(spl_associated_token_account::id(), false)); // associated_token_program
  accounts.push(AccountMeta::new_readonly(system_program::id(), false)); // system_program
  accounts.push(AccountMeta::new_readonly(Memo::id(), false)); // memo_program

  // 计算每个跳数(swap)需要多少个账户
  let mut swap_account_counts = Vec::with_capacity(swap_infos.len());

  // 添加各个swap操作的剩余账户
  for swap_info in swap_infos {
    // 核心账户: amm_config, pool_state, output_token_account, input_vault, output_vault, output_token_mint, observation_state
    let mut current_swap_account_count = 7; // 7个核心账户

    // amm_config
    accounts.push(AccountMeta::new_readonly(swap_info.amm_config, false));

    // pool_state
    accounts.push(AccountMeta::new(swap_info.pool_state, false));

    // output_token_account (自动计算ATA)
    let output_token_account = get_associated_token_address(payer, &swap_info.output_token_mint);
    accounts.push(AccountMeta::new(output_token_account, false));

    // input_vault
    accounts.push(AccountMeta::new(swap_info.input_vault, false));

    // output_vault
    accounts.push(AccountMeta::new(swap_info.output_vault, false));

    // output_token_mint
    accounts.push(AccountMeta::new_readonly(swap_info.output_token_mint, false));

    // observation_state
    accounts.push(AccountMeta::new(swap_info.observation_state, false));

    // 额外账户: tick_array_bitmap_extension + tick_arrays
    if let Some(tick_array_bitmap) = swap_info.tick_array_bitmap_extension {
      accounts.push(AccountMeta::new_readonly(tick_array_bitmap, false));
      current_swap_account_count += 1;
    }

    // 添加tick arrays
    for tick_array in &swap_info.tick_arrays {
      accounts.push(AccountMeta::new(*tick_array, false));
      current_swap_account_count += 1;
    }

    swap_account_counts.push(current_swap_account_count as u8);
  }

  // ===== 写法1 =====

  // 使用borsh进行序列化
  // 1. 创建指令参数结构体
  let args = RoutingV3Args { amount_in, amount_out_minimum, swap_account_counts };

  // 2. 序列化参数结构体
  let mut serialized_args = args.try_to_vec()?;

  // 3. 创建最终的指令数据：在序列化数据前添加8字节anchor指令标识符
  // routing 指令的前缀: [104, 36, 195, 124, 102, 53, 165, 213]
  let mut data = vec![104, 36, 195, 124, 102, 53, 165, 213];
  data.append(&mut serialized_args);

  // 创建指令
  // 先写死 routing program id
  let ix = solana_sdk::instruction::Instruction { program_id: BYREAL_CLMM_ROUTING_PROGRAM_ID, accounts, data };

  // 将指令放入Vec中，以匹配build_versioned_transaction的参数类型
  // let ixs = vec![ix];

  // ===== 写法2: 可能不能太 work =====
  // let ixs = self
  // .program
  // .request()
  // .accounts(accounts)
  // .args(&[
  //   amount_in.into(),
  //   amount_out_minimum.into(),
  //   swap_account_counts.into(),
  // ])
  // .instructions()?;

  let mut tx_builder = TransactionBuilder::default();
  tx_builder.set_program(BYREAL_CLMM_ROUTING_PROGRAM_ID);
  tx_builder.set_payer(*payer);
  tx_builder.add_instruction(ix);

  // 创建和签名交易
  let nacos_config = get_nacos_config().await;
  let async_rpc_client = nacos_config.get_rand_rpc();
  let cu_factor = nacos_config.get_cu_factor();

  let vtx = tx_builder.build_versioned_transaction(&async_rpc_client, cu_price, cu_factor).await?;

  Ok(vtx)
}

pub async fn build_swap_v2_tx(
  // nacos_config: &NacosConfig,
  payer: &Pubkey,
  amm_config: &Pubkey,
  pool_state: &Pubkey,
  input_mint_account: &Pubkey,
  output_mint_account: &Pubkey,
  input_vault_account: &Pubkey,
  output_vault_account: &Pubkey,
  observation_state: &Pubkey,
  tick_array_bitmap_extension: &Pubkey,
  tick_array_keys: &[Pubkey],
  amount: u64,
  other_amount_threshold: u64,
  sqrt_price_limit_x64: u128,
  is_base_input: bool,
  cu_price: u64,
) -> Result<VersionedTransaction> {
  let input_token_account = get_associated_token_address(payer, input_mint_account);
  let output_token_account = get_associated_token_address(payer, output_mint_account);

  let accounts = raydium_amm_v3::accounts::SwapSingleV2 {
    payer: *payer,
    amm_config: *amm_config,
    pool_state: *pool_state,
    input_token_account: input_token_account,
    output_token_account: output_token_account,
    input_vault: *input_vault_account,
    output_vault: *output_vault_account,
    observation_state: *observation_state,
    token_program: spl_token::id(),
    token_program_2022: spl_token_2022::id(),
    memo_program: Memo::id(),
    input_vault_mint: *input_mint_account,
    output_vault_mint: *output_mint_account,
  };
  let mut remaining_accounts = Vec::new();
  remaining_accounts.push(AccountMeta::new_readonly(*tick_array_bitmap_extension, false));
  for tick_array in tick_array_keys {
    remaining_accounts.push(AccountMeta::new(*tick_array, false));

    println!("===================tick_array-keys: {}", tick_array.to_string())
  }
  let args = raydium_amm_v3::instruction::SwapV2 {
    amount: amount,
    other_amount_threshold: other_amount_threshold,
    sqrt_price_limit_x64: sqrt_price_limit_x64,
    is_base_input: is_base_input,
  };

  let mut tx_builder = TransactionBuilder::default();

  tx_builder.set_program(BYREAL_CLMM_PROGRAM_ID);
  tx_builder.set_payer(*payer);
  tx_builder.add_build_instruction(accounts, Some(remaining_accounts), args);

  let nacos_config = get_nacos_config().await;
  let async_rpc_client = nacos_config.get_rand_rpc();
  let cu_factor = nacos_config.get_cu_factor();

  let vtx = tx_builder.build_versioned_transaction(&async_rpc_client, cu_price, cu_factor).await?;

  Ok(vtx)
}

/// 单个路由跳数的交换信息
#[derive(Clone, Debug)]
pub struct SwapRouteInfo {
  pub amm_config: Pubkey,
  pub pool_state: Pubkey,
  pub output_token_mint: Pubkey,
  pub input_vault: Pubkey,
  pub output_vault: Pubkey,
  pub observation_state: Pubkey,
  pub tick_array_bitmap_extension: Option<Pubkey>,
  pub tick_arrays: Vec<Pubkey>,
}
