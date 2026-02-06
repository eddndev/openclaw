use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Default)]
pub struct OpenClawConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<MetaConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<HashMap<String, serde_json::Value>>,
    pub gateway: GatewayConfig,
}

#[derive(Serialize, Deserialize)]
pub struct MetaConfig {
    #[serde(rename = "lastTouchedVersion")]
    pub last_touched_version: String,
}

#[derive(Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<HashMap<String, PluginEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load: Option<PluginLoadConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct PluginLoadConfig {
    pub paths: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PluginEntry {
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Default)]

pub struct GatewayConfig {

    pub mode: String,

    pub port: u16,

    pub bind: String,

    pub auth: GatewayAuthConfig,

}



#[derive(Serialize, Deserialize, Default)]

pub struct GatewayAuthConfig {

    pub mode: String,

    pub token: String,

}
