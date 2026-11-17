use std::{env, net::SocketAddr};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub mongodb_uri: String,
    pub mongodb_database: String,
    pub web_dist_dir: String,
    pub media_bind_addr: SocketAddr,
    pub media_advertise_addr: SocketAddr,
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
            media_bind_addr: env_addr("MEDIA_BIND_ADDR", "0.0.0.0:3478")?,
            media_advertise_addr: env_addr("MEDIA_ADVERTISE_ADDR", "127.0.0.1:3478")?,
        })
    }
}

fn env_addr(key: &str, default: &str) -> anyhow::Result<SocketAddr> {
    let value = env::var(key).unwrap_or_else(|_| default.to_string());
    Ok(value.parse()?)
}
