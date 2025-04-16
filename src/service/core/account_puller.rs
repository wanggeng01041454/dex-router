use std::sync::Arc;
use anchor_lang::{AccountDeserialize, AccountSerialize};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use spl_token_2022::extension::transfer_fee::TransferFeeConfig;
use spl_token_2022::extension::{BaseStateWithExtensions, StateWithExtensionsMut};
use spl_token_2022::state::Mint;

use super::types::MintAccountBaseInfo;

pub struct AccountPuller<'a> {
  pub rpc_client: &'a RpcClient,
}

impl<'a> AccountPuller<'a> {
  /// Creates a new `AccountPuller` instance with the given `RpcClient`.
  pub fn new(rpc_client: &'a RpcClient) -> Self {
    Self { rpc_client }
  }

  /// Fetches the account information for a given public key list.
  pub async fn get_multi_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<(Pubkey, Option<Account>)>> {
    let account_infos = self.rpc_client.get_multiple_accounts(pubkeys).await?;

    assert_eq!(account_infos.len(), pubkeys.len());
    let mut result = Vec::with_capacity(account_infos.len());
    for (pubkey, account) in pubkeys.iter().zip(account_infos) {
      result.push((*pubkey, account));
    }
    Ok(result)
  }

  /// fetches mint account information for a given public key list,
  /// and gets the extension information at the same time
  pub async fn get_multi_mint_account_with_extension_info(&self, pubkeys: &[Pubkey]) -> Result<Vec<(Pubkey, Option<MintAccountBaseInfo>)>> {
    let mut account_infos = self.get_multi_accounts(pubkeys).await?;

    let result = account_infos
      .iter_mut()
      .map(|(pubkey, account)| {
        let mint_account = match account {
          Some(account) => {
            let data_slice = account.data.as_mut_slice();
            let mint_state = StateWithExtensionsMut::<Mint>::unpack(data_slice)?;

            let mut mint_info =
              MintAccountBaseInfo { mint: *pubkey, decimal: mint_state.base.decimals, program_id: account.owner, ..Default::default() };

            mint_info.transfer_fee_config = if account.owner == spl_token_2022::id() {
              match mint_state.get_extension::<TransferFeeConfig>() {
                Ok(transfer_fee_config) => Some(*transfer_fee_config),
                _ => None,
              }
            } else {
              None
            };

            Some(mint_info)
          }
          None => None,
        };
        Ok((*pubkey, mint_account))
      })
      .collect::<Result<Vec<_>>>();

    result
  }

  pub async fn get_one_mint_account_with_extension_info(&self, pubkey: &Pubkey) -> Result<MintAccountBaseInfo> {
    let account_infos = self.get_multi_mint_account_with_extension_info(&[*pubkey]).await?;

    assert_eq!(account_infos.len(), 1);
    let result = account_infos.into_iter().next().unwrap();
    if result.1.is_none() {
      return Err(anyhow::anyhow!("No account found for the given pubkey: {}", pubkey.to_string()));
    }
    Ok(result.1.unwrap())
  }

  /// 获取多个账户数据,并将其转换成指定的账户类型（所有账户是同一个类型）; 如果给定了校验信息，则进行校验
  /// # account: 账户地址
  /// # owner_id: 账户的所有者ID, 要对该id进行校验
  pub async fn get_multi_account_data<T>(&self, pubkeys: &[Pubkey]) -> Result<Vec<(Pubkey, Option<T>)>>
  where
    T: AccountDeserialize,
  {
    let account_infos = self.get_multi_accounts(pubkeys).await?;

    let result = account_infos
      .iter()
      .map(|(pubkey, account)| {
        let mint_account = match account {
          Some(account) => {
            let mut data_slice = account.data.as_slice();
            let account_data = T::try_deserialize(&mut data_slice)?;

            Some(account_data)
          }
          None => None,
        };
        Ok((*pubkey, mint_account))
      })
      .collect::<Result<Vec<_>>>();

    result
  }

  pub async fn get_one_account_data<T>(&self, pubkey: &Pubkey) -> Result<T>
  where
    T: AccountDeserialize,
  {
    let account_infos = self.get_multi_account_data::<T>(&[*pubkey]).await?;

    assert_eq!(account_infos.len(), 1);
    let result = account_infos.into_iter().next().unwrap();
    if result.1.is_none() {
      return Err(anyhow::anyhow!("No account found for the given pubkey: {}", pubkey.to_string()));
    }
    Ok(result.1.unwrap())
  }
}
