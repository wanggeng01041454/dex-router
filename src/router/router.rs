use crate::rpc_proto::{
  CreateSwapTransactionRequest, CreateSwapTransactionResponse, QuotePriceRequest, QuotePriceResponse, router_service_server::RouterService,
};

#[derive(Default)]
pub struct LocalIndexRouter;

/// 询价服务
#[tonic::async_trait]
impl RouterService for LocalIndexRouter {
  async fn quote_price(&self, request: tonic::Request<QuotePriceRequest>) -> Result<tonic::Response<QuotePriceResponse>, tonic::Status> {
    // 处理请求
    let response = QuotePriceResponse::default();

    Ok(tonic::Response::new(response))
  }

  async fn create_swap_transaction(
    &self,
    request: tonic::Request<CreateSwapTransactionRequest>,
  ) -> Result<tonic::Response<CreateSwapTransactionResponse>, tonic::Status> {
    // 处理请求
    let response = CreateSwapTransactionResponse::default();

    Ok(tonic::Response::new(response))
  }

}
