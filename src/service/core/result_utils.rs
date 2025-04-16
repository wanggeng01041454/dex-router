use core::result::Result;

use anyhow;
use tonic::{Response, Status};

/// Converts a Result<T, anyhow::Error> into a Result<Response<T>, Status>.
pub fn convert_result<T>(input: Result<T, anyhow::Error>) -> Result<Response<T>, Status> {
  match input {
    Ok(data) => Ok(Response::new(data)),
    Err(err) => {
      println!("=========================================");
      println!("Error: {}", err);
      // todo: 这里需要根据错误类型进行分类处理
      let status = Status::internal(format!("Internal error: {}", err));
      Err(status)
    }
  }
}
