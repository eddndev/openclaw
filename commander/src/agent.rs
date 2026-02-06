use crate::config::{GatewayAuthConfig, GatewayConfig, OpenClawConfig};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{error, info, warn};

pub async fn spawn_agent(id: &str, ipv6: Option<&str>, port: u16) -> anyhow::Result<()> {
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
    
    if !status.success() {
        error!(agent = %id, status = ?status, "Agent process exited with error");
        anyhow::bail!("Agent {} exited with {}", id, status);
    }

    Ok(())
}
