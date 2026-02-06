use crate::config::{GatewayAuthConfig, GatewayConfig, OpenClawConfig};
use crate::state::{AgentState, AgentStatus, FleetState};
use std::path::Path;
use std::process::Stdio;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{error, info, warn};

use std::os::unix::fs::PermissionsExt; // Import for permissions

pub async fn ensure_config(agent_id: &str, project_root: &Path, port: u16) -> anyhow::Result<std::path::PathBuf> {
    let agent_home = project_root.join(".fleets").join(agent_id);
    let config_dir = agent_home.join(".openclaw");
    tokio::fs::create_dir_all(&config_dir).await?;

    // Fix permissions: chmod 700 (rwx------)
    let mut perms = tokio::fs::metadata(&config_dir).await?.permissions();
    perms.set_mode(0o700);
    tokio::fs::set_permissions(&config_dir, perms).await?;

    let config_path = config_dir.join("openclaw.json");
    if !config_path.exists() {
        info!(agent = %agent_id, "Provisioning new configuration");
        let token = format!("tk_{}", uuid::Uuid::new_v4().simple());
        
        let mut plugin_entries = std::collections::HashMap::new();
        plugin_entries.insert("whatsapp".to_string(), crate::config::PluginEntry { enabled: true });
        plugin_entries.insert("google-gemini-cli-auth".to_string(), crate::config::PluginEntry { enabled: true });
        
        let extensions_dir = project_root.join("extensions");
        let whatsapp_ext = extensions_dir.join("whatsapp");
        let gemini_ext = extensions_dir.join("google-gemini-cli-auth");

        let mut channels = std::collections::HashMap::new();
        channels.insert("whatsapp".to_string(), serde_json::json!({
            "dmPolicy": "open",
            "allowFrom": ["*"]
        }));

        let config = OpenClawConfig {
            meta: Some(crate::config::MetaConfig {
                last_touched_version: "2026.2.3".to_string(),
            }),
            session: Some(crate::config::SessionConfig {
                dm_scope: "per-channel-peer".to_string(),
            }),
            plugins: Some(crate::config::PluginsConfig {
                entries: Some(plugin_entries),
                load: Some(crate::config::PluginLoadConfig {
                    paths: vec![
                        whatsapp_ext.to_string_lossy().to_string(),
                        gemini_ext.to_string_lossy().to_string(),
                    ],
                }),
            }),
            channels: Some(channels),
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

    Ok(agent_home)
}

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
    if !project_root.join("openclaw.mjs").exists() {
        if let Some(parent) = project_root.parent() {
            if parent.join("openclaw.mjs").exists() {
                project_root = parent.to_path_buf();
            }
        }
    }

    let agent_home = ensure_config(id, &project_root, port).await?;
    let script_path = project_root.join("openclaw.mjs");

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