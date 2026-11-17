use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub mongodb_uri: String,
    pub mongodb_database: String,
    pub web_dist_dir: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            bind_addr: env::var("APP_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            mongodb_uri: env::var("MONGODB_URI")
                .unwrap_or_else(|_| "mongodb://localhost:27017".to_string()),
            mongodb_database: env::var("MONGODB_DATABASE")
                .unwrap_or_else(|_| "open_comms".to_string()),
            web_dist_dir: env::var("WEB_DIST_DIR")
                .unwrap_or_else(|_| "apps/web/dist".to_string()),
        })
    }
}
