use crate::swap::{router_service_server::RouterService, QuoteRequest, QuoteResponse};


#[derive(Default)]
pub struct LocalIndexRouter;

/// 询价服务
#[tonic::async_trait]
impl RouterService for LocalIndexRouter {
  async fn quote(
    &self,
    request: tonic::Request<QuoteRequest>,
  ) -> Result<tonic::Response<QuoteResponse>, tonic::Status> {
    // 处理请求
    let response = QuoteResponse::default();

    Ok(tonic::Response::new(response))
  }
}