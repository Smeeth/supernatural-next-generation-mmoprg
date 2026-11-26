// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use ash_auth_gateway::{start_auth_gateway, AuthGatewayConfig};
use ash_db::transport::write_pki_directory;
use ash_db::AshStore;
use clap::Parser;
use tracing::info;

#[derive(Debug, Parser)]
#[command(
    name = "ash-auth-gateway",
    about = "Ash-Net QUIC Auth & Character Gateway"
)]
struct Args {
    /// Optionaler Init-Modus für den `pki-init`-Container: generiert Root-CA und mTLS-Zertifikate und beendet sich.
    #[arg(long, env = "INIT_PKI_ONLY")]
    init_pki_only: Option<PathBuf>,

    #[arg(long, env = "AUTH_BIND_ADDR", default_value = "0.0.0.0:4430")]
    bind_addr: SocketAddr,

    #[arg(long, env = "TLS_CERT_PATH", default_value = "/certs/auth-gateway.crt")]
    cert_path: PathBuf,

    #[arg(long, env = "TLS_KEY_PATH", default_value = "/certs/auth-gateway.key")]
    key_path: PathBuf,

    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgres://ash:ash_secret@postgres:5432/ashnet"
    )]
    database_url: String,

    #[arg(long, env = "REDIS_URL", default_value = "redis://redis:6379")]
    redis_url: String,

    #[arg(
        long,
        env = "ROADHOUSE_CLIENT_ADDR",
        default_value = "game-node-roadhouse:4433"
    )]
    roadhouse_client_addr: String,

    #[arg(long, env = "ROUTE_STALE_TIMEOUT_MS", default_value_t = 12_000)]
    route_stale_timeout_ms: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    if let Some(pki_dir) = args.init_pki_only {
        write_pki_directory(&pki_dir)?;
        info!("PKI certificates initialized in {}", pki_dir.display());
        return Ok(());
    }

    let cert_pem = fs::read(&args.cert_path)
        .with_context(|| format!("read cert {}", args.cert_path.display()))?;
    let key_pem = fs::read(&args.key_path)
        .with_context(|| format!("read key {}", args.key_path.display()))?;

    let store = AshStore::connect_postgres_and_redis(&args.database_url, &args.redis_url).await?;
    let _ep = start_auth_gateway(
        AuthGatewayConfig {
            bind_addr: args.bind_addr,
            cert_pem,
            key_pem,
            roadhouse_fallback_client_addr: args.roadhouse_client_addr,
            route_stale_timeout_ms: args.route_stale_timeout_ms,
        },
        store,
    )?;

    tokio::signal::ctrl_c().await?;
    info!("ash-auth-gateway shutting down");
    Ok(())
}
