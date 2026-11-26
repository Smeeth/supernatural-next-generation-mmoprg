// SPDX-License-Identifier: GPL-3.0-or-later

mod mini_ircd;

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use ash_auth_gateway::{start_auth_gateway, AuthGatewayConfig};
use ash_core::character::STARTING_ZONE_ROADHOUSE;
use ash_core::equipment::{EquipmentSlot, EssenceType, ItemQuality};
use ash_db::AshStore;
use ash_game_node::{start_game_node, GameNodeConfig, NodeRole};
use ash_irc_bridge::{spawn_irc_bridge, IrcBridgeConfig};
use ash_protocol::framing::{decode_datagram, encode_datagram, rpc_call};
use ash_protocol::messages::{
    AuthRequest, AuthResponse, ClientDatagram, ClientReliableMessage, CombatActionNet,
    ServerDatagram, ServerReliableMessage,
};
use ash_protocol::tls::{build_quic_client, generate_ash_pki};
use clap::Parser;
use quinn::Endpoint;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "ash-test-cli",
    about = "Headless End-to-End Smoke-Test Runner for Ash-Net: New Lawrence"
)]
struct Args {
    /// Startet alle Dienste (PKI, Auth, Hub-Node, Frontier-Mirror-Node, IRCd, IRC-Bridge) in-process für schnelle CI-/Lokal-Tests.
    #[arg(long)]
    self_contained: bool,

    #[arg(long, env = "AUTH_ADDR", default_value = "127.0.0.1:4430")]
    auth_addr: String,

    #[arg(long, env = "IRC_ADDR", default_value = "127.0.0.1:6667")]
    irc_addr: String,

    #[arg(long, env = "TLS_CA_PATH", default_value = "certs/ca.crt")]
    ca_path: PathBuf,
}

async fn resolve_endpoint_with_localhost_fallback(raw: &str) -> Result<SocketAddr> {
    if let Ok(mut iter) = tokio::net::lookup_host(raw).await {
        if let Some(addr) = iter.next() {
            return Ok(addr);
        }
    }
    if let Some((_, port_str)) = raw.rsplit_once(':') {
        let fallback = format!("127.0.0.1:{port_str}");
        return fallback
            .parse::<SocketAddr>()
            .with_context(|| format!("parse localhost fallback '{fallback}'"));
    }
    bail!("cannot resolve target endpoint '{raw}'")
}

pub async fn run_smoke_test_suite(
    auth_addr: &str,
    irc_addr: &str,
    ca_pem: &[u8],
) -> Result<()> {
    let client_ep: Endpoint = build_quic_client("0.0.0.0:0".parse()?, ca_pem, None)?;
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
        bail!("Startzone ist nicht zone_roadhouse: {}", hunter.current_zone);
    }
    if hunter.equipment.slots.len() != 16 {
        bail!("Erwartete 16 Ausrüstungs-Slots, erhalten: {}", hunter.equipment.slots.len());
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

    let roadhouse_sock = resolve_endpoint_with_localhost_fallback(&ticket.target_client_addr).await?;
    let roadhouse_conn = client_ep.connect(roadhouse_sock, "ash-node")?.await?;

    match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &roadhouse_conn,
        &ClientReliableMessage::EnterZoneWithTicket {
            ticket_token: ticket.ticket_token,
        },
    )
    .await?
    {
        ServerReliableMessage::ZoneEntered {
            zone_id, npcs, ..
        } => {
            if zone_id != "zone_roadhouse" || npcs.len() < 3 {
                bail!("Unerwarteter Roadhouse-Zustand: zone={zone_id}, npcs={}", npcs.len());
            }
        }
        other => bail!("EnterZoneWithTicket failed: {other:?}"),
    }

    // Unreliable QUIC-Datagramm senden & 25-Hz-Snapshot empfangen
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
            info!(
                "   -> Ashs Terminal meldet {} aktive Mesh-Knoten & {} Zonen-Routen",
                report.active_nodes.len(),
                report.zone_omens.len()
            );
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

    if redirect.target_zone != "zone_new_lawrence_west" {
        bail!("Falsche Handover-Zielzone: {}", redirect.target_zone);
    }

    let frontier_sock = resolve_endpoint_with_localhost_fallback(&redirect.target_client_addr).await?;
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
            node_id,
            zone_id,
            character,
            ..
        } => {
            info!("   -> Handover erfolgreich auf Node '{node_id}' in Zone '{zone_id}'!");
            if zone_id != "zone_new_lawrence_west" || !character.salt_ward_active {
                bail!("Spielerzustand beim Handover unvollständig!");
            }
        }
        other => bail!("AcceptHandover failed: {other:?}"),
    }

    // `/emf` über IRC in `#new_lawrence` -> schließt Quest 1 ab (+5 Steinsalz-Asche) -> Hauptwaffe auf Grün aufwerten!
    irc_w
        .write_all(b"PRIVMSG #new_lawrence :/emf am Kolonie-Motel\r\n")
        .await?;
    irc_w.flush().await?;
    wait_for_irc_substring(&mut irc_lines, &mut irc_w, "EMF-SCAN zone_new_lawrence_west").await?;

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
            info!("   -> Hauptwaffe erfolgreich mit Steinsalz-Asche auf GRÜN (Geweiht) aufgewertet!");
        }
        other => bail!("UpgradeEquipment failed: {other:?}"),
    }

    // Prüfe 30-Sek.-Bann-Sheep im Kampf auf `game-node-frontier`
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
        } => info!("   -> 30-Sek.-Bann-Sheep (Blend-Augenbinde) auf Frontier-Knoten verifiziert!"),
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
    // Schließe Client-Endpoints sauber, damit Prozess sofort terminiert
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
                let _ = writer.write_all(format!("PONG {rest}\r\n").as_bytes()).await;
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

pub async fn run_self_contained_e2e() -> Result<()> {
    let pki = generate_ash_pki()?;
    let ca_bytes = pki.ca_cert_pem.into_bytes();
    let cert_bytes = pki.node_cert_pem.into_bytes();
    let key_bytes = pki.node_key_pem.into_bytes();

    let store = AshStore::new_shared_in_memory();
    let irc_sock = mini_ircd::start_embedded_mini_ircd("127.0.0.1:0".parse()?).await?;

    // 1. Hub-Node (`game-node-roadhouse`)
    let hub = start_game_node(
        GameNodeConfig {
            node_id: "game-node-roadhouse".into(),
            role: NodeRole::Hub,
            zones: vec!["zone_roadhouse".into()],
            client_bind_addr: "127.0.0.1:0".parse()?,
            client_public_addr: "127.0.0.1:0".into(),
            mesh_bind_addr: "127.0.0.1:0".parse()?,
            mesh_public_addr: "127.0.0.1:0".into(),
            hub_mesh_addr: None,
            tick_rate_hz: 25,
            ca_pem: ca_bytes.clone(),
            cert_pem: cert_bytes.clone(),
            key_pem: key_bytes.clone(),
        },
        store.clone(),
    )
    .await?;

    let hub_client_addr = format!("127.0.0.1:{}", hub.client_endpoint.local_addr()?.port());
    let hub_mesh_addr = format!("127.0.0.1:{}", hub.mesh_endpoint.local_addr()?.port());

    // 2. Mirror-Node (`game-node-frontier`) per mTLS-QUIC am Hub andocken lassen
    let _frontier = start_game_node(
        GameNodeConfig {
            node_id: "game-node-frontier".into(),
            role: NodeRole::Mirror,
            zones: vec![
                "zone_new_lawrence_west".into(),
                "zone_salzminen".into(),
            ],
            client_bind_addr: "127.0.0.1:0".parse()?,
            client_public_addr: "127.0.0.1:0".into(),
            mesh_bind_addr: "127.0.0.1:0".parse()?,
            mesh_public_addr: "127.0.0.1:0".into(),
            hub_mesh_addr: Some(hub_mesh_addr.clone()),
            tick_rate_hz: 25,
            ca_pem: ca_bytes.clone(),
            cert_pem: cert_bytes.clone(),
            key_pem: key_bytes.clone(),
        },
        store.clone(),
    )
    .await?;

    // 3. Auth-Gateway
    let auth_ep = start_auth_gateway(
        AuthGatewayConfig {
            bind_addr: "127.0.0.1:0".parse()?,
            cert_pem: cert_bytes.clone(),
            key_pem: key_bytes.clone(),
            roadhouse_fallback_client_addr: hub_client_addr,
            route_stale_timeout_ms: 10_000,
        },
        store.clone(),
    )?;
    let auth_addr = format!("127.0.0.1:{}", auth_ep.local_addr()?.port());

    // 4. IRC-Bridge
    let _bridge = spawn_irc_bridge(IrcBridgeConfig {
        irc_addr: irc_sock.to_string(),
        bot_nick: "AshBridge".into(),
        oper_user: None,
        oper_password: None,
        hub_mesh_addr: hub_mesh_addr.clone(),
        ca_pem: ca_bytes.clone(),
        cert_pem: cert_bytes,
        key_pem: key_bytes,
    })?;

    // Warte kurz auf initialen mTLS-Gossip-Sync & IRC-Bridge-Join
    tokio::time::sleep(Duration::from_millis(600)).await;

    run_smoke_test_suite(&auth_addr, &irc_sock.to_string(), &ca_bytes).await
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    if args.self_contained {
        return run_self_contained_e2e().await;
    }

    let ca_pem = fs::read(&args.ca_path)
        .with_context(|| format!("read Root CA cert at {}", args.ca_path.display()))?;
    run_smoke_test_suite(&args.auth_addr, &args.irc_addr, &ca_pem).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ash_core::character::{CharacterClass, CharacterSheet, PlayableRace};

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn full_end_to_end_mesh_handover_and_irc_smoke_test() {
        run_self_contained_e2e()
            .await
            .expect("End-to-End Smoke Test must pass");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn verifies_crash_recovery_fallback_to_roadhouse() {
        let pki = generate_ash_pki().unwrap();
        let ca_bytes = pki.ca_cert_pem.into_bytes();
        let cert_bytes = pki.node_cert_pem.into_bytes();
        let key_bytes = pki.node_key_pem.into_bytes();

        let store = AshStore::new_shared_in_memory();
        let auth_ep = start_auth_gateway(
            AuthGatewayConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                cert_pem: cert_bytes,
                key_pem: key_bytes,
                roadhouse_fallback_client_addr: "127.0.0.1:4433".into(),
                route_stale_timeout_ms: 5_000,
            },
            store.clone(),
        )
        .unwrap();

        let acc_id = store.create_account("sam", "hash").await.unwrap();
        let sess = store.create_session(acc_id).await.unwrap();
        let mut ch = CharacterSheet::create_new_hunter(
            acc_id,
            "Sam_III",
            PlayableRace::ManOfLetters,
            CharacterClass::BannWaechter,
        )
        .unwrap();
        // Simuliere einen Charakter, dessen letzter Raid-Knoten (`zone_hoellentor_alpha`) abgestürzt ist
        ch.current_zone = "zone_hoellentor_alpha".into();
        store.save_character(&ch).await.unwrap();

        let client_ep = build_quic_client("0.0.0.0:0".parse().unwrap(), &ca_bytes, None).unwrap();
        let conn = client_ep
            .connect(auth_ep.local_addr().unwrap(), "ash-node")
            .unwrap()
            .await
            .unwrap();

        let resp = rpc_call::<AuthRequest, AuthResponse>(
            &conn,
            &AuthRequest::IssueZoneTicket {
                session_token: sess,
                character_id: ch.id,
            },
        )
        .await
        .unwrap();

        match resp {
            AuthResponse::ZoneTicketIssued { ticket } => {
                assert_eq!(ticket.target_zone, STARTING_ZONE_ROADHOUSE);
                assert!(ticket.fallback_to_roadhouse);
            }
            other => panic!("expected ZoneTicketIssued with fallback, got {other:?}"),
        }
    }
}
