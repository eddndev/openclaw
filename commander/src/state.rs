use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Debug)]
pub enum AgentCommand {
    Stop,
    Restart,
    Start,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum AgentStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Restarting,
    Failed,
}

#[derive(Clone, Debug)]
pub struct AgentState {
    pub id: String,
    pub fleet_id: String,
    pub port: u16,
    pub ipv6: Option<String>,
    pub pid: Option<u32>,
    pub status: AgentStatus,
    pub uptime: Instant,
    pub tx: Option<tokio::sync::mpsc::Sender<AgentCommand>>,
}

// Custom serializer to suppress the tx field in JSON output (though serde(skip) handles it, strict typing might need manual impl if we weren't deriving Serialize)
// Since we derive Serialize on the struct with skip, we are good.

// However, we need a way to serialize AgentState without the tx field cleanly if we were manually implementing it,
// but here #[derive(Serialize)] with #[serde(skip)] on tx is sufficient.

impl Serialize for AgentState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AgentState", 7)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("fleet_id", &self.fleet_id)?;
        state.serialize_field("port", &self.port)?;
        state.serialize_field("ipv6", &self.ipv6)?;
        state.serialize_field("pid", &self.pid)?;
        state.serialize_field("status", &self.status)?;
        // We might want to serialize uptime as string or seconds
        state.serialize_field("uptime_secs", &self.uptime.elapsed().as_secs())?;
        state.end()
    }
}

pub type FleetState = Arc<Mutex<HashMap<String, AgentState>>>;

pub fn new_fleet_state() -> FleetState {
    Arc::new(Mutex::new(HashMap::new()))
}
