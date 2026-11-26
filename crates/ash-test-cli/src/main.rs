// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;
use std::fs;
use std::io::BufReader as StdBufReader;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use ash_core::character::{CharacterSheet, STARTING_ZONE_ROADHOUSE};
use ash_core::equipment::{EquipmentSlot, EssenceType, ItemQuality};
use ash_core::scenario::{
    execute_sim_scenario, ScenarioActorSpec, ScenarioExecError, ScenarioFailureReport,
    ScenarioMode, ScenarioRunOutcome, ScenarioSpec, ScenarioStep, ScenarioStepTrace,
};
use ash_protocol::client::{
    AuthRequest, AuthResponse, ClientDatagram, ClientReliableMessage, CombatActionNet,
    ServerDatagram, ServerReliableMessage,
};
use ash_protocol::codec::{
    decode_datagram, decode_frame_payload, encode_datagram, encode_frame_payload, ASH_ALPN,
    MAX_FRAME_SIZE,
};
use clap::{Parser, Subcommand};
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
    about = "Headless Blackbox QUIC + IRC Scenario & Smoke-Test Client for Ash-Net: New Lawrence"
)]
struct Cli {
    #[arg(
        long,
        env = "AUTH_ADDR",
        default_value = "127.0.0.1:4430",
        global = true
    )]
    auth_addr: String,

    #[arg(
        long,
        env = "IRC_ADDR",
        default_value = "127.0.0.1:6667",
        global = true
    )]
    irc_addr: String,

    #[arg(
        long,
        env = "TLS_CA_PATH",
        default_value = "certs/ca.crt",
        global = true
    )]
    ca_path: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Führt ein deklaratives TOML-Szenario (`mode = "sim"` oder `mode = "e2e"`) aus.
    RunScenario {
        /// Pfad zur Szenario-TOML-Datei (z. B. `harness/scenarios/00-smoke-mesh.toml`).
        scenario_path: PathBuf,

        /// Optionaler Zielpfad für den maschinenlesbaren Step-/Tick-Trace (`NDJSON`).
        #[arg(long)]
        trace: Option<PathBuf>,

        /// Optionaler Zielpfad für den kompakten Markdown-Ergebnis-/Fehlerbericht.
        #[arg(long)]
        report: Option<PathBuf>,

        /// Aktuelle Sprint-Nummer zur Auswertung von `min_sprint`.
        #[arg(long, env = "CURRENT_SPRINT", default_value_t = 1)]
        current_sprint: u32,
    },
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

struct LiveActorSession {
    session_token: String,
    sheet: CharacterSheet,
    zone_conn: Option<Connection>,
    irc_lines: Option<tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>>,
    irc_writer: Option<tokio::net::tcp::OwnedWriteHalf>,
}

fn format_live_snapshot(sessions: &BTreeMap<String, LiveActorSession>) -> String {
    if sessions.is_empty() {
        return "live_actors={}".to_string();
    }
    sessions
        .iter()
        .map(|(id, s)| {
            let main_q = s
                .sheet
                .equipment
                .get(EquipmentSlot::MainHand)
                .map(|i| i.quality.display_name())
                .unwrap_or("none");
            format!(
                "{id}:[name={} zone={} pos=({:.1},{:.1}) slots={} main_hand='{}' salt_ward={}]",
                s.sheet.name,
                s.sheet.current_zone,
                s.sheet.pos_x,
                s.sheet.pos_y,
                s.sheet.equipment.slots.len(),
                main_q,
                s.sheet.salt_ward_active
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

async fn ensure_actor_irc_connected(actor: &mut LiveActorSession, irc_addr: &str) -> Result<()> {
    if actor.irc_writer.is_some() {
        return Ok(());
    }
    let irc_sock = resolve_endpoint_with_localhost_fallback(irc_addr).await?;
    let stream = TcpStream::connect(irc_sock)
        .await
        .with_context(|| format!("connect IRC {irc_addr}"))?;
    let (r, mut w) = stream.into_split();
    let lines = BufReader::new(r).lines();
    let join_handshake = format!(
        "NICK {}\r\nUSER hunter 0 * :Ash-Net Harness\r\nJOIN #roadhouse\r\nJOIN #new_lawrence\r\nJOIN #zone_salzminen\r\n",
        actor.sheet.name
    );
    w.write_all(join_handshake.as_bytes()).await?;
    w.flush().await?;
    tokio::time::sleep(Duration::from_millis(350)).await;
    actor.irc_lines = Some(lines);
    actor.irc_writer = Some(w);
    Ok(())
}

async fn execute_e2e_scenario(
    spec: &ScenarioSpec,
    current_sprint: u32,
    auth_addr: &str,
    irc_addr: &str,
    ca_path: &Path,
) -> std::result::Result<ScenarioRunOutcome, ScenarioExecError> {
    if spec.min_sprint > current_sprint {
        return Ok(ScenarioRunOutcome::Skipped {
            scenario_id: spec.id.clone(),
            min_sprint: spec.min_sprint,
            current_sprint,
        });
    }

    let mut traces = Vec::with_capacity(spec.steps.len());
    let mut sessions: BTreeMap<String, LiveActorSession> = BTreeMap::new();

    let make_bootstrap_err = |expected: &str, actual: String| -> ScenarioExecError {
        Box::new((
            ScenarioFailureReport {
                scenario_id: spec.id.clone(),
                failed_step_index: 0,
                failed_step: "bootstrap_e2e_transport".into(),
                expected: expected.to_string(),
                actual,
                state_snapshot: "live_actors={}".into(),
            },
            Vec::new(),
        ))
    };

    let ca_pem = fs::read(ca_path).map_err(|e| {
        make_bootstrap_err(
            "readable TLS CA file",
            format!("{}: {e}", ca_path.display()),
        )
    })?;
    let bind_any: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let client_ep = build_public_quic_client(bind_any, &ca_pem)
        .map_err(|e| make_bootstrap_err("valid QUIC client endpoint", format!("{e:#}")))?;
    let auth_sock = resolve_endpoint_with_localhost_fallback(auth_addr)
        .await
        .map_err(|e| make_bootstrap_err("resolvable auth_addr", format!("{e:#}")))?;
    let auth_conn = client_ep
        .connect(auth_sock, "ash-node")
        .map_err(|e| make_bootstrap_err("QUIC connect init", format!("{e:#}")))?
        .await
        .map_err(|e| make_bootstrap_err("QUIC handshake to auth-gateway", format!("{e:#}")))?;

    let actor_defs: BTreeMap<String, &ScenarioActorSpec> =
        spec.actors.iter().map(|a| (a.id.clone(), a)).collect();
    let run_suffix = Uuid::new_v4().simple().to_string()[..6].to_string();
    let mut shared_probe_session: Option<String> = None;

    for (idx, step) in spec.steps.iter().enumerate() {
        let tick = (idx as u64 + 1) * 25;
        let before = format_live_snapshot(&sessions);
        let action_name = step.action_name().to_string();
        let actor_label = step.primary_actor().to_string();

        let fail = |expected: String,
                    actual: String,
                    cur: &BTreeMap<String, LiveActorSession>,
                    tr: &[ScenarioStepTrace]|
         -> ScenarioExecError {
            Box::new((
                ScenarioFailureReport {
                    scenario_id: spec.id.clone(),
                    failed_step_index: idx,
                    failed_step: format!("{action_name} ({actor_label})"),
                    expected,
                    actual,
                    state_snapshot: format_live_snapshot(cur),
                },
                tr.to_vec(),
            ))
        };

        let detail = match step {
            ScenarioStep::RegisterAndCreate { actor } => {
                let Some(def) = actor_defs.get(actor) else {
                    return Err(fail(
                        format!("actor '{actor}' defined"),
                        "missing in [[actors]]".into(),
                        &sessions,
                        &traces,
                    ));
                };
                let unique_user = format!("{}_{}", def.id, run_suffix);
                let unique_hunter = format!("{}_{}", def.name, run_suffix);
                let sess_tok = match rpc_call::<AuthRequest, AuthResponse>(
                    &auth_conn,
                    &AuthRequest::RegisterAccount {
                        username: unique_user,
                        password: "impala_1967_password".into(),
                    },
                )
                .await
                {
                    Ok(AuthResponse::AccountRegistered { session_token, .. }) => session_token,
                    Ok(other) => {
                        return Err(fail(
                            "AuthResponse::AccountRegistered".into(),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                    Err(e) => {
                        return Err(fail(
                            "QUIC RegisterAccount".into(),
                            format!("{e:#}"),
                            &sessions,
                            &traces,
                        ));
                    }
                };
                shared_probe_session = Some(sess_tok.clone());

                let sheet = match rpc_call::<AuthRequest, AuthResponse>(
                    &auth_conn,
                    &AuthRequest::CreateCharacter {
                        session_token: sess_tok.clone(),
                        name: unique_hunter.clone(),
                        race: def.race.clone(),
                        class: def.class.clone(),
                    },
                )
                .await
                {
                    Ok(AuthResponse::CharacterCreated { character }) => character,
                    Ok(other) => {
                        return Err(fail(
                            "AuthResponse::CharacterCreated".into(),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                    Err(e) => {
                        return Err(fail(
                            "QUIC CreateCharacter".into(),
                            format!("{e:#}"),
                            &sessions,
                            &traces,
                        ));
                    }
                };
                sessions.insert(
                    actor.clone(),
                    LiveActorSession {
                        session_token: sess_tok,
                        sheet,
                        zone_conn: None,
                        irc_lines: None,
                        irc_writer: None,
                    },
                );
                format!("registered & created '{unique_hunter}'")
            }
            ScenarioStep::ExpectRejectedCreation {
                name,
                race,
                class,
                expected_reason,
            } => {
                let probe_tok = match &shared_probe_session {
                    Some(t) => t.clone(),
                    None => {
                        let u = format!("probe_{run_suffix}");
                        match rpc_call::<AuthRequest, AuthResponse>(
                            &auth_conn,
                            &AuthRequest::RegisterAccount {
                                username: u,
                                password: "impala_1967_password".into(),
                            },
                        )
                        .await
                        {
                            Ok(AuthResponse::AccountRegistered { session_token, .. }) => {
                                shared_probe_session = Some(session_token.clone());
                                session_token
                            }
                            other => {
                                return Err(fail(
                                    "probe session registered".into(),
                                    format!("{other:?}"),
                                    &sessions,
                                    &traces,
                                ));
                            }
                        }
                    }
                };

                match rpc_call::<AuthRequest, AuthResponse>(
                    &auth_conn,
                    &AuthRequest::CreateCharacter {
                        session_token: probe_tok,
                        name: format!("{name}_{run_suffix}"),
                        race: race.clone(),
                        class: class.clone(),
                    },
                )
                .await
                {
                    Ok(AuthResponse::Error { code, message })
                        if code.contains(expected_reason) || message.contains(expected_reason) =>
                    {
                        format!("rejected with {code}")
                    }
                    other => {
                        return Err(fail(
                            format!("AuthResponse::Error matching '{expected_reason}'"),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                }
            }
            ScenarioStep::Expect16GraySlots { actor } => {
                let Some(s) = sessions.get(actor) else {
                    return Err(fail(
                        format!("actor '{actor}'"),
                        "missing".into(),
                        &sessions,
                        &traces,
                    ));
                };
                if s.sheet.equipment.slots.len() != 16 {
                    return Err(fail(
                        "16 slots".into(),
                        format!("{} slots", s.sheet.equipment.slots.len()),
                        &sessions,
                        &traces,
                    ));
                }
                for slot in EquipmentSlot::ALL {
                    let Some(item) = s.sheet.equipment.get(slot) else {
                        return Err(fail(
                            format!("{slot:?}"),
                            "missing".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    if item.quality != ItemQuality::Grey {
                        return Err(fail(
                            "ItemQuality::Grey".into(),
                            format!("{:?}", item.quality),
                            &sessions,
                            &traces,
                        ));
                    }
                }
                "verified 16 Grey starter items".into()
            }
            ScenarioStep::EnterRoadhouse { actor } => {
                let (sess_tok, cid) = {
                    let Some(s) = sessions.get(actor) else {
                        return Err(fail(
                            format!("actor '{actor}'"),
                            "missing".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    (s.session_token.clone(), s.sheet.id)
                };
                let ticket = match rpc_call::<AuthRequest, AuthResponse>(
                    &auth_conn,
                    &AuthRequest::IssueZoneTicket {
                        session_token: sess_tok,
                        character_id: cid,
                    },
                )
                .await
                {
                    Ok(AuthResponse::ZoneTicketIssued { ticket }) => ticket,
                    other => {
                        return Err(fail(
                            "ZoneTicketIssued".into(),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                };

                let sock = resolve_endpoint_with_localhost_fallback(&ticket.target_client_addr)
                    .await
                    .map_err(|e| {
                        fail(
                            "resolve roadhouse addr".into(),
                            format!("{e:#}"),
                            &sessions,
                            &traces,
                        )
                    })?;
                let conn = client_ep
                    .connect(sock, "ash-node")
                    .map_err(|e| fail("connect init".into(), format!("{e:#}"), &sessions, &traces))?
                    .await
                    .map_err(|e| {
                        fail(
                            "QUIC connect roadhouse".into(),
                            format!("{e:#}"),
                            &sessions,
                            &traces,
                        )
                    })?;

                match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
                    &conn,
                    &ClientReliableMessage::EnterZoneWithTicket {
                        ticket_token: ticket.ticket_token,
                    },
                )
                .await
                {
                    Ok(ServerReliableMessage::ZoneEntered {
                        zone_id,
                        character,
                        npcs,
                        ..
                    }) if zone_id == STARTING_ZONE_ROADHOUSE && npcs.len() >= 3 => {
                        let dgram = encode_datagram(&ClientDatagram::MoveInput {
                            character_id: cid,
                            seq: 1,
                            pos_x: 5.0,
                            pos_y: 2.0,
                        })
                        .map_err(|e| {
                            fail(
                                "encode datagram".into(),
                                format!("{e:#}"),
                                &sessions,
                                &traces,
                            )
                        })?;
                        let _ = conn.send_datagram(dgram);
                        let raw_snap = timeout(Duration::from_secs(3), conn.read_datagram())
                            .await
                            .map_err(|_| {
                                fail(
                                    "25Hz ZoneSnapshot within 3s".into(),
                                    "timeout".into(),
                                    &sessions,
                                    &traces,
                                )
                            })?
                            .map_err(|e| {
                                fail("read datagram".into(), format!("{e:#}"), &sessions, &traces)
                            })?;
                        let ServerDatagram::ZoneSnapshot {
                            zone_id: snap_z, ..
                        } = decode_datagram(&raw_snap).map_err(|e| {
                            fail(
                                "decode snapshot".into(),
                                format!("{e:#}"),
                                &sessions,
                                &traces,
                            )
                        })?;
                        if snap_z != STARTING_ZONE_ROADHOUSE {
                            return Err(fail(
                                STARTING_ZONE_ROADHOUSE.into(),
                                snap_z,
                                &sessions,
                                &traces,
                            ));
                        }
                        if let Some(s) = sessions.get_mut(actor) {
                            s.sheet = character;
                            s.zone_conn = Some(conn);
                        }
                        "entered zone_roadhouse + verified 25Hz QUIC datagram".into()
                    }
                    other => {
                        return Err(fail(
                            "ZoneEntered(zone_roadhouse, npcs>=3)".into(),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                }
            }
            ScenarioStep::InteractNpc { actor, npc_id } => {
                let (conn, cid) = {
                    let Some(s) = sessions.get(actor) else {
                        return Err(fail(
                            format!("actor '{actor}'"),
                            "missing".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    let Some(c) = s.zone_conn.clone() else {
                        return Err(fail(
                            "active zone_conn".into(),
                            "None".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    (c, s.sheet.id)
                };
                match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
                    &conn,
                    &ClientReliableMessage::InteractNpc {
                        character_id: cid,
                        npc_id: npc_id.clone(),
                    },
                )
                .await
                {
                    Ok(ServerReliableMessage::NpcInteractionResult {
                        npc_name,
                        quests,
                        character,
                        ..
                    }) if !quests.is_empty() => {
                        if let Some(s) = sessions.get_mut(actor) {
                            s.sheet = character;
                        }
                        format!("interacted with '{npc_name}' (quests={})", quests.len())
                    }
                    other => {
                        return Err(fail(
                            format!("NpcInteractionResult({npc_id}) with quests"),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                }
            }
            ScenarioStep::QueryAshTerminal {
                actor,
                min_active_nodes,
            } => {
                let (conn, cid) = {
                    let Some(s) = sessions.get(actor) else {
                        return Err(fail(
                            format!("actor '{actor}'"),
                            "missing".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    let Some(c) = s.zone_conn.clone() else {
                        return Err(fail(
                            "active zone_conn".into(),
                            "None".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    (c, s.sheet.id)
                };
                match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
                    &conn,
                    &ClientReliableMessage::QueryAshTerminal { character_id: cid },
                )
                .await
                {
                    Ok(ServerReliableMessage::AshTerminalStatus { report })
                        if report.active_nodes.len() >= *min_active_nodes =>
                    {
                        format!(
                            "terminal reported {} active mesh nodes",
                            report.active_nodes.len()
                        )
                    }
                    other => {
                        return Err(fail(
                            format!("AshTerminalStatus(active_nodes >= {min_active_nodes})"),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                }
            }
            ScenarioStep::IrcCommand {
                actor,
                channel,
                command_line,
                expect_reply_contains,
            } => {
                let init_res = {
                    let Some(s) = sessions.get_mut(actor) else {
                        return Err(fail(
                            format!("actor '{actor}'"),
                            "missing".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    ensure_actor_irc_connected(s, irc_addr).await
                };
                if let Err(e) = init_res {
                    return Err(fail(
                        "IRC connection established".into(),
                        format!("{e:#}"),
                        &sessions,
                        &traces,
                    ));
                }

                let wait_res = {
                    let s = sessions.get_mut(actor).unwrap();
                    let w = s.irc_writer.as_mut().unwrap();
                    let lines = s.irc_lines.as_mut().unwrap();
                    let wire = format!("PRIVMSG {channel} :{command_line}\r\n");
                    if let Err(e) = w.write_all(wire.as_bytes()).await {
                        Err(anyhow::anyhow!(e))
                    } else {
                        let _ = w.flush().await;
                        wait_for_irc_substring(lines, w, expect_reply_contains).await
                    }
                };
                match wait_res {
                    Ok(matched_line) => {
                        if command_line.starts_with("/salzen") {
                            if let Some(s) = sessions.get_mut(actor) {
                                s.sheet.salt_ward_active = true;
                            }
                        }
                        format!("IRC matched: {matched_line}")
                    }
                    Err(e) => {
                        return Err(fail(
                            format!("IRC reply containing '{expect_reply_contains}'"),
                            format!("{e:#}"),
                            &sessions,
                            &traces,
                        ));
                    }
                }
            }
            ScenarioStep::MoveAndHandover {
                actor,
                pos_x,
                pos_y,
                expected_zone,
            } => {
                let (old_conn, cid) = {
                    let Some(s) = sessions.get(actor) else {
                        return Err(fail(
                            format!("actor '{actor}'"),
                            "missing".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    let Some(c) = s.zone_conn.clone() else {
                        return Err(fail(
                            "active zone_conn".into(),
                            "None".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    (c, s.sheet.id)
                };
                let redirect = match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
                    &old_conn,
                    &ClientReliableMessage::MoveAndCheckBoundary {
                        character_id: cid,
                        pos_x: *pos_x,
                        pos_y: *pos_y,
                    },
                )
                .await
                {
                    Ok(ServerReliableMessage::PlayerHandoverRedirect { redirect }) => redirect,
                    other => {
                        return Err(fail(
                            "PlayerHandoverRedirect".into(),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                };

                let next_sock =
                    resolve_endpoint_with_localhost_fallback(&redirect.target_client_addr)
                        .await
                        .map_err(|e| {
                            fail(
                                "resolve target node".into(),
                                format!("{e:#}"),
                                &sessions,
                                &traces,
                            )
                        })?;
                let next_conn = client_ep
                    .connect(next_sock, "ash-node")
                    .map_err(|e| {
                        fail(
                            "connect target node init".into(),
                            format!("{e:#}"),
                            &sessions,
                            &traces,
                        )
                    })?
                    .await
                    .map_err(|e| {
                        fail(
                            "QUIC connect target node".into(),
                            format!("{e:#}"),
                            &sessions,
                            &traces,
                        )
                    })?;

                match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
                    &next_conn,
                    &ClientReliableMessage::AcceptHandover {
                        handover_token: redirect.handover_token,
                    },
                )
                .await
                {
                    Ok(ServerReliableMessage::HandoverCompleted {
                        node_id,
                        zone_id,
                        character,
                        ..
                    }) if &zone_id == expected_zone => {
                        old_conn.close(0u32.into(), b"handover_complete");
                        if let Some(s) = sessions.get_mut(actor) {
                            s.sheet = character;
                            s.zone_conn = Some(next_conn);
                        }
                        format!("handover completed to '{zone_id}' on '{node_id}'")
                    }
                    other => {
                        return Err(fail(
                            format!("HandoverCompleted({expected_zone})"),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                }
            }
            ScenarioStep::UpgradeMainHandGreen { actor } => {
                let (conn, cid) = {
                    let Some(s) = sessions.get(actor) else {
                        return Err(fail(
                            format!("actor '{actor}'"),
                            "missing".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    let Some(c) = s.zone_conn.clone() else {
                        return Err(fail(
                            "active zone_conn".into(),
                            "None".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    (c, s.sheet.id)
                };
                match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
                    &conn,
                    &ClientReliableMessage::UpgradeEquipment {
                        character_id: cid,
                        slot: EquipmentSlot::MainHand,
                        essence: EssenceType::RockSaltAsh,
                    },
                )
                .await
                {
                    Ok(ServerReliableMessage::EquipmentUpgraded { character, .. })
                        if character
                            .equipment
                            .get(EquipmentSlot::MainHand)
                            .is_some_and(|i| i.quality == ItemQuality::Green) =>
                    {
                        if let Some(s) = sessions.get_mut(actor) {
                            s.sheet = character;
                        }
                        "upgraded MainHand to Green (Geweiht)".into()
                    }
                    other => {
                        return Err(fail(
                            "MainHand upgraded to ItemQuality::Green".into(),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                }
            }
            ScenarioStep::PerformBanishSheep { actor } => {
                let (conn, cid) = {
                    let Some(s) = sessions.get(actor) else {
                        return Err(fail(
                            format!("actor '{actor}'"),
                            "missing".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    let Some(c) = s.zone_conn.clone() else {
                        return Err(fail(
                            "active zone_conn".into(),
                            "None".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    (c, s.sheet.id)
                };
                match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
                    &conn,
                    &ClientReliableMessage::PerformCombat {
                        character_id: cid,
                        action: CombatActionNet::CastBanishSheep,
                    },
                )
                .await
                {
                    Ok(ServerReliableMessage::CombatOutcome {
                        enemy_banished: true,
                        summary,
                        ..
                    }) => summary,
                    other => {
                        return Err(fail(
                            "CombatOutcome(enemy_banished: true)".into(),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                }
            }
            ScenarioStep::TriggerOmen {
                actor,
                zone_id,
                omen_level,
                reason,
                expect_irc_alert_contains,
            } => {
                let conn = {
                    let Some(s) = sessions.get(actor) else {
                        return Err(fail(
                            format!("actor '{actor}'"),
                            "missing".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    let Some(c) = s.zone_conn.clone() else {
                        return Err(fail(
                            "active zone_conn".into(),
                            "None".into(),
                            &sessions,
                            &traces,
                        ));
                    };
                    c
                };
                match rpc_call::<ClientReliableMessage, ServerReliableMessage>(
                    &conn,
                    &ClientReliableMessage::TriggerZoneOmen {
                        zone_id: zone_id.clone(),
                        new_omen_level: *omen_level,
                        reason: reason.clone(),
                    },
                )
                .await
                {
                    Ok(ServerReliableMessage::OmenUpdated {
                        omen_level: lvl, ..
                    }) if lvl == *omen_level => {
                        if let Some(alert_sub) = expect_irc_alert_contains {
                            let init_res = {
                                let s = sessions.get_mut(actor).unwrap();
                                ensure_actor_irc_connected(s, irc_addr).await
                            };
                            if let Err(e) = init_res {
                                return Err(fail(
                                    "IRC connected".into(),
                                    format!("{e:#}"),
                                    &sessions,
                                    &traces,
                                ));
                            }
                            let s = sessions.get_mut(actor).unwrap();
                            let w = s.irc_writer.as_mut().unwrap();
                            let lines = s.irc_lines.as_mut().unwrap();
                            wait_for_irc_substring(lines, w, alert_sub)
                                .await
                                .map_err(|e| {
                                    fail(
                                        format!("IRC alert '{alert_sub}'"),
                                        format!("{e:#}"),
                                        &sessions,
                                        &traces,
                                    )
                                })?;
                        }
                        format!("omen set to {omen_level} in {zone_id}")
                    }
                    other => {
                        return Err(fail(
                            format!("OmenUpdated({omen_level})"),
                            format!("{other:?}"),
                            &sessions,
                            &traces,
                        ));
                    }
                }
            }
            ScenarioStep::AssertState {
                actor,
                expected_zone,
                expected_salt_ward,
                min_slots,
            } => {
                let Some(s) = sessions.get(actor) else {
                    return Err(fail(
                        format!("actor '{actor}'"),
                        "missing".into(),
                        &sessions,
                        &traces,
                    ));
                };
                if let Some(z) = expected_zone {
                    if &s.sheet.current_zone != z {
                        return Err(fail(
                            z.clone(),
                            s.sheet.current_zone.clone(),
                            &sessions,
                            &traces,
                        ));
                    }
                }
                if let Some(sw) = expected_salt_ward {
                    if s.sheet.salt_ward_active != *sw {
                        return Err(fail(
                            format!("salt_ward={sw}"),
                            format!("salt_ward={}", s.sheet.salt_ward_active),
                            &sessions,
                            &traces,
                        ));
                    }
                }
                if let Some(ms) = min_slots {
                    if s.sheet.equipment.slots.len() < *ms {
                        return Err(fail(
                            format!("slots >= {ms}"),
                            format!("slots = {}", s.sheet.equipment.slots.len()),
                            &sessions,
                            &traces,
                        ));
                    }
                }
                "live state assertions matched".into()
            }
            other => {
                return Err(fail(
                    "e2e-compatible step".into(),
                    format!("step '{}' is sim-only", other.action_name()),
                    &sessions,
                    &traces,
                ));
            }
        };

        let after = format_live_snapshot(&sessions);
        traces.push(ScenarioStepTrace {
            scenario_id: spec.id.clone(),
            step_index: idx,
            tick,
            actor: actor_label,
            action: action_name,
            state_before: before,
            state_after: after,
            assertion_passed: true,
            detail,
        });
    }

    for s in sessions.values() {
        if let Some(c) = &s.zone_conn {
            c.close(0u32.into(), b"done");
        }
    }
    auth_conn.close(0u32.into(), b"done");

    Ok(ScenarioRunOutcome::Passed {
        scenario_id: spec.id.clone(),
        steps_executed: traces.len(),
        traces,
    })
}

fn write_ndjson_trace(path: &Path, traces: &[ScenarioStepTrace]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut out = String::new();
    for entry in traces {
        out.push_str(&serde_json::to_string(entry)?);
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}

fn write_markdown_report(
    path: &Path,
    spec: &ScenarioSpec,
    result: &std::result::Result<ScenarioRunOutcome, ScenarioFailureReport>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let content = match result {
        Ok(ScenarioRunOutcome::Skipped {
            min_sprint,
            current_sprint,
            ..
        }) => format!(
            "# Scenario Report: `{}`\n\n- **Status:** SKIPPED (`min_sprint = {}` > `CURRENT_SPRINT = {}`)\n- **Mode:** `{:?}`\n",
            spec.id, min_sprint, current_sprint, spec.mode
        ),
        Ok(ScenarioRunOutcome::Passed { steps_executed, .. }) => format!(
            "# Scenario Report: `{}`\n\n- **Status:** PASSED\n- **Mode:** `{:?}`\n- **Seed:** `{}`\n- **Steps Executed:** `{}`\n",
            spec.id, spec.mode, spec.seed, steps_executed
        ),
        Err(fail) => format!(
            "# Scenario Report: `{}`\n\n- **Status:** FAILED\n\n```text\n{}\n```\n",
            spec.id,
            fail.format_compact_block()
        ),
    };
    fs::write(path, content)?;
    Ok(())
}

async fn run_scenario_command(
    scenario_path: &Path,
    trace_path: Option<&Path>,
    report_path: Option<&Path>,
    current_sprint: u32,
    auth_addr: &str,
    irc_addr: &str,
    ca_path: &Path,
) -> ExitCode {
    let raw = match fs::read_to_string(scenario_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "FAILED_STEP: load_scenario ({})\nEXPECTED: readable TOML scenario\nACTUAL: {e}",
                scenario_path.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let spec = match ScenarioSpec::from_toml_str(&raw) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "FAILED_STEP: parse_scenario ({})\nEXPECTED: valid ScenarioSpec TOML\nACTUAL: {e}",
                scenario_path.display()
            );
            return ExitCode::FAILURE;
        }
    };

    let exec_res = match spec.mode {
        ScenarioMode::Sim => execute_sim_scenario(&spec, current_sprint),
        ScenarioMode::E2e => {
            execute_e2e_scenario(&spec, current_sprint, auth_addr, irc_addr, ca_path).await
        }
    };

    match exec_res {
        Ok(outcome) => {
            match &outcome {
                ScenarioRunOutcome::Skipped {
                    scenario_id,
                    min_sprint,
                    current_sprint,
                } => {
                    info!(
                        "[SKIP] Scenario '{}' skipped (min_sprint={} > CURRENT_SPRINT={})",
                        scenario_id, min_sprint, current_sprint
                    );
                    if let Some(tp) = trace_path {
                        let _ = write_ndjson_trace(tp, &[]);
                    }
                }
                ScenarioRunOutcome::Passed {
                    scenario_id,
                    steps_executed,
                    traces,
                } => {
                    info!(
                        "[PASS] Scenario '{}' ({:?}) passed ({} steps)",
                        scenario_id, spec.mode, steps_executed
                    );
                    if let Some(tp) = trace_path {
                        if let Err(e) = write_ndjson_trace(tp, traces) {
                            eprintln!("WARN: failed writing trace {}: {e}", tp.display());
                        }
                    }
                }
            }
            if let Some(rp) = report_path {
                let _ = write_markdown_report(rp, &spec, &Ok(outcome));
            }
            ExitCode::SUCCESS
        }
        Err(boxed_err) => {
            let (failure, partial_traces) = *boxed_err;
            if let Some(tp) = trace_path {
                let _ = write_ndjson_trace(tp, &partial_traces);
            }
            if let Some(rp) = report_path {
                let _ = write_markdown_report(rp, &spec, &Err(failure.clone()));
            }
            eprintln!("{}", failure.format_compact_block());
            ExitCode::FAILURE
        }
    }
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
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Commands::RunScenario {
            scenario_path,
            trace,
            report,
            current_sprint,
        }) => {
            run_scenario_command(
                &scenario_path,
                trace.as_deref(),
                report.as_deref(),
                current_sprint,
                &cli.auth_addr,
                &cli.irc_addr,
                &cli.ca_path,
            )
            .await
        }
        None => {
            // Standard-Verhalten ohne Subcommand (Kompatibilität zu `docker-compose.yml` Profil `test`):
            // führt `harness/scenarios/00-smoke-mesh.toml` aus, falls vorhanden, sonst eingebettetes Äquivalent.
            let default_path = Path::new("harness/scenarios/00-smoke-mesh.toml");
            if default_path.exists() {
                run_scenario_command(
                    default_path,
                    None,
                    None,
                    1,
                    &cli.auth_addr,
                    &cli.irc_addr,
                    &cli.ca_path,
                )
                .await
            } else {
                let inline = include_str!("../../../harness/scenarios/00-smoke-mesh.toml");
                let Ok(spec) = ScenarioSpec::from_toml_str(inline) else {
                    eprintln!("FAILED_STEP: parse embedded 00-smoke-mesh.toml");
                    return ExitCode::FAILURE;
                };
                match execute_e2e_scenario(&spec, 1, &cli.auth_addr, &cli.irc_addr, &cli.ca_path)
                    .await
                {
                    Ok(_) => ExitCode::SUCCESS,
                    Err(boxed_err) => {
                        eprintln!("{}", boxed_err.0.format_compact_block());
                        ExitCode::FAILURE
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_localhost_fallback_port;
    use ash_core::scenario::{execute_sim_scenario, ScenarioRunOutcome, ScenarioSpec};

    #[test]
    fn maps_docker_service_endpoint_to_localhost_port() {
        let addr = parse_localhost_fallback_port("game-node-frontier:4434").unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:4434");
    }

    #[test]
    fn executes_01_sim_invariants_toml_deterministically() {
        let raw = include_str!("../../../harness/scenarios/01-sim-invariants.toml");
        let spec = ScenarioSpec::from_toml_str(raw).expect("valid 01-sim-invariants.toml");
        let outcome = execute_sim_scenario(&spec, 1).expect("sim scenario must pass");
        assert!(
            matches!(outcome, ScenarioRunOutcome::Passed { steps_executed, .. } if steps_executed >= 8)
        );
    }

    #[test]
    fn skips_future_sprint_templates_cleanly() {
        for raw in [
            include_str!("../../../harness/scenarios/00-smoke-mesh.toml"),
            include_str!("../../../harness/scenarios/01-sim-invariants.toml"),
            include_str!("../../../harness/scenarios/02-combat-4role.template.toml"),
            include_str!("../../../harness/scenarios/03-roadhouse-quests.template.toml"),
            include_str!("../../../harness/scenarios/04-raid-failover.template.toml"),
        ] {
            ScenarioSpec::from_toml_str(raw).expect("all harness TOML scenarios must parse");
        }

        let raw = include_str!("../../../harness/scenarios/02-combat-4role.template.toml");
        let spec = ScenarioSpec::from_toml_str(raw).expect("valid template TOML");
        let outcome = execute_sim_scenario(&spec, 1).expect("skip is ok");
        assert!(matches!(
            outcome,
            ScenarioRunOutcome::Skipped {
                min_sprint: 2,
                current_sprint: 1,
                ..
            }
        ));
    }
}
