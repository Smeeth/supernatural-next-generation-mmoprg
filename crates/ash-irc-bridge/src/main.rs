// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use ash_irc_bridge::{spawn_irc_bridge, IrcBridgeConfig};
use clap::Parser;
use tracing::info;

#[derive(Debug, Parser)]
#[command(
    name = "ash-irc-bridge",
    about = "Ash-Net IRC <-> QUIC-Mesh Service Bridge"
)]
struct Args {
    #[arg(long, env = "IRCD_ADDR", default_value = "ircd:6667")]
    irc_addr: String,

    #[arg(long, env = "IRC_BOT_NICK", default_value = "AshBridge")]
    bot_nick: String,

    #[arg(long, env = "IRC_OPER_USER", default_value = "ashoper")]
    oper_user: Option<String>,

    #[arg(
        long,
        env = "IRC_OPER_PASSWORD",
        default_value = "enochian_oper_secret"
    )]
    oper_password: Option<String>,

    #[arg(
        long,
        env = "HUB_MESH_ADDR",
        default_value = "game-node-roadhouse:5000"
    )]
    hub_mesh_addr: String,

    #[arg(long, env = "TLS_CA_PATH", default_value = "/certs/ca.crt")]
    ca_path: PathBuf,

    #[arg(long, env = "TLS_CERT_PATH", default_value = "/certs/irc-bridge.crt")]
    cert_path: PathBuf,

    #[arg(long, env = "TLS_KEY_PATH", default_value = "/certs/irc-bridge.key")]
    key_path: PathBuf,
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

    let _handle = spawn_irc_bridge(IrcBridgeConfig {
        irc_addr: args.irc_addr,
        bot_nick: args.bot_nick,
        oper_user: args.oper_user,
        oper_password: args.oper_password,
        hub_mesh_addr: args.hub_mesh_addr,
        ca_pem,
        cert_pem,
        key_pem,
    })?;

    tokio::signal::ctrl_c().await?;
    info!("ash-irc-bridge shutting down");
    Ok(())
}
