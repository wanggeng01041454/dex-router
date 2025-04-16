use std::sync::{Arc, RwLock};


#[derive(Clone,Debug,Default)]
pub struct AppState {
    pub config: Arc<RwLock<crate::config::types::AppConfig>>,
}

