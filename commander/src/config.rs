use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct OpenClawConfig {
    pub gateway: GatewayConfig,
}

#[derive(Serialize, Deserialize)]
pub struct GatewayConfig {
    pub mode: String,
    pub port: u16,
    pub bind: String,
    pub auth: GatewayAuthConfig,
}

#[derive(Serialize, Deserialize)]
pub struct GatewayAuthConfig {
    pub mode: String,
    pub token: String,
}
