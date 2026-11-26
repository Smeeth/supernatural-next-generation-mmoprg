// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::io::BufReader as StdBufReader;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use ash_core::character::STARTING_ZONE_ROADHOUSE;
use ash_core::equipment::{EquipmentSlot, EssenceType, ItemQuality};
use ash_protocol::client::{
    AuthRequest, AuthResponse, ClientDatagram, ClientReliableMessage, CombatActionNet,
    ServerDatagram, ServerReliableMessage,
};
use ash_protocol::codec::{
    decode_datagram, decode_frame_payload, encode_datagram, encode_frame_payload, ASH_ALPN,
    MAX_FRAME_SIZE,
};
use clap::Parser;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Connection, Endpoint, TransportConfig};
use rustls::RootCertStore;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "ash-test-cli",
    about = "Headless Blackbox QUIC + IRC End-to-End Smoke-Test Client for Ash-Net: New Lawrence"
)]
struct Args {
    #[arg(long, env = "AUTH_ADDR", default_value = "127.0.0.1:4430")]
    auth_addr: String,

    #[arg(long, env = "IRC_ADDR", default_value = "127.0.0.1:6667")]
    irc_addr: String,

    #[arg(long, env = "TLS_CA_PATH", default_value = "certs/ca.crt")]
    ca_path: PathBuf,
}

/// Erstellt einen reinen Client-QUIC-Endpoint (verifiziert ausschließlich gegen öffentliches `ca.crt`).
fn build_public_quic_client(bind_addr: SocketAddr, ca_pem: &[u8]) -> Result<Endpoint> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut reader = StdBufReader::new(ca_pem);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse CA PEM")?;

    let mut roots = RootCertStore::empty();
    for cert in certs {
        roots.add(cert).context("add CA cert to root store")?;
    }

    let mut tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls_config.alpn_protocols = vec![ASH_ALPN.to_vec()];

    let quic_tls = QuicClientConfig::try_from(tls_config).context("build QuicClientConfig")?;
    let mut client_config = ClientConfig::new(Arc::new(quic_tls));
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(4)));
    transport.max_idle_timeout(Some(Duration::from_secs(30).try_into().unwrap()));
    transport.datagram_receive_buffer_size(Some(512 * 1024));
    transport.datagram_send_buffer_size(512 * 1024);
    client_config.transport_config(Arc::new(transport));

    let mut endpoint = Endpoint::client(bind_addr).context("bind QUIC client endpoint")?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

async fn rpc_call<Req: Serialize, Resp: DeserializeOwned>(
    conn: &Connection,
    req: &Req,
) -> Result<Resp> {
    let (mut send, mut recv) = conn.open_bi().await.context("open QUIC bi-stream")?;
    let payload = encode_frame_payload(req)?;
    let len_bytes = (payload.len() as u32).to_be_bytes();
    send.write_all(&len_bytes).await?;
    send.write_all(&payload).await?;
    send.finish()?;

    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_SIZE {
        bail!("incoming QUIC frame exceeds max size ({len} bytes)");
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    decode_frame_payload(&buf)
}

fn parse_localhost_fallback_port(raw: &str) -> Result<SocketAddr> {
    if let Some((_, port_str)) = raw.rsplit_once(':') {
        let fallback = format!("127.0.0.1:{port_str}");
        return fallback
            .parse::<SocketAddr>()
            .with_context(|| format!("parse localhost fallback '{fallback}'"));
    }
    bail!("cannot resolve target endpoint '{raw}'")
}

async fn resolve_endpoint_with_localhost_fallback(raw: &str) -> Result<SocketAddr> {
    if let Ok(mut iter) = tokio::net::lookup_host(raw).await {
        if let Some(addr) = iter.next() {
            return Ok(addr);
        }
    }
    parse_localhost_fallback_port(raw)
}

pub async fn run_smoke_test_suite(auth_addr: &str, irc_addr: &str, ca_pem: &[u8]) -> Result<()> {
    let client_ep: Endpoint = build_public_quic_client("0.0.0.0:0".parse()?, ca_pem)?;
    let auth_sock = resolve_endpoint_with_localhost_fallback(auth_addr).await?;
    let auth_conn = client_ep
        .connect(auth_sock, "ash-node")?
        .await
        .context("connect to auth-gateway over QUIC")?;

    let suffix = Uuid::new_v4().simple().to_string()[..6].to_string();
    let username = format!("dean_{suffix}");
    let hunter_name = format!("Winchester_{suffix}");

    // 1. Account-Registrierung
    info!("[1/8] Registriere Account '{username}' via QUIC an auth-gateway...");
    let session_token = match rpc_call::<AuthRequest, AuthResponse>(
        &auth_conn,
        &AuthRequest::RegisterAccount {
            username: username.clone(),
            password: "impala_1967_password".into(),
        },
    )
    .await?
    {
        AuthResponse::AccountRegistered { session_token, .. } => session_token,
        other => bail!("unexpected RegisterAccount response: {other:?}"),
    };

    // 2. Verifiziere striktes Verbot von Engeln & Dämonen am Auth-Gateway
    info!("[2/8] Prüfe strikte Ablehnung von Engeln & Dämonen...");
    for forbidden in ["Engel", "Dämon"] {
        match rpc_call::<AuthRequest, AuthResponse>(
            &auth_conn,
            &AuthRequest::CreateCharacter {
                session_token: session_token.clone(),
                name: format!("Bad_{forbidden}"),
                race: forbidden.into(),
                class: "Winchester-Sniper".into(),
            },
        )
        .await?
        {
            AuthResponse::Error { code, .. } if code == "FORBIDDEN_OVERPOWER_RACE" => {}
            other => bail!("Engel/Dämon wurde NICHT korrekt abgewiesen: {other:?}"),
        }
    }

    // 3. Gültigen Jäger erstellen (16 graue Items in zone_roadhouse)
    info!("[3/8] Erstelle Jäger '{hunter_name}' (Winchester-Abkömmling / Winchester-Sniper)...");
    let hunter = match rpc_call::<AuthRequest, AuthResponse>(
        &auth_conn,
        &AuthRequest::CreateCharacter {
            session_token: session_token.clone(),
            name: hunter_name.clone(),
            race: "Winchester-Abkömmling".into(),
            class: "Winchester-Sniper".into(),
        },
    )
    .await?
    {
        AuthResponse::CharacterCreated { character } => character,
        other => bail!("CharacterCreated failed: {other:?}"),
    };

    if hunter.current_zone != STARTING_ZONE_ROADHOUSE {
        bail!(
            "Startzone ist nicht zone_roadhouse: {}",
            hunter.current_zone
        );
    }
    if hunter.equipment.slots.len() != 16 {
        bail!(
            "Erwartete 16 Ausrüstungs-Slots, erhalten: {}",
            hunter.equipment.slots.len()
        );
    }
    for slot in EquipmentSlot::ALL {
        let item = hunter.equipment.get(slot).context("missing slot")?;
        if item.quality != ItemQuality::Grey {
            bail!("Start-Item in Slot {:?} ist nicht Grau!", slot);
        }
    }

    // 4. Login-Routing-Ticket abrufen & Harvelle's Roadhouse betreten
    info!("[4/8] Betrete Harvelle's Roadhouse (game-node-roadhouse) + prüfe QUIC-Datagramme...");
    let ticket = match rpc_call::<AuthRequest, AuthResponse>(
        &auth_conn,
        &AuthRequest::IssueZoneTicket {
            session_token: session_token.clone(),
            character_id: hunter.id,
        },
    )
    .await?
    {
        AuthResponse::ZoneTicketIssued { ticket } => ticket,
        other => bail!("IssueZoneTicket failed: {other:?}"),
    };

    let roadhouse_sock =
        resolve_endpoint_with_localhost_fallback(&ticket.target_client_addr).await?;
    let roadhouse_conn = client_ep.connect(roadhouse_sock, "ash-node")?.await?;

    match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &roadhouse_conn,
        &ClientReliableMessage::EnterZoneWithTicket {
            ticket_token: ticket.ticket_token,
        },
    )
    .await?
    {
        ServerReliableMessage::ZoneEntered { zone_id, npcs, .. } => {
            if zone_id != "zone_roadhouse" || npcs.len() < 3 {
                bail!(
                    "Unerwarteter Roadhouse-Zustand: zone={zone_id}, npcs={}",
                    npcs.len()
                );
            }
        }
        other => bail!("EnterZoneWithTicket failed: {other:?}"),
    }

    let dgram = encode_datagram(&ClientDatagram::MoveInput {
        character_id: hunter.id,
        seq: 1,
        pos_x: 5.0,
        pos_y: 2.0,
    })?;
    roadhouse_conn.send_datagram(dgram)?;
    let raw_snap = timeout(Duration::from_secs(3), roadhouse_conn.read_datagram())
        .await
        .context("wait for QUIC ZoneSnapshot datagram")??;
    let ServerDatagram::ZoneSnapshot { zone_id, .. } = decode_datagram(&raw_snap)?;
    if zone_id != "zone_roadhouse" {
        bail!("Datagramm meldet falsche Zone: {zone_id}");
    }

    // 5. Interaktion mit Jo Harvelle, Ellen Harvelle & Ashs Terminal
    info!("[5/8] Interagiere mit Jo Harvelles Echo, Ellen Harvelles Echo & Ashs Terminal...");
    for npc_id in ["npc_jo_harvelle", "npc_ellen_harvelle", "obj_ash_terminal"] {
        match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
            &roadhouse_conn,
            &ClientReliableMessage::InteractNpc {
                character_id: hunter.id,
                npc_id: npc_id.into(),
            },
        )
        .await?
        {
            ServerReliableMessage::NpcInteractionResult { quests, .. } => {
                if quests.is_empty() {
                    bail!("Keine Einstiegs-Quests bei {npc_id} erhalten!");
                }
            }
            other => bail!("InteractNpc({npc_id}) failed: {other:?}"),
        }
    }

    match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &roadhouse_conn,
        &ClientReliableMessage::QueryAshTerminal {
            character_id: hunter.id,
        },
    )
    .await?
    {
        ServerReliableMessage::AshTerminalStatus { report } => {
            if report.active_nodes.len() < 2 {
                bail!(
                    "Mirror-Node (game-node-frontier) ist noch nicht im Gossip-Mesh sichtbar: {:?}",
                    report.active_nodes
                );
            }
        }
        other => bail!("QueryAshTerminal failed: {other:?}"),
    }

    // 6. Verbinde mit IRCd -> `/salzen` in `#roadhouse`
    info!("[6/8] Verbinde mit IRCd ({irc_addr}) und sende '/salzen' in #roadhouse...");
    let irc_stream = TcpStream::connect(irc_addr)
        .await
        .with_context(|| format!("connect IRC {irc_addr}"))?;
    let (irc_r, mut irc_w) = irc_stream.into_split();
    let mut irc_lines = BufReader::new(irc_r).lines();

    irc_w
        .write_all(
            format!(
                "NICK {hunter_name}\r\nUSER hunter 0 * :New Lawrence Hunter\r\nJOIN #roadhouse\r\nJOIN #new_lawrence\r\n"
            )
            .as_bytes(),
        )
        .await?;
    irc_w.flush().await?;
    tokio::time::sleep(Duration::from_millis(350)).await;

    irc_w
        .write_all(b"PRIVMSG #roadhouse :/salzen an der Roadhouse-Schwelle\r\n")
        .await?;
    irc_w.flush().await?;
    wait_for_irc_substring(&mut irc_lines, &mut irc_w, "ECS-SCHUTZLINIE").await?;

    // 7. Überschreite Zonengrenze (x=105.0) -> Live PLAYER_HANDOVER auf `game-node-frontier`!
    info!("[7/8] Überschreite Zonengrenze (x=105.0) -> Live PLAYER_HANDOVER nach zone_new_lawrence_west...");
    let redirect = match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &roadhouse_conn,
        &ClientReliableMessage::MoveAndCheckBoundary {
            character_id: hunter.id,
            pos_x: 105.0,
            pos_y: 0.0,
        },
    )
    .await?
    {
        ServerReliableMessage::PlayerHandoverRedirect { redirect } => redirect,
        other => bail!("Erwartete PlayerHandoverRedirect, erhalten: {other:?}"),
    };

    let frontier_sock =
        resolve_endpoint_with_localhost_fallback(&redirect.target_client_addr).await?;
    let frontier_conn = client_ep.connect(frontier_sock, "ash-node")?.await?;

    match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &frontier_conn,
        &ClientReliableMessage::AcceptHandover {
            handover_token: redirect.handover_token,
        },
    )
    .await?
    {
        ServerReliableMessage::HandoverCompleted {
            zone_id, character, ..
        } => {
            if zone_id != "zone_new_lawrence_west" || !character.salt_ward_active {
                bail!("Spielerzustand beim Handover unvollständig!");
            }
        }
        other => bail!("AcceptHandover failed: {other:?}"),
    }

    irc_w
        .write_all(b"PRIVMSG #new_lawrence :/emf am Kolonie-Motel\r\n")
        .await?;
    irc_w.flush().await?;
    wait_for_irc_substring(
        &mut irc_lines,
        &mut irc_w,
        "EMF-SCAN zone_new_lawrence_west",
    )
    .await?;

    match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &frontier_conn,
        &ClientReliableMessage::UpgradeEquipment {
            character_id: hunter.id,
            slot: EquipmentSlot::MainHand,
            essence: EssenceType::RockSaltAsh,
        },
    )
    .await?
    {
        ServerReliableMessage::EquipmentUpgraded { character, .. } => {
            let main = character.equipment.get(EquipmentSlot::MainHand).unwrap();
            if main.quality != ItemQuality::Green {
                bail!("Hauptwaffe wurde nicht auf Grün aufgewertet!");
            }
        }
        other => bail!("UpgradeEquipment failed: {other:?}"),
    }

    match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &frontier_conn,
        &ClientReliableMessage::PerformCombat {
            character_id: hunter.id,
            action: CombatActionNet::CastBanishSheep,
        },
    )
    .await?
    {
        ServerReliableMessage::CombatOutcome {
            enemy_banished: true,
            ..
        } => {}
        other => bail!("CastBanishSheep failed: {other:?}"),
    }

    // 8. Löse Omen-Stufe 2 aus -> Verifiziere automatischen IRC-Funk-Alarm in `#new_lawrence`!
    info!("[8/8] Löse Omen-Durchbruch (Stufe 2) aus & erwarte Funk-Alarm in #new_lawrence...");
    match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &frontier_conn,
        &ClientReliableMessage::TriggerZoneOmen {
            zone_id: "zone_new_lawrence_west".into(),
            new_omen_level: 2,
            reason: "Schwefelriss am Kolonie-Motel aufgebrochen".into(),
        },
    )
    .await?
    {
        ServerReliableMessage::OmenUpdated {
            omen_level: 2,
            alert_broadcasted: true,
            ..
        } => {}
        other => bail!("TriggerZoneOmen failed: {other:?}"),
    }

    wait_for_irc_substring(&mut irc_lines, &mut irc_w, "OMEN-STUFE 2").await?;

    auth_conn.close(0u32.into(), b"done");
    roadhouse_conn.close(0u32.into(), b"done");
    frontier_conn.close(0u32.into(), b"done");
    info!("ALLE 8 END-TO-END SMOKE-TEST-PHASEN ERFOLGREICH BESTANDEN!");
    Ok(())
}

async fn wait_for_irc_substring(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    expected: &str,
) -> Result<String> {
    let fut = async {
        while let Some(line) = lines.next_line().await? {
            if let Some(rest) = line.strip_prefix("PING ") {
                let _ = writer
                    .write_all(format!("PONG {rest}\r\n").as_bytes())
                    .await;
                let _ = writer.flush().await;
                continue;
            }
            if line.contains(expected) {
                return Ok::<String, anyhow::Error>(line);
            }
        }
        bail!("IRC connection closed before receiving '{expected}'")
    };
    timeout(Duration::from_secs(8), fut)
        .await
        .with_context(|| format!("timeout waiting for IRC message containing '{expected}'"))?
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    let ca_pem = fs::read(&args.ca_path)
        .with_context(|| format!("read Root CA cert at {}", args.ca_path.display()))?;
    run_smoke_test_suite(&args.auth_addr, &args.irc_addr, &ca_pem).await
}

#[cfg(test)]
mod tests {
    use super::parse_localhost_fallback_port;

    #[test]
    fn maps_docker_service_endpoint_to_localhost_port() {
        let addr = parse_localhost_fallback_port("game-node-frontier:4434").unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:4434");
    }
}
