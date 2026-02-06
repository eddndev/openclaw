mod agent;
mod config;
mod service;
mod state;
mod utils;

use dotenvy::dotenv;
use std::sync::Arc;
use tokio::net::TcpListener;

use clap::{Parser, Subcommand};
use tracing::{error, info};
use axum::{routing::get, Router, Json, extract::State};
use tower_http::trace::TraceLayer;
use std::net::SocketAddr;

use crate::agent::spawn_agent;
use crate::service::install_service;
use crate::utils::calculate_ipv6;
use crate::state::{new_fleet_state, FleetState, AgentState};

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let state = new_fleet_state();

    match &cli.command {
        Commands::RunAgent { id, ipv6, port } => {
            // For single agent run, we use a generic fleet id
            spawn_agent("single-run", id, ipv6.as_deref(), *port, state.clone()).await?;
        }
        Commands::StartFleet { count } => {
            info!("Starting fleet with {} agents...", count);
            
            let fleet_id = std::env::var("COMMANDER_FLEET_ID").unwrap_or_else(|_| "fleet-local".into());
            let ipv6_prefix = std::env::var("COMMANDER_IPV6_PREFIX").ok();
            
            let base_port = std::env::var("COMMANDER_BASE_PORT")
                .ok()
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(20000);

            // Start API Server
            let api_port = base_port - 1;
            let state_for_server = state.clone();
            tokio::spawn(async move {
                if let Err(e) = start_api_server(api_port, state_for_server).await {
                    error!("API server failed: {}", e);
                }
            });

            let mut handles = vec![];

            for i in 0..*count {
                let agent_id = format!("{}-{}", fleet_id, i);
                let agent_port = base_port + (i as u16 * 100);
                
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
                let fleet_id_clone = fleet_id.clone();
                let state_clone = state.clone();
                
                handles.push(tokio::spawn(async move {
                    if let Err(e) = spawn_agent(&fleet_id_clone, &agent_id_clone, ipv6.as_deref(), agent_port, state_clone).await {
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

async fn start_api_server(port: u16, state: FleetState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/status", get(get_status))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Commander API listening on http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn get_status(State(state): State<FleetState>) -> Json<Vec<AgentState>> {
    let mut agents: Vec<AgentState> = state.lock().unwrap().values().cloned().collect();
    agents.sort_by(|a, b| a.id.cmp(&b.id));
    Json(agents)
}