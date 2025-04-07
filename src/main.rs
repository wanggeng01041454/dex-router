use std::net::SocketAddr;

use json_rpc::{JsonRpcRequest, JsonRpcResponse};
use router::router::LocalIndexRouter;
use swap::router_service_server::RouterServiceServer;
use tonic::transport::Server;
use warp::Filter;

pub mod swap {
  tonic::include_proto!("swap"); // 导入生成的代码
}

mod constants;
mod json_rpc;
mod nacos_config;
mod router;

const GRPC_PORT: u16 = 9980;
const JSON_RPC_PORT: u16 = 9981;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

  // 初始化 Nacos 配置
  nacos_config::entrance::init_nacos().await?;

  let grpc_server_addr: SocketAddr = format!("[::]:{}", GRPC_PORT).parse()?;
  let http_server_addr: SocketAddr = format!("[::]:{}", JSON_RPC_PORT).parse()?;

  // 创建 gRPC 服务器
  let grpc_server = Server::builder().add_service(RouterServiceServer::new(LocalIndexRouter::default())).serve(grpc_server_addr);
  // .serve_with_shutdown(grpc_server_addr, async {});

  // 启动 HTTP 服务以处理 JSON-RPC
  let json_rpc_service = warp::post().and(warp::path("jsonrpc")).and(warp::body::json()).map(|req: JsonRpcRequest| {
    // 处理 JSON-RPC 请求
    // todo, 将逻辑移动到单独的文档
    if req.method == "SayHello" {
      let name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("World");
      let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        result: serde_json::json!({
            "message": format!("Hello, {}!", name)
        }),
        id: req.id,
      };
      warp::reply::json(&response)
    } else {
      // 处理未知方法
      warp::reply::json(&JsonRpcResponse { jsonrpc: "2.0".to_string(), result: serde_json::json!(null), id: req.id })
    }
  });

  // 启动 HTTP 服务器
  let http_server = warp::serve(json_rpc_service).run(http_server_addr);

  // 使用 tokio 的 join! 来并行运行 gRPC 和 HTTP 服务
  let (_, _) = tokio::join!(grpc_server, http_server,);

  Ok(())
}
