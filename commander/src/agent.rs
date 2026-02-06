use crate::config::{GatewayAuthConfig, GatewayConfig, OpenClawConfig};
use crate::state::{AgentState, AgentStatus, FleetState};
use std::process::Stdio;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{error, info, warn};

pub async fn spawn_agent(fleet_id: &str, id: &str, ipv6: Option<&str>, port: u16, state: FleetState) -> anyhow::Result<()> {
    // Register starting state
    {
        let mut guard = state.lock().unwrap();
        guard.insert(id.to_string(), AgentState {
            id: id.to_string(),
            fleet_id: fleet_id.to_string(),
            port,
            ipv6: ipv6.map(|s| s.to_string()),
            pid: None,
            status: AgentStatus::Starting,
            uptime: Instant::now(),
        });
    }

    let mut project_root = std::env::current_dir()?;
    let mut script_path = project_root.join("openclaw.mjs");

    if !script_path.exists() {
        if let Some(parent) = project_root.parent() {
            let fallback_path = parent.join("openclaw.mjs");
            if fallback_path.exists() {
                script_path = fallback_path;
                project_root = parent.to_path_buf();
            }
        }
    }

    if !script_path.exists() {
        anyhow::bail!("Could not find openclaw.mjs at {:?}", script_path);
    }

    let agent_home = project_root.join(".fleets").join(id);
    let config_dir = agent_home.join(".openclaw");
    tokio::fs::create_dir_all(&config_dir).await?;

    // Provision config if missing
    let config_path = config_dir.join("openclaw.json");
    if !config_path.exists() {
        info!(agent = %id, "Provisioning new configuration");
        let token = format!("tk_{}", uuid::Uuid::new_v4().simple());
        let config = OpenClawConfig {
            gateway: GatewayConfig {
                mode: "local".to_string(),
                port,
                bind: "loopback".to_string(),
                auth: GatewayAuthConfig {
                    mode: "token".to_string(),
                    token,
                },
            },
        };
        let json = serde_json::to_string_pretty(&config)?;
        tokio::fs::write(&config_path, json).await?;
    }

    info!(agent = %id, home = ?agent_home, port = %port, "Spawning agent process");

    let mut cmd = Command::new("node");
    cmd.arg(&script_path)
        .arg("gateway")
        .arg("run")
        .env("HOME", &agent_home)
        .env("OPENCLAW_GATEWAY_PORT", port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ip) = ipv6 {
        info!(agent = %id, ipv6 = %ip, "Binding to specific IPv6");
        cmd.env("OPENCLAW_BAILEYS_BIND_IP", ip);
    }

    let mut child = cmd.spawn()?;
    let pid = child.id();

    // Register running state
    {
        let mut guard = state.lock().unwrap();
        if let Some(agent) = guard.get_mut(id) {
            agent.pid = pid;
            agent.status = AgentStatus::Running;
        }
    }

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    let id_clone = id.to_string();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            info!(agent = %id_clone, "{}", line);
        }
    });

    let id_clone_err = id.to_string();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            warn!(agent = %id_clone_err, "{}", line);
        }
    });

    let status = child.wait().await?;
    
    // Register stopped state
    {
        let mut guard = state.lock().unwrap();
        if let Some(agent) = guard.get_mut(id) {
            agent.status = if status.success() { AgentStatus::Stopped } else { AgentStatus::Failed };
            agent.pid = None;
        }
    }
    
    if !status.success() {
        error!(agent = %id, status = ?status, "Agent process exited with error");
        anyhow::bail!("Agent {} exited with {}", id, status);
    }

    Ok(())
}
