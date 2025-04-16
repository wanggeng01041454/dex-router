extern crate core;

use crate::service::{pb::query::*, pb::router::*, query_service::DexQueryService, router_service::DexRouterService};
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::{Request, Status, transport::Server};
use tower::{ServiceBuilder, util::MapRequestLayer};
use trace::set_trace_id;

use crate::app_state::AppState;
use log::{error, info, warn};

mod app_state;
mod config;
mod constants;
mod logging;
mod nacos_config;
mod service;
mod startup;
mod trace;

const GRPC_PORT: u16 = 9090;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  // 初始化 Nacos 配置

  nacos_config::entrance::init_nacos().await?;

  logging::log::init_tracing();

  // let config_watcher = config::loader::load_config_from_nacos().await?;
  //
  // let app_state = Arc::new(AppState { config: config_watcher.config.clone() });

  let grpc_server_addr: SocketAddr = format!("[::]:{}", GRPC_PORT).parse()?;
  println!("1 Server listening on {}", grpc_server_addr);

  let dex_query_service = DexQueryService {};
  let dex_router_service = DexRouterService {};
  println!("2 Server listening on {}", grpc_server_addr);

  // let mut receiver = config_watcher.sender.subscribe(); // 创建 receiver
  // tokio::spawn(async move {
  //   loop {
  //     receiver.changed().await.unwrap(); // 等待配置更新
  //     let updated_config = receiver.borrow(); // 获取更新后的配置
  //     tracing::info!("App config updated: {:?}", updated_config);
  //     // 更新 gRPC 配置或其他操作
  //   }
  // });

  Server::builder()
    .accept_http1(true)
    .add_service(router_service_server::RouterServiceServer::with_interceptor(dex_router_service, logging::trace::tracing_interceptor))
    .add_service(query_service_server::QueryServiceServer::with_interceptor(dex_query_service, logging::trace::tracing_interceptor))
    .serve(grpc_server_addr)
    .await?;

  println!("3 Server listening on {}", grpc_server_addr);

  Ok(())
}
