use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
pub enum AgentStatus {
    Starting,
    Running,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentState {
    pub id: String,
    pub fleet_id: String,
    pub port: u16,
    pub ipv6: Option<String>,
    pub pid: Option<u32>,
    pub status: AgentStatus,
    #[serde(skip)]
    pub uptime: Instant,
}

pub type FleetState = Arc<Mutex<HashMap<String, AgentState>>>;

pub fn new_fleet_state() -> FleetState {
    Arc::new(Mutex::new(HashMap::new()))
}
