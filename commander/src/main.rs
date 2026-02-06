use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "commander")]
#[command(about = "OpenClaw Fleet Orchestrator", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a single agent manually (for testing)
    RunAgent {
        /// Unique Agent ID
        #[arg(long)]
        id: String,

        /// IPv6 Address to bind (optional, overrides env)
        #[arg(long, env = "OPENCLAW_BIND_IP")]
        ipv6: Option<String>,

        /// Base port for this agent
        #[arg(long, default_value_t = 20000)]
        port: u16,
    },
    /// Start the fleet based on environment variables
    StartFleet {
        /// Number of agents to spawn
        #[arg(long, default_value_t = 1)]
        count: u32,
    },
    /// Install the commander as a systemd service (requires sudo)
    Install {
        /// The Fleet ID for this server
        #[arg(long, default_value = "fleet-default")]
        fleet_id: String,

        /// The IPv6 Prefix for this server (e.g. 2001:db8::)
        #[arg(long)]
        ipv6_prefix: Option<String>,

        /// Base port for the fleet (default: 20000)
        #[arg(long, default_value_t = 20000)]
        base_port: u16,

        /// User to run the service as (defaults to current user)
        #[arg(long)]
        user: Option<String>,
    },
}

#[derive(Serialize, Deserialize)]
struct OpenClawConfig {
    gateway: GatewayConfig,
}

#[derive(Serialize, Deserialize)]
struct GatewayConfig {
    mode: String,
    port: u16,
    bind: String,
    auth: GatewayAuthConfig,
}

#[derive(Serialize, Deserialize)]
struct GatewayAuthConfig {
    mode: String,
    token: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match &cli.command {
        Commands::RunAgent { id, ipv6, port } => {
            spawn_agent(id, ipv6.as_deref(), *port).await?;
        }
        Commands::StartFleet { count } => {
            info!("Starting fleet with {} agents...", count);
            let mut handles = vec![];
            
            let fleet_id = std::env::var("COMMANDER_FLEET_ID").unwrap_or_else(|_| "fleet-local".into());
            let ipv6_prefix = std::env::var("COMMANDER_IPV6_PREFIX").ok();
            
            let base_port = std::env::var("COMMANDER_BASE_PORT")
                .ok()
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(20000);

            for i in 0..*count {
                let agent_id = format!("{}-", fleet_id, i);
                let agent_port = base_port + (i as u16 * 100);
                
                // Calculate IPv6 if prefix is available
                let ipv6 = if let Some(ref prefix) = ipv6_prefix {
                    match calculate_ipv6(prefix, i) {
                        Ok(ip) => Some(ip),
                        Err(e) => {
                            error!(agent_id = %agent_id, error = %e, "Failed to calculate IPv6");
                            None
                        }
                    }
                } else {
                    None
                };

                let agent_id_clone = agent_id.clone();
                handles.push(tokio::spawn(async move {
                    if let Err(e) = spawn_agent(&agent_id_clone, ipv6.as_deref(), agent_port).await {
                        error!(agent_id = %agent_id_clone, error = %e, "Agent failed to start");
                    }
                }));
            }

            // Wait for all agents (supervision would happen here)
            for h in handles {
                let _ = h.await;
            }
        }
        Commands::Install { fleet_id, ipv6_prefix, base_port, user } => {
            install_service(fleet_id, ipv6_prefix.as_deref(), *base_port, user.as_deref()).await?;
        }
    }

    Ok(())
}

/// Calculate a unique IPv6 address by adding an offset to a base prefix
fn calculate_ipv6(prefix: &str, index: u32) -> anyhow::Result<String> {
    let base_addr: Ipv6Addr = prefix.parse() 
        .map_err(|_| anyhow::anyhow!("Invalid IPv6 prefix: {}", prefix))?;
    
    // Convert to u128 for bitwise math
    let base_u128 = u128::from(base_addr);
    
    // Add the index to the address
    let new_u128 = base_u128.checked_add(index as u128) 
        .ok_or_else(|| anyhow::anyhow!("IPv6 address overflow for index {}", index))?;
    
    Ok(Ipv6Addr::from(new_u128).to_string())
}

async fn install_service(fleet_id: &str, ipv6_prefix: Option<&str>, base_port: u16, user: Option<&str>) -> anyhow::Result<()> {
    let current_exe = std::env::current_exe()?;
    let current_dir = std::env::current_dir()?;
    
    // Locate openclaw.mjs relative to repo root
    let repo_root = if current_dir.join("openclaw.mjs").exists() {
        current_dir.clone()
    } else if current_dir.parent().map(|p| p.join("openclaw.mjs").exists()).unwrap_or(false) {
        current_dir.parent().unwrap().to_path_buf()
    } else {
        anyhow::bail!("Could not locate openclaw.mjs. Run install from the project root.");
    };

    info!("Installing OpenClaw Commander...");
    info!("Binary source: {:?}", current_exe);
    info!("Repo root (WorkingDir): {:?}", repo_root);

    // 1. Copy binary to /usr/local/bin
    let target_bin = PathBuf::from("/usr/local/bin/openclaw-commander");
    info!("Copying binary to {:?}", target_bin);
    tokio::fs::copy(&current_exe, &target_bin).await.map_err(|e| {
        anyhow::anyhow!("Failed to copy binary to /usr/local/bin. Are you running with sudo? Error: {}", e)
    })?;

    // 2. Resolve User
    let run_as_user = if let Some(u) = user {
        u.to_string()
    } else {
        std::env::var("USER").unwrap_or_else(|_| "root".to_string())
    };

    // 3. Generate Systemd Unit
    let ipv6_env = if let Some(prefix) = ipv6_prefix {
        format!("Environment=\"COMMANDER_IPV6_PREFIX={}\"\n", prefix)
    } else {
        String::new()
    };

    let service_content = format!(
        r#"[Unit]
Description=OpenClaw Fleet Commander ({fleet_id})
After=network.target

[Service]
Type=simple
User={user}
WorkingDirectory={workdir}
ExecStart={bin} start-fleet --count 1
Restart=always
RestartSec=5
Environment=\"COMMANDER_FLEET_ID={fleet_id}\" 
Environment="COMMANDER_BASE_PORT={base_port}"
{ipv6_env}
Environment="NODE_ENV=production"

[Install]
WantedBy=multi-user.target
"#,
        user = run_as_user,
        workdir = repo_root.display(),
        bin = target_bin.display(),
        fleet_id = fleet_id,
        base_port = base_port,
        ipv6_env = ipv6_env
    );

    let service_filename = format!("openclaw-commander-{}.service", fleet_id);
    let service_path = PathBuf::from("/etc/systemd/system").join(&service_filename);
    
    info!("Writing systemd unit to {:?}", service_path);
    tokio::fs::write(&service_path, service_content).await.map_err(|e| {
        anyhow::anyhow!("Failed to write systemd service file. Are you running with sudo? Error: {}", e)
    })?;

    info!("Installation complete!");
    println!("\nTo enable and start the service, run:");
    println!("  sudo systemctl daemon-reload");
    println!("  sudo systemctl enable --now {}", service_filename);
    println!("\nTo view logs:");
    println!("  journalctl -u {} -f", service_filename, service_filename);

    Ok(())
}

async fn spawn_agent(id: &str, ipv6: Option<&str>, port: u16) -> anyhow::Result<()> {
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