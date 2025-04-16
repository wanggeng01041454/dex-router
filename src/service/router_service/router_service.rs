use std::i32;
use std::str::FromStr;

use crate::nacos_config::entrance::get_nacos_config;
use crate::service::core::account_puller::{self, AccountPuller};
use crate::service::core::clmm_program::{self, SwapRouteInfo};
use crate::service::core::result_utils::convert_result;
use crate::service::pb::base::CommonResult;
use crate::service::pb::router::router_service_server::RouterService;
use crate::service::pb::router::{
  CreateSwapTransactionRequest, CreateSwapTransactionResponse, QuotePriceRequest, QuotePriceResponse, SwapV1Out, TransactionData,
  swap_v1_out::RoutePlan, swap_v1_out::SwapType,
};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use raydium_amm_v3::states::{PoolState, tick_array};
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address;
use tonic::{Request, Response, Status};

use super::route_utils::{self, RouteInformationType};
use super::types::PoolInfo;

#[derive(Debug, Default)]
pub struct DexRouterService {}

#[tonic::async_trait]
impl RouterService for DexRouterService {
  /// 询价服务
  async fn quote_price(&self, request: Request<QuotePriceRequest>) -> Result<Response<QuotePriceResponse>, Status> {
    convert_result(Self::quote_price_impl(request.into_inner()).await)
  }

  /// 构建交易：生成用于交换的交易数据
  async fn create_swap_transaction(
    &self,
    request: Request<CreateSwapTransactionRequest>,
  ) -> Result<Response<CreateSwapTransactionResponse>, Status> {
    convert_result(Self::create_swap_transaction_impl(request.into_inner()).await)
  }
}

impl DexRouterService {
  pub async fn quote_price_impl(req: QuotePriceRequest) -> core::result::Result<QuotePriceResponse, anyhow::Error> {
    let rpc_client = get_nacos_config().await.get_rand_rpc();

    let input_mint = Pubkey::from_str(&req.input_mint)?;
    let output_mint = Pubkey::from_str(&req.output_mint)?;
    let amount: u64 = req.amount.parse()?;
    let base_input = req.is_base_input;

    // todo: 临时加速的话，可以先在启动时获取，但会导致信息不准
    // todo: 池子信息从 redis 中获取
    // 获取所有的池子信息
    let pool_infos = route_utils::fetch_all_clmm_pools(&rpc_client).await?;
    // 打印所有pool的代币信息
    for pool in &pool_infos {
      println!("Pool: {}, {}", pool.base_info.mint_a_info.mint.to_string(), pool.base_info.mint_b_info.mint.to_string());
    }
    let epoch_info = rpc_client.get_epoch_info().await?;
    let all_route_paths = route_utils::get_all_route_path(&input_mint, &output_mint, &pool_infos).await?;
    println!("All Route Path: {}", &all_route_paths);

    if all_route_paths.is_empty() {
      return Err(anyhow::anyhow!("No route found"));
    }

    let best_route = route_utils::compute_best_route(all_route_paths, &input_mint, base_input, amount, &epoch_info).await?;
    println!("Best route: {}", &best_route);
    let rsp = QuotePriceResponse {
      result: Some(CommonResult { ret_code: 0, ret_msg: "Quote price retrieved successfully".to_string() }),
      version: "V0".to_string(),
      open_time: best_route.get_pool_open_time(), // 示例时间戳
      data: Some(SwapV1Out {
        swap_type: SwapType::BaseInUnspecified as i32,
        input_mint: best_route.get_input_mint().to_string(),
        input_amount: best_route.get_amount_in().to_string(),
        output_mint: best_route.get_output_mint().to_string(),
        output_amount: best_route.get_amount_out().to_string(),
        // todo, 这个字段的意义
        other_amount_threshold: "10".to_string(),
        // todo: 好像没意义
        slippage_bps: 50, // 0.5%
        // todo: 导致的价格变化，如果多路径时，如何定义这个参数
        price_impact_pct: 5,
        route_plan: best_route.into_route_plan_vec(),
      }),
    };

    Ok(rsp)
  }

  pub async fn create_swap_transaction_impl(
    req: CreateSwapTransactionRequest,
  ) -> core::result::Result<CreateSwapTransactionResponse, anyhow::Error> {
    let rpc_client = get_nacos_config().await.get_rand_rpc();
    let account_puller = AccountPuller::new(&rpc_client);

    println!("req: {}", serde_json::to_string_pretty(&req)?);

    let swap_rsp = req.swap_response.ok_or(anyhow::anyhow!("Swap response is missing"))?;

    let mut tx_data = String::new();

    // todo: 实际实现时，需要从redis中获取池子信息，暂时先从链上获取
    if swap_rsp.route_plan.len() == 1 {
      // 单池
      // todo : 放到独立的函数中处理
      let pool_id = Pubkey::from_str(&swap_rsp.route_plan[0].pool_id)?;
      let pool_account_data: PoolState = account_puller.get_one_account_data(&pool_id).await?;
      let route_plan = &swap_rsp.route_plan[0];

      let payer = Pubkey::from_str(&req.wallet)?;
      let input_mint_account = Pubkey::from_str(&route_plan.input_mint)?;
      let output_mint_account = Pubkey::from_str(&route_plan.output_mint)?;

      let (input_vault_account, output_vault_account) = if pool_account_data.token_mint_0 == input_mint_account {
        (pool_account_data.token_vault_0, pool_account_data.token_vault_1)
      } else {
        (pool_account_data.token_vault_1, pool_account_data.token_vault_0)
      };

      let tick_array_bitmap_extension_key = PoolInfo::tick_array_bitmap_extension_key(&pool_id);

      let mut tick_array_keys = Vec::new();
      for tk in route_plan.remaining_accounts.iter() {
        let tick_array_key = Pubkey::from_str(&tk)?;
        tick_array_keys.push(tick_array_key);
        println!("===================tick_array-keys: {}", tick_array_key.to_string());
      }

      // todo: 输入参数的amount, 另一个代币的量，逻辑是混乱的，要整理清楚
      let amount: u64 = swap_rsp.input_amount.parse()?;
      let other_amount_threshold: u64 = swap_rsp.other_amount_threshold.parse()?;
      // todo: 直接修改为布尔值？
      let is_base_input = match swap_rsp.swap_type {
        0 => true,
        1 => false,
        _ => return Err(anyhow::anyhow!("Swap type is not supported")),
      };

      let cu_price: u64 = req.compute_unit_price_micro_lamports.parse()?;

      let vtx = clmm_program::build_swap_v2_tx(
        &payer,
        &pool_account_data.amm_config,
        &pool_id,
        &input_mint_account,
        &output_mint_account,
        &input_vault_account,
        &output_vault_account,
        &pool_account_data.observation_key,
        &tick_array_bitmap_extension_key,
        &tick_array_keys,
        amount,
        other_amount_threshold,
        0,
        is_base_input,
        cu_price,
      )
      .await?;

      tx_data = BASE64_STANDARD.encode(bincode::serialize(&vtx)?);
    } else if swap_rsp.route_plan.len() == 2 {
      // 多次跳转的路径，需要使用路由合约进行兑换
      let payer = Pubkey::from_str(&req.wallet)?;
      let route_plan = &swap_rsp.route_plan;

      // 获取输入代币相关信息
      let input_mint = Pubkey::from_str(&route_plan[0].input_mint)?;
      let input_token_account = if !req.input_account.is_empty() {
        Pubkey::from_str(&req.input_account)?
      } else {
        get_associated_token_address(&payer, &input_mint)
      };

      // 获取交易相关金额参数
      let amount_in: u64 = swap_rsp.input_amount.parse()?;
      let amount_out_minimum: u64 = swap_rsp.other_amount_threshold.parse()?;
      let cu_price: u64 = req.compute_unit_price_micro_lamports.parse()?;

      // 构建每一跳的交换信息
      let mut swap_infos = Vec::with_capacity(route_plan.len());

      // 处理两个池子的信息
      for (i, plan) in route_plan.iter().enumerate() {
        // 获取池子账户数据
        let pool_id = Pubkey::from_str(&plan.pool_id)?;
        let pool_account_data: PoolState = account_puller.get_one_account_data(&pool_id).await?;

        let input_mint_account = Pubkey::from_str(&plan.input_mint)?;
        let output_mint_account = Pubkey::from_str(&plan.output_mint)?;

        // 确定代币金库账户
        let (input_vault_account, output_vault_account) = if pool_account_data.token_mint_0 == input_mint_account {
          (pool_account_data.token_vault_0, pool_account_data.token_vault_1)
        } else {
          (pool_account_data.token_vault_1, pool_account_data.token_vault_0)
        };

        // 获取tick array bitmap扩展账户
        let tick_array_bitmap_extension_key = PoolInfo::tick_array_bitmap_extension_key(&pool_id);

        // 解析tick arrays
        let mut tick_arrays = Vec::new();
        for tk in plan.remaining_accounts.iter() {
          let tick_array_key = Pubkey::from_str(tk)?;
          tick_arrays.push(tick_array_key);
        }

        // 构建SwapRouteInfo
        let swap_info = SwapRouteInfo {
          amm_config: pool_account_data.amm_config,
          pool_state: pool_id,
          output_token_mint: output_mint_account,
          input_vault: input_vault_account,
          output_vault: output_vault_account,
          observation_state: pool_account_data.observation_key,
          tick_array_bitmap_extension: Some(tick_array_bitmap_extension_key),
          tick_arrays,
        };

        swap_infos.push(swap_info);
      }

      // 构建路由交易
      let vtx =
        clmm_program::build_route_tx(&payer, &input_mint, &input_token_account, amount_in, amount_out_minimum, &swap_infos, cu_price)
          .await?;

      tx_data = BASE64_STANDARD.encode(bincode::serialize(&vtx)?);
    } else {
      return Err(anyhow::anyhow!("Route plan length is error"));
    }

    // 这里仅作为示例，返回一个默认的响应
    let response = CreateSwapTransactionResponse {
      result: Some(CommonResult { ret_code: 0, ret_msg: "Swap transaction created successfully".to_string() }),
      data: vec![TransactionData { transaction: tx_data }],
    };
    Ok(response)
  }
}
