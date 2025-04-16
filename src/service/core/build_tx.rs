use std::{marker::PhantomData, ops::Deref, sync::Arc};

use anchor_client::AsSigner;
use anchor_lang::{InstructionData, ToAccountMetas, prelude::AccountMeta};
use anyhow::Result;
use solana_client::{nonblocking::rpc_client::RpcClient as AsyncRpcClient, rpc_config::RpcSimulateTransactionConfig};
use solana_sdk::{
  compute_budget::ComputeBudgetInstruction,
  hash::{Hash, Hasher},
  instruction::Instruction,
  message::{AddressLookupTableAccount, VersionedMessage, v0::Message as V0Message},
  pubkey::Pubkey,
  signature::{Keypair, Signature},
  signers::Signers,
  transaction::VersionedTransaction,
};

/// 最大的 cu 值
pub const MAX_COMPUTE_UNIT_LIMIT: u32 = 1400000;
/// 添加 compute budget 指令之后的最小 cu 增加值
pub const MIN_CU_INCREASE_AFTER_ADD_COMPUTE_BUDGET_IX: u32 = 450;

#[derive(Default)]
pub struct TransactionBuilder {
  program_id: Pubkey,
  instructions: Vec<Instruction>,
  payer: Pubkey,
  signers: Vec<Keypair>,
  address_lookup_tables: Vec<AddressLookupTableAccount>,
}

// Shared implementation for all RequestBuilders
impl TransactionBuilder {
  #[must_use]
  pub fn set_payer(&mut self, payer: Pubkey) {
    self.payer = payer;
  }

  #[must_use]
  pub fn set_program(&mut self, program_id: Pubkey) {
    self.program_id = program_id;
  }

  pub fn add_build_instruction<A: ToAccountMetas, R: ToAccountMetas, D: InstructionData>(
    &mut self,
    accounts: A,
    remaining_accounts: Option<R>,
    args: D,
  ) {
    let mut accounts = accounts.to_account_metas(None);
    if let Some(remaining_accounts) = remaining_accounts {
      accounts.append(&mut remaining_accounts.to_account_metas(None));
    }
    let ix = Instruction { program_id: self.program_id, accounts, data: args.data() };
    self.instructions.push(ix);
  }

  pub fn add_instruction(&mut self, ix: Instruction) {
    self.instructions.push(ix);
  }

  pub fn add_alt(&mut self, alt: AddressLookupTableAccount) {
    self.address_lookup_tables.push(alt);
  }

  pub fn add_signer(&mut self, signer: Keypair) {
    self.signers.push(signer);
  }

  /// 构造 VersionedTrsansaction
  pub async fn build_versioned_transaction(
    &self,
    async_rpc_client: &AsyncRpcClient,
    cu_price: u64,
    cu_factor: f64,
  ) -> Result<VersionedTransaction> {
    // 否则（需要设置cu，并且没有设置cu），模拟执行交易，获取 cu 值; 并估算最终的cu值
    let sim_cu = self.simulate_transaction(async_rpc_client).await?;
    let mut real_cu = (sim_cu as f64 * cu_factor) as u64;
    if real_cu - sim_cu < MIN_CU_INCREASE_AFTER_ADD_COMPUTE_BUDGET_IX.into() {
      real_cu = sim_cu + MIN_CU_INCREASE_AFTER_ADD_COMPUTE_BUDGET_IX as u64;
    }

    // 在指令集中增加设置cu的指令

    let mut new_ixs = vec![];
    new_ixs.push(ComputeBudgetInstruction::set_compute_unit_limit(real_cu as u32));
    new_ixs.push(ComputeBudgetInstruction::set_compute_unit_price(cu_price));
    new_ixs.extend_from_slice(&self.instructions.as_slice());

    let vtx = Self::build_versioned_transaction_sync(
      async_rpc_client.get_latest_blockhash().await?,
      &self.payer,
      &new_ixs,
      &self.address_lookup_tables,
      &self.signers,
    )?;

    Ok(vtx)
  }

  /// 模拟执行交易，返回错误或者 cu 值
  pub async fn simulate_transaction(&self, async_rpc_client: &AsyncRpcClient) -> Result<u64> {
    let mut tmp_ixs: Vec<Instruction>;

    // try to set max compute unit limit
    tmp_ixs = vec![ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT)];
    tmp_ixs.extend_from_slice(&self.instructions);

    let ixs: &[Instruction] = &tmp_ixs;

    // simulate transaction, 使用空的hash值，模拟执行交易
    let pre_tx =
      Self::build_versioned_transaction_sync(Hasher::default().result(), &self.payer, ixs, &self.address_lookup_tables, &self.signers)?;
    let sim_config = RpcSimulateTransactionConfig {
      sig_verify: false,
      commitment: None,
      replace_recent_blockhash: true,
      encoding: None,
      accounts: None,
      min_context_slot: None,
      inner_instructions: true,
    };

    let simulate_result = async_rpc_client.simulate_transaction_with_config(&pre_tx, sim_config).await?;
    if let Some(tx_err) = simulate_result.value.err {
      let err = format!(
        "simulate transaction error: {}, \nsimulate log:{}",
        tx_err.to_string(),
        serde_json::to_string(&simulate_result.value.logs)?
      );

      return Err(anyhow::anyhow!(err));
    }

    Ok(simulate_result.value.units_consumed.unwrap_or_default())
  }

  /// 同步构造 versioned transaction
  /// @param keypairs: 如果为None，则构建无签名的交易 todo: 创建全签名交易，无签名交易，部分签名交易
  pub fn build_versioned_transaction_sync(
    recent_blockhash: Hash,
    payer_pubkey: &Pubkey,
    instructions: &[Instruction],
    addr_lookup_table: &[AddressLookupTableAccount],
    keypairs: &Vec<Keypair>,
  ) -> Result<VersionedTransaction> {
    let v0_message = V0Message::try_compile(payer_pubkey, instructions, addr_lookup_table, recent_blockhash)?;
    let message = VersionedMessage::V0(v0_message);

    // let versioned_tx = VersionedTransaction::try_new(message, keypairs)?;
    // 将 try_new 的代码拷贝过来进行改造
    // try_new 只支持 私钥个数和签名个数完全匹配的情况；我们这这里要支持部分签名的情况（包括没有签名的情况）
    let static_account_keys = message.static_account_keys();
    if static_account_keys.len() < message.header().num_required_signatures as usize {
      return Err(anyhow::anyhow!("static_account_keys is less than num_required_signatures"));
    }

    let signer_keys = keypairs.try_pubkeys()?;
    let expected_signer_keys = &static_account_keys[0..message.header().num_required_signatures as usize];

    // 如果签名者个数大于预期签名者个数，则返回错误
    if signer_keys.len() > expected_signer_keys.len() {
      return Err(anyhow::anyhow!("signer_keys is more than expected_signer_keys"));
    }

    // 原本的顺序 =map=> 传递进来的顺序;
    // 如果没有找到，则返回 invalid_index
    let invalid_index: usize = (message.header().num_required_signatures + 1).into(); // 无效的索引
    let signature_indexes: Vec<usize> =
      expected_signer_keys.iter().map(|signer_key| signer_keys.iter().position(|key| key == signer_key).unwrap_or(invalid_index)).collect();

    let message_data = message.serialize();
    let unordered_signatures = keypairs.try_sign_message(message_data.as_ref())?;

    let signatures: Vec<Signature> = signature_indexes
      .into_iter()
      .map(|index| {
        if index == invalid_index {
          return Ok(Signature::default());
        }
        unordered_signatures.get(index).copied().ok_or_else(|| anyhow::anyhow!("signature index out of bounds"))
      })
      .collect::<Result<Vec<Signature>, anyhow::Error>>()?;

    Ok(VersionedTransaction { signatures, message })
  }
}
