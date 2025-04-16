use serde::Serialize;
use std::fmt::Write;
use tracing::{Event, Subscriber};
use tracing_appender::rolling;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{EnvFilter, Layer, fmt};
use tracing_subscriber::{
  fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
  layer::SubscriberExt,
  util::SubscriberInitExt,
};

/// 初始化 tracing 日志系统
pub fn init_tracing() {
  // 控制台日志（彩色）
  let stdout_log = fmt::layer().pretty().with_target(false).with_level(true).event_format(CustomJsonFormatter).with_writer(std::io::stdout);

  // 文件日志（INFO）
  let info_log = rolling::daily("logs", "info.log");
  let info_layer = fmt::layer()
    .with_ansi(false)
    .with_writer(info_log)
    .event_format(CustomJsonFormatter)
    .with_filter(tracing_subscriber::filter::LevelFilter::INFO);

  // 文件日志（ERROR）
  let error_log = rolling::daily("logs", "error.log");
  let error_layer = fmt::layer()
    .with_ansi(false)
    .with_writer(error_log)
    .event_format(CustomJsonFormatter)
    .with_filter(tracing_subscriber::filter::LevelFilter::ERROR);

  // 环境变量控制日志级别（默认 INFO）
  let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

  // 注册所有 Layer
  tracing_subscriber::registry().with(filter).with(stdout_log).with(info_layer).with(error_layer).init();
}

struct CustomJsonFormatter;
//
// impl<S, N> FormatEvent<S, N> for CustomJsonFormatter
// where
//     S: Subscriber + for<'a> LookupSpan<'a>,
//     N: for<'writer> FormatFields<'writer> + 'static,
// {
//     fn format_event(
//         &self,
//         ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
//         writer: &mut Writer<'_>,
//         event: &Event<'_>,
//     ) -> std::fmt::Result {
//         let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3f%z").to_string(); // T 字段
//         let mut visitor = tracing_subscriber::fmt::format::JsonFields::default();
//         let mut log_buf = String::new();
//
//         // 提取 level
//         let level = *event.metadata().level();
//         let level_str = match level {
//             Level::ERROR => "ERROR",
//             Level::WARN => "WARN",
//             Level::INFO => "INFO",
//             Level::DEBUG => "DEBUG",
//             Level::TRACE => "TRACE",
//         };
//
//         // 获取线程名
//         let thread = std::thread::current();
//         let thread_name = thread.name().unwrap_or("unknown").to_string();
//
//         // 获取代码位置
//         let loc = event.metadata();
//         let code_pos = format!(
//             "{}:{}",
//             loc.file().unwrap_or("unknown"),
//             loc.line().unwrap_or(0)
//         );
//
//         // trace_id 从当前 span 的 extensions 中取（可选）
//         let trace_id = ctx
//             .lookup_current()
//             .and_then(|span| span.extensions().get::<String>().cloned())
//             .unwrap_or_else(|| "TID:N/A".to_string());
//
//         // 收集 event 字段（message）
//         event.record(&mut visitor);
//         // 提取 event 内容（如 message）
//         let mut message_buf = String::new();
//         event.record(&mut tracing_subscriber::fmt::format::FmtRecorder::new(&mut message_buf));
//
//         // 构建 LogEntry 结构体
//         let log_entry = LogEntry {
//             timestamp,
//             level: level_str.to_string(),
//             trace_id,
//             thread_name,
//             code_pos,
//             message: message_buf.trim().to_string(),
//         };
//
//         // 序列化为 JSON 字符串
//         let json_str = serde_json::to_string(&log_entry).unwrap();
//         writeln!(writer, "{}", json_str)
//     }
// }
impl<S, N> FormatEvent<S, N> for CustomJsonFormatter
where
  S: Subscriber + for<'a> LookupSpan<'a>,
  N: for<'writer> FormatFields<'writer> + 'static,
{
  fn format_event(&self, ctx: &FmtContext<'_, S, N>, mut writer: Writer<'_>, event: &Event<'_>) -> std::fmt::Result {
    use std::fmt;
    use tracing::field::Visit;

    let now = chrono::Utc::now().to_rfc3339();
    let meta = event.metadata();

    let trace_id = event
      .parent()
      .and_then(|id| ctx.span(id))
      .and_then(|span| span.extensions().get::<String>().cloned())
      .unwrap_or_else(|| "TID:N/A".to_string());

    // 字段字符串缓冲区
    let mut field_str = String::new();

    struct Visitor<'a> {
      out: &'a mut String,
      first: bool,
    }

    impl<'a> Visit for Visitor<'a> {
      fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.out, "{}\"{}\":{:?}", if self.first { "" } else { "," }, field.name(), value);
        self.first = false;
      }
    }

    let mut visitor = Visitor { out: &mut field_str, first: true };
    event.record(&mut visitor);

    // 写入完整 JSON
    write!(writer, r#"{{"T":"{}","L":"{}","trace":"{}","fields":{{{}}}}}"#, now, meta.level(), trace_id, field_str)
  }
}
#[derive(Serialize)]
struct LogEntry {
  timestamp: String,
  level: String,
  trace_id: String,
  thread_name: String,
  code_pos: String,
  message: String,
}
