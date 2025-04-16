
use nacos_sdk::api::config::{ConfigChangeListener, ConfigResponse};
use nacos_sdk::api::config::{ConfigService, ConfigServiceBuilder};
use nacos_sdk::api::props::ClientProps;
use rand::random;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::env;
use std::sync::{Arc, RwLock};
use tokio::sync::watch;
use tracing::{info, warn};
use crate::config::types::AppConfig;

#[derive(Clone)]
pub struct ConfigWatcher {
  pub config: Arc<RwLock<AppConfig>>,
  pub sender: watch::Sender<AppConfig>,
}
impl ConfigWatcher {
  pub fn new(initial: AppConfig) -> Self {
    let config = Arc::new(RwLock::new(initial.clone()));
    let (sender, _) = watch::channel(initial);
    Self { config, sender }
  }

  pub fn receiver(&self) -> watch::Receiver<AppConfig> {
    self.sender.subscribe()
  }
}

impl ConfigChangeListener for ConfigWatcher {
  fn notify(&self, config_resp: ConfigResponse) {
    let config = self.config.clone();
    let sender = self.sender.clone();

    // 在这里手动 spawn 一个任务去处理异步逻辑
    tokio::spawn(async move {
      match serde_json::from_str::<AppConfig>(config_resp.content()) {
        Ok(new_config) => {
          info!("New config from nacos: {:?}", new_config);
          let mut cfg = config.write().unwrap();
          *cfg = new_config.clone();
          let _ = sender.send(new_config);
        }
        Err(e) => {
          warn!("Failed to parse config from nacos: {}", e);
        }
      }
    });

  }
}
