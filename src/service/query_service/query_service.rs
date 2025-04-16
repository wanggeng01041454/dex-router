use crate::service::core::account_puller;
use crate::service::core::account_puller::AccountPuller;
use crate::service::pb::base::CommonResult;
use crate::service::pb::query::query_service_server::QueryService;
use crate::service::pb::query::{
  CheckTxRequest, CheckTxResponse, GetContractInfoRequest, GetContractInfoResponse, ListLinePositionRequest, ListLinePositionResponse,
  PointData,
};
use anchor_lang::AccountDeserialize;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use log::{error, info, log};
use solana_sdk::transaction::VersionedTransaction;
use std::str::FromStr;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Default, Debug)]
pub struct DexQueryService {

}

#[tonic::async_trait]
impl QueryService for DexQueryService {
  /// 确认交易未被篡改：验证签名后的交易是否合法
  async fn check_tx(&self, request: Request<CheckTxRequest>) -> Result<Response<CheckTxResponse>, Status> {
    // 实现交易检查逻辑
    let req = request.into_inner();
    info!("check_tx request: {:?}", req);

    if req.pre_data.len() != req.data.len() {
      return Err(Status::invalid_argument("pre_data and data length mismatch"));
    }

    for (pre_b64, signed_b64) in req.pre_data.iter().zip(req.data.iter()) {
      // 解析未签名交易
      let pre_tx: VersionedTransaction = {
        let decoded =
          BASE64_STANDARD.decode(pre_b64).map_err(|e| Status::invalid_argument(format!("Failed to decode pre_data base64: {}", e)))?;
        bincode::deserialize(&decoded).map_err(|e| Status::invalid_argument(format!("Failed to deserialize pre_data: {}", e)))?
      };

      // 解析签名交易
      let signed_tx: VersionedTransaction = {
        let decoded =
          BASE64_STANDARD.decode(signed_b64).map_err(|e| Status::invalid_argument(format!("Failed to decode data base64: {}", e)))?;
        bincode::deserialize(&decoded).map_err(|e| Status::invalid_argument(format!("Failed to deserialize data: {}", e)))?
      };

      // 比较 message 是否相等
      if pre_tx.message != signed_tx.message {
        return Err(Status::permission_denied("Transaction message mismatch — possible tampering"));
      }
    }
    Ok(Response::new(CheckTxResponse { result: Some(CommonResult { ret_code: 0, ret_msg: "success".to_string() }) }))
  }

  /// 获取代币的基本信息
  async fn get_contract_info(&self, request: Request<GetContractInfoRequest>) -> Result<Response<GetContractInfoResponse>, Status> {
    // 实现获取合约信息的逻辑
    let _req = request.into_inner();
    info!("get_contract_info request: {:?}", _req);

    let rpc_client = crate::nacos_config::entrance::get_nacos_config().await.get_rand_rpc();
    let puller=crate::service::core::account_puller::AccountPuller::new(&rpc_client);

    // 将 String 转换为 Pubkey
    let input_mint =
      solana_sdk::pubkey::Pubkey::from_str(&_req.input_mint).map_err(|e| Status::invalid_argument(format!("Invalid pubkey: {}", e)))?;

    // 使用 await 处理 Future
    let account_data = puller.get_one_mint_account_with_extension_info(&input_mint).await;
    if let Err(err) = account_data {
      error!("Failed to get account data: {}", err);
      return Err(Status::internal("Failed to get account data"));
    }
    let data = account_data.unwrap();
    // 这里仅作为示例，返回一个默认的响应
    let response = GetContractInfoResponse {
      result: Some(CommonResult { ret_code: 0, ret_msg: "Contract info retrieved successfully".to_string() }),
      program_id: data.program_id.to_string(),
      decimal: data.decimal as i32,
    };
    Ok(Response::new(response))
  }

  /// 获取仓位信息
  async fn list_line_position(&self, request: Request<ListLinePositionRequest>) -> Result<Response<ListLinePositionResponse>, Status> {
    // 实现获取仓位信息的逻辑
    let _req = request.into_inner();
    info!("list_line_position request: {:?}", _req);

    let pool_id =
      solana_sdk::pubkey::Pubkey::from_str(&_req.pool_id).map_err(|e| Status::invalid_argument(format!("Invalid pubkey: {}", e)))?;

    let rpc_client = crate::nacos_config::entrance::get_nacos_config().await.get_rand_rpc();
    let puller=crate::service::core::account_puller::AccountPuller::new(&rpc_client);

    let vec1 = crate::service::router_service::route_utils::get_pool_tick_liquidity(&puller,&pool_id).await;

    if vec1.is_err() {
      return Err(Status::internal("Failed to get pool tick liquidity"));
    }
    let mut tick_data: Vec<PointData> = Vec::new();

    for out in vec1.unwrap() {
      tick_data.push(PointData {
        price: out.tick_price.to_string(),
        liquidity: out.liquidity.to_string(),
        tick: out.tick_index as i64,
      })
    }
    // 这里仅作为示例，返回一个默认的响应
    let response = ListLinePositionResponse {
      result: Some(CommonResult { ret_code: 0, ret_msg: "Line position info retrieved successfully".to_string() }),
      count: tick_data.len() as i64,
      data: tick_data,
    };
    Ok(Response::new(response))
  }
}
