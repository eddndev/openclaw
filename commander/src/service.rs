use std::path::PathBuf;
use tracing::info;

pub async fn install_service(fleet_id: &str, ipv6_prefix: Option<&str>, base_port: u16, user: Option<&str>) -> anyhow::Result<()> {
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
Environment="COMMANDER_FLEET_ID={fleet_id}"
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
