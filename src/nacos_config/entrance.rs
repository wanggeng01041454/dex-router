use std::{env, sync::Arc};

use anyhow::Result;
use nacos_sdk::api::{
  config::{ConfigChangeListener, ConfigResponse, ConfigServiceBuilder},
  props::ClientProps,
};
use tokio::{sync::RwLock, task};

use super::types::NacosConfig;

lazy_static::lazy_static! {
  static ref nacos_global_config :Arc<RwLock<NacosConfig>> = Arc::new(RwLock::new(NacosConfig::default()));
}

/// 初始化 nacos
pub async fn init_nacos() -> Result<()> {
  // 尝试从环境变量读取 NACOS_SERVER_ADDR
  let nacos_server_addr = match env::var("NACOS_SERVER_ADDR") {
    Ok(addr) => addr,
    Err(_) => "127.0.0.1:8848".to_string(), // 默认值
  };

  let nacos_namespace = match env::var("NACOS_NAMESPACE") {
    Ok(namespace) => namespace,
    Err(_) => "sbu-test-5".to_string(), // 默认值
  };

  let nacos_group = match env::var("NACOS_GROUP") {
    Ok(group) => group,
    Err(_) => "dex-indexer".to_string(), // 默认值
  };

  let nacos_data_id = match env::var("NACOS_DATA_ID") {
    Ok(data_id) => data_id,
    Err(_) => "dex-indexer".to_string(), // 默认值
  };
  let nacos_user = match env::var("NACOS_USER") {
    Ok(user) => user,
    Err(_) => "nacos".to_string(), // 默认值
  };
  let nacos_password = match env::var("NACOS_PASSWORD") {
    Ok(password) => password,
    Err(_) => "nacos".to_string(), // 默认值
  };

  println!("Nacos server address: {}", nacos_server_addr);
  println!("Nacos namespace: {}", nacos_namespace);
  println!("Nacos group: {}", nacos_group);
  println!("Nacos data ID: {}", nacos_data_id);

  let config_service = ConfigServiceBuilder::new(
    ClientProps::new().server_addr(nacos_server_addr).namespace(nacos_namespace).auth_username(nacos_user).auth_password(nacos_password),
  )
  .enable_auth_plugin_http()
  .build()?;

  let config_resp = config_service.get_config(nacos_data_id.clone(), nacos_group.clone()).await?;

  set_global_nacos_config(&config_resp).await?;

  // add a listener
  let _ = config_service.add_listener(nacos_data_id.clone(), nacos_group.clone(), Arc::new(MyConfigChangeListener::default())).await?;

  Ok(())
}

#[derive(Default)]
struct MyConfigChangeListener;

impl ConfigChangeListener for MyConfigChangeListener {
  fn notify(&self, config_resp: ConfigResponse) {
    // 使用 tokio::spawn 启动异步任务
    let config_resp_clone = config_resp.clone();
    task::spawn(async move {
      if let Err(err) = set_global_nacos_config(&config_resp_clone).await {
        eprintln!("Failed to set global nacos config: {:?}", err);
      }
    });
  }
}

/// 设置nacos配置到全局变量中
async fn set_global_nacos_config(config_resp: &ConfigResponse) -> Result<()> {
  let cfg: NacosConfig = serde_json::from_str(config_resp.content())?;
  println!("Nacos config: {}", serde_json::to_string_pretty(&cfg)?);

  //设置到全局变量中
  let mut nacos_config = nacos_global_config.write().await;
  *nacos_config = cfg;
  println!("Nacos config set to global variable");

  Ok(())
}

/// 读取nacos配置
pub async fn get_nacos_config() -> NacosConfig {
  let nacos_config = nacos_global_config.read().await;
  nacos_config.clone()
}

/// 读取nacos配置, 同步
/// 在同步代码中使用，该方式会阻塞当前线程，导致性能降低
pub fn get_nacos_config_slow_sync() -> NacosConfig {
    // 使用 tokio::runtime::Handle 来阻塞运行异步代码
    let handle = tokio::runtime::Handle::current();
    handle.block_on(async {
        let nacos_config = nacos_global_config.read().await;
        nacos_config.clone()
    })
}