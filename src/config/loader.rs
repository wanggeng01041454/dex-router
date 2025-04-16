use crate::config::types::AppConfig;
use crate::config::watcher::ConfigWatcher;
use nacos_sdk::api::config::{ConfigChangeListener, ConfigResponse,ConfigService, ConfigServiceBuilder};
use nacos_sdk::api::props::ClientProps;
use rand::random;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::env;
use std::error::Error;
use std::sync::{Arc, RwLock};
use tokio::sync::watch;
use tracing::{info, warn};

pub async fn load_config_from_nacos() -> Result<ConfigWatcher, Box<dyn Error>> {
  let server_addr = env::var("NACOS_SERVER_ADDR").unwrap_or_else(|_| "localhost:8848".into());
  let namespace = env::var("NACOS_NAMESPACE").unwrap_or_default();
  let group = env::var("NACOS_GROUP").unwrap_or_else(|_| "DEFAULT_GROUP".into());
  let data_id = env::var("NACOS_DATA_ID").unwrap_or_else(|_| "my-config".into());
  let username = env::var("NACOS_USER").unwrap_or_else(|_| "nacos".into());
  let password = env::var("NACOS_PASSWORD").unwrap_or_else(|_| "nacos".into());

  let props =
    ClientProps::new().
        server_addr(server_addr.clone())
        .namespace(namespace.clone())
        .auth_username(username)
        .auth_password(password);

  let config_service = ConfigServiceBuilder::new(props)
      .enable_auth_plugin_http()
      .build()?;

  let config_resp = config_service
      .get_config(data_id.clone(), group.clone()).await?;

  let config: AppConfig = serde_json::from_str(config_resp.content())?;

  info!("Loaded initial config: {:?}", config);

  let watcher = ConfigWatcher::new(config.clone());

  config_service.add_listener(data_id, group, Arc::new(watcher.clone())).await?;

  Ok(watcher)
}
