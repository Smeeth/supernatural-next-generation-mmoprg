// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use ash_db::AshStore;
use ash_game_node::{start_game_node, GameNodeConfig, NodeRole};
use clap::Parser;
use tracing::info;

#[derive(Debug, Parser)]
#[command(
    name = "ash-game-node",
    about = "Ash-Net Headless ECS Zone & Raid Node"
)]
struct Args {
    #[arg(long, env = "NODE_ID", default_value = "game-node-roadhouse")]
    node_id: String,

    #[arg(long, env = "NODE_ROLE", default_value = "hub")]
    node_role: String,

    #[arg(
        long,
        env = "ZONES",
        value_delimiter = ',',
        default_value = "zone_roadhouse"
    )]
    zones: Vec<String>,

    #[arg(long, env = "CLIENT_BIND_ADDR", default_value = "0.0.0.0:4433")]
    client_bind_addr: SocketAddr,

    #[arg(
        long,
        env = "CLIENT_PUBLIC_ADDR",
        default_value = "game-node-roadhouse:4433"
    )]
    client_public_addr: String,

    #[arg(long, env = "MESH_BIND_ADDR", default_value = "0.0.0.0:5000")]
    mesh_bind_addr: SocketAddr,

    #[arg(
        long,
        env = "MESH_PUBLIC_ADDR",
        default_value = "game-node-roadhouse:5000"
    )]
    mesh_public_addr: String,

    #[arg(long, env = "HUB_ADDR")]
    hub_addr: Option<String>,

    #[arg(long, env = "TICK_RATE_HZ", default_value_t = 25)]
    tick_rate_hz: u32,

    #[arg(long, env = "TLS_CA_PATH", default_value = "/certs/ca.crt")]
    ca_path: PathBuf,

    #[arg(long, env = "TLS_CERT_PATH", default_value = "/certs/node.crt")]
    cert_path: PathBuf,

    #[arg(long, env = "TLS_KEY_PATH", default_value = "/certs/node.key")]
    key_path: PathBuf,

    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgres://ash:ash_secret@postgres:5432/ashnet"
    )]
    database_url: String,

    #[arg(long, env = "REDIS_URL", default_value = "redis://redis:6379")]
    redis_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    let ca_pem =
        fs::read(&args.ca_path).with_context(|| format!("read CA {}", args.ca_path.display()))?;
    let cert_pem = fs::read(&args.cert_path)
        .with_context(|| format!("read cert {}", args.cert_path.display()))?;
    let key_pem = fs::read(&args.key_path)
        .with_context(|| format!("read key {}", args.key_path.display()))?;

    let store = AshStore::connect_postgres_and_redis(&args.database_url, &args.redis_url).await?;
    let _running = start_game_node(
        GameNodeConfig {
            node_id: args.node_id,
            role: NodeRole::parse(&args.node_role),
            zones: args.zones,
            client_bind_addr: args.client_bind_addr,
            client_public_addr: args.client_public_addr,
            mesh_bind_addr: args.mesh_bind_addr,
            mesh_public_addr: args.mesh_public_addr,
            hub_mesh_addr: args.hub_addr,
            tick_rate_hz: args.tick_rate_hz,
            ca_pem,
            cert_pem,
            key_pem,
        },
        store,
    )
    .await?;

    tokio::signal::ctrl_c().await?;
    info!("ash-game-node shutting down");
    Ok(())
}
