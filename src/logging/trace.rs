use std::any::Any;
use tonic::{Request, Status};
use tracing::{Span, info};

pub fn tracing_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
  println!("Intercepting request: {:?}", req);

  if let Some(trace_id) = req.metadata().get("trace-id").and_then(|v| v.to_str().ok()) {
    // 获取当前 Span
    let current_span = Span::current();

    // 将 trace_id 存储到 Span 的 extensions 中
    current_span.record("trace-id", trace_id);

    // 打印日志以验证 trace-id 是否正确记录
    info!(parent: &current_span, "Trace ID set: {}", trace_id);
  }

  Ok(req)
}
