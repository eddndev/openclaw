use crate::config::{GatewayAuthConfig, GatewayConfig, OpenClawConfig};
use crate::state::{AgentCommand, AgentState, AgentStatus, FleetState};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use std::os::unix::fs::PermissionsExt;

pub async fn ensure_config(
    agent_id: &str,
    project_root: &Path,
    port: u16,
) -> anyhow::Result<std::path::PathBuf> {
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
        plugin_entries.insert(
            "whatsapp".to_string(),
            crate::config::PluginEntry { enabled: true },
        );
        plugin_entries.insert(
            "google-gemini-cli-auth".to_string(),
            crate::config::PluginEntry { enabled: true },
        );

        // Resolve absolute paths for plugins
        let extensions_dir = project_root.join("extensions");
        let whatsapp_ext = extensions_dir.join("whatsapp");
        let gemini_ext = extensions_dir.join("google-gemini-cli-auth");

        let mut channels = std::collections::HashMap::new();
        channels.insert(
            "whatsapp".to_string(),
            serde_json::json!({
                "dmPolicy": "open",
                "allowFrom": ["*"]
            }),
        );

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

struct AgentSupervisor {
    id: String,
    fleet_id: String,
    ipv6: Option<String>,
    port: u16,
    state: FleetState,
    rx: mpsc::Receiver<AgentCommand>,
}

impl AgentSupervisor {
    async fn run(mut self) {
        let mut restart_count = 0;
        let mut last_restart = Instant::now();

        loop {
            // Check if we should stop before starting
            if let Ok(cmd) = self.rx.try_recv() {
                if matches!(cmd, AgentCommand::Stop) {
                    self.update_status(AgentStatus::Stopped, None);
                    info!(agent = %self.id, "Agent stopped by command");
                    // Wait for start command
                    if !self.wait_for_start().await {
                        break; // Channel closed
                    }
                }
            }

            match self.spawn_and_monitor().await {
                Ok(should_continue) => {
                    if !should_continue {
                        break;
                    }
                }
                Err(e) => {
                    error!(agent = %self.id, error = %e, "Agent process failed");
                }
            }

            // Watchdog logic
            if last_restart.elapsed() > Duration::from_secs(60) {
                restart_count = 0;
            }
            last_restart = Instant::now();
            restart_count += 1;

            let backoff = u64::min(restart_count * 2, 30);
            info!(agent = %self.id, backoff = %backoff, "Restarting agent in {}s...", backoff);

            self.update_status(AgentStatus::Restarting, None);

            // Wait with cancellation via channel
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(backoff)) => {},
                cmd = self.rx.recv() => {
                    match cmd {
                        Some(AgentCommand::Stop) => {
                            self.update_status(AgentStatus::Stopped, None);
                            if !self.wait_for_start().await { break; }
                        }
                        None => break, // Channel closed
                        _ => {} // Ignore other commands while waiting
                    }
                }
            }
        }
    }

    async fn wait_for_start(&mut self) -> bool {
        while let Some(cmd) = self.rx.recv().await {
            if matches!(cmd, AgentCommand::Start | AgentCommand::Restart) {
                return true;
            }
        }
        false
    }

    async fn spawn_and_monitor(&mut self) -> anyhow::Result<bool> {
        let mut project_root = std::env::current_dir()?;
        if !project_root.join("openclaw.mjs").exists() {
            if let Some(parent) = project_root.parent() {
                if parent.join("openclaw.mjs").exists() {
                    project_root = parent.to_path_buf();
                }
            }
        }

        let agent_home = ensure_config(&self.id, &project_root, self.port).await?;
        let script_path = project_root.join("openclaw.mjs");

        let mut cmd = Command::new("node");
        cmd.arg(&script_path)
            .arg("gateway")
            .arg("run")
            .env("HOME", &agent_home)
            .env("OPENCLAW_GATEWAY_PORT", self.port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true); // Critical: kill child if supervisor dies

        if let Some(ref ip) = self.ipv6 {
            cmd.env("OPENCLAW_BAILEYS_BIND_IP", ip);
        }

        info!(agent = %self.id, "Spawning process...");
        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);

        self.update_status(AgentStatus::Running, Some(pid));

        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");

        let id_tag = self.id.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                info!(agent = %id_tag, "{}", line);
            }
        });

        let id_tag_err = self.id.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                warn!(agent = %id_tag_err, "{}", line);
            }
        });

        // Monitor loop
        loop {
            tokio::select! {
                status = child.wait() => {
                    let s = status?;
                    self.update_status(if s.success() { AgentStatus::Stopped } else { AgentStatus::Failed }, None);

                    if s.success() {
                        // Clean exit usually implies we shouldn't auto-restart unless configured otherwise,
                        // but for a fleet, we usually want it running.
                        // However, if the user ran a command that naturally exits, we shouldn't restart.
                        // For now, let's say exit 0 means "I'm done".
                        info!(agent = %self.id, "Process exited successfully");
                        return Ok(false);
                    } else {
                        error!(agent = %self.id, status = ?s, "Process crashed");
                        return Ok(true); // Restart
                    }
                }
                cmd = self.rx.recv() => {
                    match cmd {
                        Some(AgentCommand::Stop) => {
                            info!(agent = %self.id, "Stopping process via signal...");
                            let _ = child.kill().await;
                            self.update_status(AgentStatus::Stopping, Some(pid));
                            let _ = child.wait().await;
                            self.update_status(AgentStatus::Stopped, None);

                            // Wait for start
                            if self.wait_for_start().await {
                                return Ok(true); // Restart loop
                            } else {
                                return Ok(false); // Exit supervisor
                            }
                        }
                        Some(AgentCommand::Restart) => {
                             info!(agent = %self.id, "Restarting process via signal...");
                             let _ = child.kill().await;
                             let _ = child.wait().await;
                             return Ok(true); // Loop back to spawn
                        }
                        Some(AgentCommand::Start) => {
                            // Already running, ignore
                        }
                        None => {
                             // Channel closed, shutdown
                             let _ = child.kill().await;
                             return Ok(false);
                        }
                    }
                }
            }
        }
    }

    fn update_status(&self, status: AgentStatus, pid: Option<u32>) {
        let mut guard = self.state.lock().unwrap();
        if let Some(agent) = guard.get_mut(&self.id) {
            agent.status = status;
            agent.pid = pid;
        }
    }
}

pub async fn spawn_agent(
    fleet_id: &str,
    id: &str,
    ipv6: Option<&str>,
    port: u16,
    state: FleetState,
) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel(32);

    // Initial State Registration
    {
        let mut guard = state.lock().unwrap();
        guard.insert(
            id.to_string(),
            AgentState {
                id: id.to_string(),
                fleet_id: fleet_id.to_string(),
                port,
                ipv6: ipv6.map(|s| s.to_string()),
                pid: None,
                status: AgentStatus::Starting,
                uptime: Instant::now(),
                tx: Some(tx),
            },
        );
    }

    let supervisor = AgentSupervisor {
        id: id.to_string(),
        fleet_id: fleet_id.to_string(),
        ipv6: ipv6.map(|s| s.to_string()),
        port,
        state,
        rx,
    };

    // We spawn the supervisor as a detachable task.
    // However, the original `main.rs` waits on the JoinHandle.
    // To keep compatible with main.rs without rewriting it entirely yet, we can await the supervisor run.
    supervisor.run().await;

    Ok(())
}
