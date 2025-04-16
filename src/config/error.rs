use thiserror::Error;
use serde::{Serialize, Deserialize};
use tonic::{Status, Code};
use std::fmt::Debug;

/// 用于传输的错误信息结构体（序列化为 JSON）
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: &'static str,
    pub message: String,
}

/// 应用级错误定义
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Invalid input: {field}")]
    InvalidInput {
        field: String,
        reason: String,
    },

    #[error("Database error: {0}")]
    DatabaseError(String),



    #[error("Unknown error")]
    Unknown {
        details: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

/// 转换为 gRPC 的 tonic::Status，序列化为 JSON 并整合 reason 信息
impl From<AppError> for Status {
    fn from(err: AppError) -> Self {
        let (code, message, grpc_code) = match &err {
            AppError::InvalidInput { field, reason } => (
                "E1001",
                format!("Invalid input in field '{}': {}", field, reason),
                Code::InvalidArgument,
            ),
            AppError::DatabaseError(msg) => (
                "E2001",
                format!("Database error: {}", msg),
                Code::Unavailable,
            ),

            AppError::Unknown { details, .. } => (
                "E9999",
                format!("Unknown server error: {}", details),
                Code::Internal,
            ),
        };

        let payload = ErrorPayload { code, message };

        Status::new(grpc_code, serde_json::to_string(&payload).unwrap())
    }
}