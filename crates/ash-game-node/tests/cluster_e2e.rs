// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use ash_auth_gateway::{start_auth_gateway, AuthGatewayConfig};
use ash_core::character::{CharacterClass, CharacterSheet, PlayableRace, STARTING_ZONE_ROADHOUSE};
use ash_core::equipment::{EquipmentSlot, EssenceType, ItemQuality};
use ash_db::transport::{build_quic_client, generate_ash_pki, rpc_call};
use ash_db::AshStore;
use ash_game_node::{start_game_node, GameNodeConfig, NodeRole};
use ash_irc_bridge::{spawn_irc_bridge, IrcBridgeConfig};
use ash_protocol::client::{
    AuthRequest, AuthResponse, ClientDatagram, ClientReliableMessage, CombatActionNet,
    ServerDatagram, ServerReliableMessage,
};
use ash_protocol::codec::{decode_datagram, encode_datagram};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use uuid::Uuid;

#[derive(Default)]
struct MiniIrcState {
    clients: HashMap<usize, (String, mpsc::UnboundedSender<String>)>,
    channels: HashMap<String, HashSet<usize>>,
    next_id: usize,
}

async fn start_embedded_mini_ircd(bind_addr: SocketAddr) -> Result<SocketAddr> {
    let listener = TcpListener::bind(bind_addr).await?;
    let local = listener.local_addr()?;
    let state = Arc::new(Mutex::new(MiniIrcState::default()));

    tokio::spawn(async move {
        while let Ok((stream, _peer)) = listener.accept().await {
            let st = state.clone();
            tokio::spawn(async move {
                let (r, mut w) = stream.into_split();
                let (tx, mut rx) = mpsc::unbounded_channel::<String>();

                let client_id = {
                    let mut g = st.lock().await;
                    let id = g.next_id;
                    g.next_id += 1;
                    g.clients.insert(id, (format!("guest{id}"), tx.clone()));
                    id
                };

                tokio::spawn(async move {
                    while let Some(line) = rx.recv().await {
                        let out = format!("{line}\r\n");
                        if w.write_all(out.as_bytes()).await.is_err() {
                            break;
                        }
                        let _ = w.flush().await;
                    }
                });

                let mut reader = BufReader::new(r).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let trimmed = line.trim();
                    if let Some(nick) = trimmed.strip_prefix("NICK ") {
                        let nick = nick.trim().to_string();
                        if let Some(entry) = st.lock().await.clients.get_mut(&client_id) {
                            entry.0 = nick;
                        }
                    } else if trimmed.starts_with("USER ") {
                        let nick = st
                            .lock()
                            .await
                            .clients
                            .get(&client_id)
                            .map(|(n, _)| n.clone())
                            .unwrap_or_default();
                        let _ = tx.send(format!(":ash.irc 001 {nick} :Welcome to Ash-Net IRC"));
                    } else if let Some(rest) = trimmed.strip_prefix("PING ") {
                        let _ = tx.send(format!("PONG {rest}"));
                    } else if let Some(chan) = trimmed.strip_prefix("JOIN ") {
                        let ch_name = chan
                            .split_whitespace()
                            .next()
                            .unwrap_or("#roadhouse")
                            .to_string();
                        let mut g = st.lock().await;
                        let nick = g
                            .clients
                            .get(&client_id)
                            .map(|(n, _)| n.clone())
                            .unwrap_or_default();
                        g.channels
                            .entry(ch_name.clone())
                            .or_default()
                            .insert(client_id);
                        if let Some(members) = g.channels.get(&ch_name).cloned() {
                            for mid in members {
                                if let Some((_, mtx)) = g.clients.get(&mid) {
                                    let _ = mtx.send(format!(":{nick}!u@ash JOIN {ch_name}"));
                                }
                            }
                        }
                    } else if let Some(rest) = trimmed.strip_prefix("PRIVMSG ") {
                        if let Some((target, msg)) = rest.split_once(" :") {
                            let target = target.trim();
                            let g = st.lock().await;
                            let sender_nick = g
                                .clients
                                .get(&client_id)
                                .map(|(n, _)| n.clone())
                                .unwrap_or_default();
                            let wire = format!(":{sender_nick}!u@ash PRIVMSG {target} :{msg}");
                            if target.starts_with('#') {
                                if let Some(members) = g.channels.get(target) {
                                    for &mid in members {
                                        if mid != client_id {
                                            if let Some((_, mtx)) = g.clients.get(&mid) {
                                                let _ = mtx.send(wire.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let mut g = st.lock().await;
                g.clients.remove(&client_id);
                for members in g.channels.values_mut() {
                    members.remove(&client_id);
                }
            });
        }
    });

    Ok(local)
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_end_to_end_mesh_handover_and_irc_smoke_test() -> Result<()> {
    let pki = generate_ash_pki()?;
    let ca_bytes = pki.ca_cert_pem.into_bytes();
    let cert_bytes = pki.node_cert_pem.into_bytes();
    let key_bytes = pki.node_key_pem.into_bytes();

    let store = AshStore::new_shared_in_memory();
    let irc_sock = start_embedded_mini_ircd("127.0.0.1:0".parse()?).await?;

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

    let _frontier = start_game_node(
        GameNodeConfig {
            node_id: "game-node-frontier".into(),
            role: NodeRole::Mirror,
            zones: vec!["zone_new_lawrence_west".into(), "zone_salzminen".into()],
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

    let _bridge = spawn_irc_bridge(IrcBridgeConfig {
        irc_addr: irc_sock.to_string(),
        bot_nick: "AshBridge".into(),
        oper_user: None,
        oper_password: None,
        hub_mesh_addr,
        ca_pem: ca_bytes.clone(),
        cert_pem: cert_bytes,
        key_pem: key_bytes,
    })?;

    tokio::time::sleep(Duration::from_millis(600)).await;

    let client_ep = build_quic_client("0.0.0.0:0".parse()?, &ca_bytes, None)?;
    let auth_conn = client_ep
        .connect(auth_ep.local_addr()?, "ash-node")?
        .await?;

    let suffix = Uuid::new_v4().simple().to_string()[..6].to_string();
    let hunter_name = format!("Winchester_{suffix}");

    let AuthResponse::AccountRegistered { session_token, .. } =
        rpc_call::<AuthRequest, AuthResponse>(
            &auth_conn,
            &AuthRequest::RegisterAccount {
                username: format!("dean_{suffix}"),
                password: "impala_1967_password".into(),
            },
        )
        .await?
    else {
        bail!("AccountRegistered failed");
    };

    for forbidden in ["Engel", "Dämon"] {
        let resp = rpc_call::<AuthRequest, AuthResponse>(
            &auth_conn,
            &AuthRequest::CreateCharacter {
                session_token: session_token.clone(),
                name: format!("Bad_{forbidden}"),
                race: forbidden.into(),
                class: "Winchester-Sniper".into(),
            },
        )
        .await?;
        assert!(matches!(
            resp,
            AuthResponse::Error { code, .. } if code == "FORBIDDEN_OVERPOWER_RACE"
        ));
    }

    let AuthResponse::CharacterCreated { character: hunter } =
        rpc_call::<AuthRequest, AuthResponse>(
            &auth_conn,
            &AuthRequest::CreateCharacter {
                session_token: session_token.clone(),
                name: hunter_name.clone(),
                race: "Winchester-Abkömmling".into(),
                class: "Winchester-Sniper".into(),
            },
        )
        .await?
    else {
        bail!("CharacterCreated failed");
    };

    assert_eq!(hunter.current_zone, STARTING_ZONE_ROADHOUSE);
    assert_eq!(hunter.equipment.slots.len(), 16);

    let AuthResponse::ZoneTicketIssued { ticket } = rpc_call::<AuthRequest, AuthResponse>(
        &auth_conn,
        &AuthRequest::IssueZoneTicket {
            session_token,
            character_id: hunter.id,
        },
    )
    .await?
    else {
        bail!("ZoneTicketIssued failed");
    };

    let roadhouse_conn = client_ep
        .connect(ticket.target_client_addr.parse()?, "ash-node")?
        .await?;

    let ServerReliableMessage::ZoneEntered { zone_id, npcs, .. } =
        rpc_call::<ClientReliableMessage, ServerReliableMessage>(
            &roadhouse_conn,
            &ClientReliableMessage::EnterZoneWithTicket {
                ticket_token: ticket.ticket_token,
            },
        )
        .await?
    else {
        bail!("ZoneEntered failed");
    };
    assert_eq!(zone_id, "zone_roadhouse");
    assert!(npcs.len() >= 3);

    roadhouse_conn.send_datagram(encode_datagram(&ClientDatagram::MoveInput {
        character_id: hunter.id,
        seq: 1,
        pos_x: 5.0,
        pos_y: 2.0,
    })?)?;
    let raw_snap = timeout(Duration::from_secs(3), roadhouse_conn.read_datagram()).await??;
    let ServerDatagram::ZoneSnapshot { zone_id, .. } = decode_datagram(&raw_snap)?;
    assert_eq!(zone_id, "zone_roadhouse");

    for npc_id in ["npc_jo_harvelle", "npc_ellen_harvelle", "obj_ash_terminal"] {
        let resp = rpc_call::<ClientReliableMessage, ServerReliableMessage>(
            &roadhouse_conn,
            &ClientReliableMessage::InteractNpc {
                character_id: hunter.id,
                npc_id: npc_id.into(),
            },
        )
        .await?;
        assert!(
            matches!(resp, ServerReliableMessage::NpcInteractionResult { quests, .. } if !quests.is_empty())
        );
    }

    let ServerReliableMessage::AshTerminalStatus { report } =
        rpc_call::<ClientReliableMessage, ServerReliableMessage>(
            &roadhouse_conn,
            &ClientReliableMessage::QueryAshTerminal {
                character_id: hunter.id,
            },
        )
        .await?
    else {
        bail!("QueryAshTerminal failed");
    };
    assert!(report.active_nodes.len() >= 2);

    let irc_stream = TcpStream::connect(irc_sock).await?;
    let (irc_r, mut irc_w) = irc_stream.into_split();
    let mut irc_lines = BufReader::new(irc_r).lines();
    irc_w
        .write_all(
            format!(
                "NICK {hunter_name}\r\nUSER hunter 0 * :Hunter\r\nJOIN #roadhouse\r\nJOIN #new_lawrence\r\n"
            )
            .as_bytes(),
        )
        .await?;
    irc_w.flush().await?;
    tokio::time::sleep(Duration::from_millis(300)).await;

    irc_w
        .write_all(b"PRIVMSG #roadhouse :/salzen an der Roadhouse-Schwelle\r\n")
        .await?;
    irc_w.flush().await?;
    wait_for_irc_substring(&mut irc_lines, &mut irc_w, "ECS-SCHUTZLINIE").await?;

    let ServerReliableMessage::PlayerHandoverRedirect { redirect } =
        rpc_call::<ClientReliableMessage, ServerReliableMessage>(
            &roadhouse_conn,
            &ClientReliableMessage::MoveAndCheckBoundary {
                character_id: hunter.id,
                pos_x: 105.0,
                pos_y: 0.0,
            },
        )
        .await?
    else {
        bail!("PlayerHandoverRedirect expected");
    };
    assert_eq!(redirect.target_zone, "zone_new_lawrence_west");

    let frontier_conn = client_ep
        .connect(redirect.target_client_addr.parse()?, "ash-node")?
        .await?;

    let ServerReliableMessage::HandoverCompleted {
        zone_id, character, ..
    } = rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &frontier_conn,
        &ClientReliableMessage::AcceptHandover {
            handover_token: redirect.handover_token,
        },
    )
    .await?
    else {
        bail!("HandoverCompleted expected");
    };
    assert_eq!(zone_id, "zone_new_lawrence_west");
    assert!(character.salt_ward_active);

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

    let ServerReliableMessage::EquipmentUpgraded { character, .. } =
        rpc_call::<ClientReliableMessage, ServerReliableMessage>(
            &frontier_conn,
            &ClientReliableMessage::UpgradeEquipment {
                character_id: hunter.id,
                slot: EquipmentSlot::MainHand,
                essence: EssenceType::RockSaltAsh,
            },
        )
        .await?
    else {
        bail!("EquipmentUpgraded expected");
    };
    assert_eq!(
        character
            .equipment
            .get(EquipmentSlot::MainHand)
            .unwrap()
            .quality,
        ItemQuality::Green
    );

    let ServerReliableMessage::CombatOutcome {
        enemy_banished: true,
        ..
    } = rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &frontier_conn,
        &ClientReliableMessage::PerformCombat {
            character_id: hunter.id,
            action: CombatActionNet::CastBanishSheep,
        },
    )
    .await?
    else {
        bail!("CastBanishSheep expected");
    };

    let _ = rpc_call::<ClientReliableMessage, ServerReliableMessage>(
        &frontier_conn,
        &ClientReliableMessage::TriggerZoneOmen {
            zone_id: "zone_new_lawrence_west".into(),
            new_omen_level: 2,
            reason: "Schwefelriss aufgebrochen".into(),
        },
    )
    .await?;
    wait_for_irc_substring(&mut irc_lines, &mut irc_w, "OMEN-STUFE 2").await?;

    auth_conn.close(0u32.into(), b"done");
    roadhouse_conn.close(0u32.into(), b"done");
    frontier_conn.close(0u32.into(), b"done");
    Ok(())
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
