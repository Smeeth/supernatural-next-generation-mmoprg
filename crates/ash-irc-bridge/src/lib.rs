// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use ash_core::world::IrcSlashCommand;
use ash_protocol::framing::rpc_call;
use ash_protocol::messages::{MeshMessage, MeshResponse};
use ash_protocol::tls::build_quic_client;
use quinn::{Connection, Endpoint};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

pub const STANDARD_CHANNELS: [&str; 4] = [
    "#roadhouse",
    "#new_lawrence",
    "#zone_salzminen",
    "#men_of_letters",
];

#[derive(Clone)]
pub struct IrcBridgeConfig {
    pub irc_addr: String,
    pub bot_nick: String,
    pub oper_user: Option<String>,
    pub oper_password: Option<String>,
    pub hub_mesh_addr: String,
    pub ca_pem: Vec<u8>,
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

async fn resolve_addr(addr: &str) -> Result<SocketAddr> {
    tokio::net::lookup_host(addr)
        .await
        .with_context(|| format!("resolve '{addr}'"))?
        .next()
        .with_context(|| format!("no addr for '{addr}'"))
}

fn default_zone_for_channel(channel: &str) -> &'static str {
    match channel {
        "#roadhouse" => "zone_roadhouse",
        "#zone_salzminen" => "zone_salzminen",
        "#men_of_letters" => "zone_relais_kathedrale",
        _ => "zone_new_lawrence_west",
    }
}

pub fn spawn_irc_bridge(cfg: IrcBridgeConfig) -> Result<JoinHandle<()>> {
    let mesh_client = build_quic_client(
        "0.0.0.0:0".parse().unwrap(),
        &cfg.ca_pem,
        Some((&cfg.cert_pem, &cfg.key_pem)),
    )?;

    let handle = tokio::spawn(async move {
        loop {
            if let Err(err) = run_bridge_session(&cfg, &mesh_client).await {
                warn!("irc-bridge session interrupted ({err:#}); reconnecting in 2s...");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    });
    Ok(handle)
}

async fn run_bridge_session(cfg: &IrcBridgeConfig, mesh_client: &Endpoint) -> Result<()> {
    let stream = TcpStream::connect(&cfg.irc_addr)
        .await
        .with_context(|| format!("connect to IRCd at {}", cfg.irc_addr))?;
    let (read_half, mut write_half) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Writer task
    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            let formatted = format!("{line}\r\n");
            if write_half.write_all(formatted.as_bytes()).await.is_err() {
                break;
            }
            let _ = write_half.flush().await;
        }
    });

    tx.send(format!("NICK {}", cfg.bot_nick))?;
    tx.send(format!(
        "USER {} 0 * :Ash-Net Enochian Relay Service",
        cfg.bot_nick.to_lowercase()
    ))?;

    if let (Some(ou), Some(op)) = (&cfg.oper_user, &cfg.oper_password) {
        tx.send(format!("OPER {ou} {op}"))?;
    }
    for ch in STANDARD_CHANNELS {
        tx.send(format!("JOIN {ch}"))?;
    }

    info!(
        "ash-irc-bridge connected to IRCd {} and joined {:?}",
        cfg.irc_addr, STANDARD_CHANNELS
    );

    // Background Omen Alert Poller -> hält eine persistente mTLS-QUIC-Verbindung zum Hub
    {
        let alert_tx = tx.clone();
        let hub_addr = cfg.hub_mesh_addr.clone();
        let mc = mesh_client.clone();
        tokio::spawn(async move {
            let mut last_seq = 0u64;
            let mut cached_conn: Option<Connection> = None;
            let mut interval = tokio::time::interval(Duration::from_millis(400));
            loop {
                interval.tick().await;
                if cached_conn
                    .as_ref()
                    .is_none_or(|c| c.close_reason().is_some())
                {
                    cached_conn = None;
                    if let Ok(sock) = resolve_addr(&hub_addr).await {
                        if let Ok(connecting) = mc.connect(sock, "ash-node") {
                            if let Ok(conn) = connecting.await {
                                cached_conn = Some(conn);
                            }
                        }
                    }
                }
                let Some(conn) = cached_conn.as_ref() else {
                    continue;
                };
                match rpc_call::<MeshMessage, MeshResponse>(
                    conn,
                    &MeshMessage::PollOmenAlerts { since_seq: last_seq },
                )
                .await
                {
                    Ok(MeshResponse::OmenAlerts { alerts }) => {
                        for ev in alerts {
                            if ev.seq > last_seq {
                                last_seq = ev.seq;
                                if alert_tx
                                    .send(format!("PRIVMSG #new_lawrence :{}", ev.irc_alert_line))
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        }
                    }
                    Err(_) => {
                        cached_conn = None;
                    }
                    _ => {}
                }
            }
        });
    }

    let mut lines = BufReader::new(read_half).lines();
    while let Some(line) = lines.next_line().await? {
        if let Some(rest) = line.strip_prefix("PING ") {
            let _ = tx.send(format!("PONG {rest}"));
            continue;
        }

        // Parse `:Nick!user@host PRIVMSG #channel :/salzen ...`
        if let Some((nick, channel, text)) = parse_irc_privmsg(&line) {
            if nick.eq_ignore_ascii_case(&cfg.bot_nick) {
                continue;
            }
            if let Some(cmd) = IrcSlashCommand::parse_chat_line(text) {
                let target_zone = extract_zone_override(text)
                    .unwrap_or_else(|| default_zone_for_channel(channel).to_string());

                match dispatch_slash_to_mesh(
                    mesh_client,
                    &cfg.hub_mesh_addr,
                    &nick,
                    channel,
                    &target_zone,
                    cmd,
                )
                .await
                {
                    Ok((reply, quest_note)) => {
                        let _ = tx.send(format!("PRIVMSG {channel} :{reply}"));
                        if let Some(q) = quest_note {
                            let _ = tx.send(format!("PRIVMSG {channel} :[QUEST] {q}"));
                        }
                    }
                    Err(e) => {
                        warn!("slash dispatch failed: {e:#}");
                    }
                }
            }
        }
    }

    Ok(())
}

fn parse_irc_privmsg(raw: &str) -> Option<(String, &str, &str)> {
    let without_colon = raw.strip_prefix(':')?;
    let (prefix, rest) = without_colon.split_once(' ')?;
    let nick = prefix.split('!').next()?.to_string();
    let rest = rest.strip_prefix("PRIVMSG ")?;
    let (target, msg_part) = rest.split_once(" :")?;
    Some((nick, target.trim(), msg_part.trim()))
}

fn extract_zone_override(text: &str) -> Option<String> {
    text.split_whitespace()
        .skip(1)
        .find(|tok| tok.starts_with("zone_"))
        .map(|s| s.to_string())
}

async fn dispatch_slash_to_mesh(
    mesh_client: &Endpoint,
    hub_mesh_addr: &str,
    nick: &str,
    channel: &str,
    zone_id: &str,
    command: IrcSlashCommand,
) -> Result<(String, Option<String>)> {
    let hub_sock = resolve_addr(hub_mesh_addr).await?;
    let hub_conn = mesh_client.connect(hub_sock, "ash-node")?.await?;

    // Ermittle den zuständigen Zonen-Knoten dynamisch über die Gossip-Routing-Tabelle
    let mut target_mesh_addr = hub_mesh_addr.to_string();
    if let Ok(MeshResponse::RoutingTable { routes, .. }) =
        rpc_call::<MeshMessage, MeshResponse>(&hub_conn, &MeshMessage::QueryRoutingTable).await
    {
        if let Some(r) = routes.into_iter().find(|r| r.zone_id == zone_id) {
            target_mesh_addr = r.mesh_addr;
        }
    }

    let (node_conn, extra_conn_to_close) = if target_mesh_addr == hub_mesh_addr {
        (hub_conn.clone(), None)
    } else {
        let sock = resolve_addr(&target_mesh_addr).await?;
        let c = mesh_client.connect(sock, "ash-node")?.await?;
        (c.clone(), Some(c))
    };

    let res = rpc_call::<MeshMessage, MeshResponse>(
        &node_conn,
        &MeshMessage::IrcSlashEvent {
            irc_nick: nick.to_string(),
            character_name: nick.to_string(),
            channel: channel.to_string(),
            zone_id: zone_id.to_string(),
            command,
        },
    )
    .await;

    if let Some(c) = extra_conn_to_close {
        c.close(0u32.into(), b"slash_done");
    }
    hub_conn.close(0u32.into(), b"slash_done");

    match res? {
        MeshResponse::IrcSlashHandled {
            reply_message,
            quest_updated,
            ..
        } => Ok((reply_message, quest_updated)),
        MeshResponse::Error { message } => Ok((format!("[FEHLER] {message}"), None)),
        _ => Ok(("[ASH-NET] Event verarbeitet".into(), None)),
    }
}
