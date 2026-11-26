// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use ash_core::character::CharacterSheet;
use ash_core::combat::{
    apply_banish_sheep, cast_eigenblut_spende, cast_pyromancer_fire_spell,
    cast_pyromancer_wildfire, EnemyCombatState, PyromancerFireSpell, TankAggroTracker,
};
use ash_core::equipment::{EssenceSource, EssenceType};
use ash_core::world::{
    IrcSlashCommand, MalphasEncounter, NpcDefinition, OmenIndex, QuestCatalog, SarielEncounter,
    ZoneCatalog, ZoneDefinition,
};
use ash_db::{now_epoch_ms, AshStore};
use ash_protocol::framing::{
    decode_datagram, encode_datagram, read_frame, rpc_call, send_response_and_close,
};
use ash_protocol::messages::{
    AshTerminalReport, ClientDatagram, ClientReliableMessage, ClusterNodeInfo, CombatActionNet,
    EntityPositionNet, HandoverRedirectInfo, MeshMessage, MeshResponse, OmenAlertEvent,
    PlayerHandoverState, QuestStateDto, ServerDatagram, ServerReliableMessage, ZoneRouteInfo,
};
use ash_protocol::tls::{build_quic_client, build_quic_server};
use hecs::{Entity, World};
use quinn::{Connection, Endpoint};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRole {
    Hub,
    Mirror,
}

impl NodeRole {
    pub fn parse(raw: &str) -> Self {
        if raw.trim().eq_ignore_ascii_case("hub") {
            Self::Hub
        } else {
            Self::Mirror
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Hub => "hub",
            Self::Mirror => "mirror",
        }
    }
}

#[derive(Clone)]
pub struct GameNodeConfig {
    pub node_id: String,
    pub role: NodeRole,
    pub zones: Vec<String>,
    pub client_bind_addr: SocketAddr,
    pub client_public_addr: String,
    pub mesh_bind_addr: SocketAddr,
    pub mesh_public_addr: String,
    pub hub_mesh_addr: Option<String>,
    pub tick_rate_hz: u32,
    pub ca_pem: Vec<u8>,
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

struct PlayerEcsComp {
    sheet: CharacterSheet,
}

struct PosComp {
    x: f32,
    y: f32,
}

struct ZoneRuntime {
    def: ZoneDefinition,
    world: World,
    player_entities: HashMap<Uuid, Entity>,
    player_conns: HashMap<Uuid, Connection>,
    pending_handovers: HashMap<Uuid, HandoverRedirectInfo>,
    omen: OmenIndex,
    aggro: TankAggroTracker,
    enemies: Vec<EnemyCombatState>,
    malphas: Option<MalphasEncounter>,
    sariel: Option<SarielEncounter>,
    tick: u64,
}

impl ZoneRuntime {
    fn new(def: ZoneDefinition) -> Self {
        let omen = OmenIndex::from_u8(def.default_omen);
        let malphas = if def.raid_boss.as_deref() == Some("malphas") {
            Some(MalphasEncounter::new())
        } else {
            None
        };
        let sariel = if def.raid_boss.as_deref() == Some("sariel") {
            Some(SarielEncounter::new())
        } else {
            None
        };
        let enemies = if def.safe_sanctuary {
            Vec::new()
        } else {
            vec![
                EnemyCombatState::new("Besessener Kolonist", 900, false, 15.0, 2.0),
                EnemyCombatState::new("Höllenhund-Späher", 1100, false, 17.0, -1.0),
                EnemyCombatState::new("Schwefel-Kultist", 850, false, 18.0, 1.5),
                EnemyCombatState::new("Riss-Schemen", 750, false, 19.0, 0.0),
            ]
        };

        Self {
            def,
            world: World::new(),
            player_entities: HashMap::new(),
            player_conns: HashMap::new(),
            pending_handovers: HashMap::new(),
            omen,
            aggro: TankAggroTracker::default(),
            enemies,
            malphas,
            sariel,
            tick: 0,
        }
    }

    fn spawn_or_update_player(&mut self, sheet: CharacterSheet, conn: Option<Connection>) {
        let cid = sheet.id;
        let px = sheet.pos_x;
        let py = sheet.pos_y;
        if let Some(&entity) = self.player_entities.get(&cid) {
            if let Ok(mut p) = self.world.get::<&mut PlayerEcsComp>(entity) {
                p.sheet = sheet;
            }
            if let Ok(mut pos) = self.world.get::<&mut PosComp>(entity) {
                pos.x = px;
                pos.y = py;
            }
        } else {
            let entity = self
                .world
                .spawn((PlayerEcsComp { sheet }, PosComp { x: px, y: py }));
            self.player_entities.insert(cid, entity);
        }
        if let Some(c) = conn {
            self.player_conns.insert(cid, c);
        }
    }

    fn remove_player(&mut self, cid: Uuid) -> Option<CharacterSheet> {
        self.player_conns.remove(&cid);
        if let Some(entity) = self.player_entities.remove(&cid) {
            let sheet = self
                .world
                .get::<&PlayerEcsComp>(entity)
                .ok()
                .map(|p| p.sheet.clone());
            let _ = self.world.despawn(entity);
            return sheet;
        }
        None
    }
}

struct NodeSharedState {
    cfg: GameNodeConfig,
    store: AshStore,
    zone_catalog: ZoneCatalog,
    quest_catalog: QuestCatalog,
    zones: HashMap<String, ZoneRuntime>,
    cluster_nodes: HashMap<String, ClusterNodeInfo>,
    cluster_routes: HashMap<String, ZoneRouteInfo>,
    staged_handovers: HashMap<String, PlayerHandoverState>,
    omen_alerts: Vec<OmenAlertEvent>,
    next_alert_seq: u64,
    mesh_client: Endpoint,
}

pub struct RunningGameNode {
    pub client_endpoint: Endpoint,
    pub mesh_endpoint: Endpoint,
}

async fn resolve_socket_addr(addr_str: &str) -> Result<SocketAddr> {
    tokio::net::lookup_host(addr_str)
        .await
        .with_context(|| format!("resolve address '{addr_str}'"))?
        .next()
        .with_context(|| format!("no socket addr resolved for '{addr_str}'"))
}

pub async fn start_game_node(mut cfg: GameNodeConfig, store: AshStore) -> Result<RunningGameNode> {
    cfg.tick_rate_hz = cfg.tick_rate_hz.clamp(20, 30);

    let client_ep = build_quic_server(cfg.client_bind_addr, &cfg.cert_pem, &cfg.key_pem, None)?;
    let mesh_ep = build_quic_server(
        cfg.mesh_bind_addr,
        &cfg.cert_pem,
        &cfg.key_pem,
        Some(&cfg.ca_pem),
    )?;
    let mesh_client = build_quic_client(
        "0.0.0.0:0".parse().unwrap(),
        &cfg.ca_pem,
        Some((&cfg.cert_pem, &cfg.key_pem)),
    )?;

    // Falls Port 0 in Tests gebunden wurde, echte Adresse übernehmen
    if cfg.client_public_addr.ends_with(":0") {
        cfg.client_public_addr = format!("127.0.0.1:{}", client_ep.local_addr()?.port());
    }
    if cfg.mesh_public_addr.ends_with(":0") {
        cfg.mesh_public_addr = format!("127.0.0.1:{}", mesh_ep.local_addr()?.port());
    }

    let zone_catalog = ZoneCatalog::load_embedded()?;
    let quest_catalog = QuestCatalog::load_embedded()?;

    let mut zones_map = HashMap::new();
    let mut initial_routes = HashMap::new();
    let now = now_epoch_ms();

    for zid in &cfg.zones {
        let def = zone_catalog
            .find_zone(zid)
            .cloned()
            .unwrap_or(ZoneDefinition {
                id: zid.clone(),
                name: zid.clone(),
                irc_channel: "#new_lawrence".into(),
                safe_sanctuary: false,
                default_omen: 0,
                boundary_west_x: None,
                boundary_west_target: None,
                boundary_west_spawn_x: None,
                boundary_west_spawn_y: None,
                boundary_east_x: None,
                boundary_east_target: None,
                boundary_east_spawn_x: None,
                boundary_east_spawn_y: None,
                raid_boss: None,
                description: "Dynamisch gespiegelte Ash-Net-Zone".into(),
                npcs: Vec::new(),
            });
        let route = ZoneRouteInfo {
            zone_id: zid.clone(),
            node_id: cfg.node_id.clone(),
            client_public_addr: cfg.client_public_addr.clone(),
            mesh_addr: cfg.mesh_public_addr.clone(),
            omen_level: def.default_omen,
            last_heartbeat_ms: now,
        };
        store.publish_zone_route(&route).await?;
        initial_routes.insert(zid.clone(), route);
        zones_map.insert(zid.clone(), ZoneRuntime::new(def));
    }

    let self_node_info = ClusterNodeInfo {
        node_id: cfg.node_id.clone(),
        role: cfg.role.as_str().to_string(),
        client_public_addr: cfg.client_public_addr.clone(),
        mesh_addr: cfg.mesh_public_addr.clone(),
        zones: cfg.zones.clone(),
        last_heartbeat_ms: now,
    };
    let mut cluster_nodes = HashMap::new();
    cluster_nodes.insert(cfg.node_id.clone(), self_node_info);

    let state = Arc::new(Mutex::new(NodeSharedState {
        cfg: cfg.clone(),
        store,
        zone_catalog,
        quest_catalog,
        zones: zones_map,
        cluster_nodes,
        cluster_routes: initial_routes,
        staged_handovers: HashMap::new(),
        omen_alerts: Vec::new(),
        next_alert_seq: 1,
        mesh_client,
    }));

    info!(
        "ash-game-node '{}' ({}) hosting {:?} | client={} | mTLS-mesh={} | {} Hz",
        cfg.node_id,
        cfg.role.as_str(),
        cfg.zones,
        client_ep.local_addr()?,
        mesh_ep.local_addr()?,
        cfg.tick_rate_hz
    );

    // 1. mTLS Mesh Listener
    {
        let mesh_accept = mesh_ep.clone();
        let st = state.clone();
        tokio::spawn(async move {
            while let Some(incoming) = mesh_accept.accept().await {
                let st = st.clone();
                tokio::spawn(async move {
                    if let Ok(conn) = incoming.await {
                        while let Ok((send, mut recv)) = conn.accept_bi().await {
                            let st = st.clone();
                            tokio::spawn(async move {
                                if let Ok(msg) = read_frame::<MeshMessage>(&mut recv).await {
                                    let resp = handle_mesh_message(msg, st).await;
                                    let _ = send_response_and_close(send, &resp).await;
                                }
                            });
                        }
                    }
                });
            }
        });
    }

    // 2. Client QUIC Listener (Reliable Streams + Unreliable Datagrams)
    {
        let client_accept = client_ep.clone();
        let st = state.clone();
        tokio::spawn(async move {
            while let Some(incoming) = client_accept.accept().await {
                let st = st.clone();
                tokio::spawn(async move {
                    match incoming.await {
                        Ok(conn) => {
                            // Unreliable Datagram Reader Loop
                            let dgram_conn = conn.clone();
                            let dgram_st = st.clone();
                            tokio::spawn(async move {
                                while let Ok(raw) = dgram_conn.read_datagram().await {
                                    if let Ok(ClientDatagram::MoveInput {
                                        character_id,
                                        pos_x,
                                        pos_y,
                                        ..
                                    }) = decode_datagram::<ClientDatagram>(&raw)
                                    {
                                        let _ = process_player_move(
                                            character_id,
                                            pos_x,
                                            pos_y,
                                            dgram_st.clone(),
                                        )
                                        .await;
                                    }
                                }
                            });

                            // Reliable Bi-Stream Loop
                            while let Ok((send, mut recv)) = conn.accept_bi().await {
                                let st = st.clone();
                                let c = conn.clone();
                                tokio::spawn(async move {
                                    if let Ok(msg) =
                                        read_frame::<ClientReliableMessage>(&mut recv).await
                                    {
                                        let resp = handle_client_reliable(msg, c, st).await;
                                        let _ = send_response_and_close(send, &resp).await;
                                    }
                                });
                            }
                        }
                        Err(e) => warn!("client QUIC handshake failed: {e:#}"),
                    }
                });
            }
        });
    }

    // 3. Headless ECS Fixed-Tick Simulation Loop (20-30 Hz)
    {
        let st = state.clone();
        let tick_ms = (1000 / cfg.tick_rate_hz).max(33) as u64;
        let dt_secs = 1.0 / (cfg.tick_rate_hz as f32);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(tick_ms));
            loop {
                ticker.tick().await;
                let mut guard = st.lock().await;
                let now = now_epoch_ms();
                let node_id = guard.cfg.node_id.clone();
                let client_pub = guard.cfg.client_public_addr.clone();
                let mesh_pub = guard.cfg.mesh_public_addr.clone();
                let store = guard.store.clone();

                let mut routes_to_refresh = Vec::new();
                for z in guard.zones.values_mut() {
                    z.tick += 1;
                    for e in &mut z.enemies {
                        e.tick(dt_secs);
                    }
                    if let Some(m) = &mut z.malphas {
                        m.tick(dt_secs);
                    }
                    if let Some(s) = &mut z.sariel {
                        s.tick(dt_secs);
                    }

                    // Sammle Entitäten für Unreliable ZoneSnapshot Datagramm
                    let mut entities = Vec::new();
                    for (_, (p, pos)) in z.world.query_mut::<(&PlayerEcsComp, &PosComp)>() {
                        entities.push(EntityPositionNet {
                            id: p.sheet.id,
                            name: p.sheet.name.clone(),
                            pos_x: pos.x,
                            pos_y: pos.y,
                            hp: p.sheet.hp,
                        });
                    }

                    for (pid, conn) in &z.player_conns {
                        let snap = ServerDatagram::ZoneSnapshot {
                            tick: z.tick,
                            zone_id: z.def.id.clone(),
                            omen_level: z.omen.level(),
                            entities: entities.clone(),
                            pending_handover: z.pending_handovers.get(pid).cloned(),
                        };
                        if let Ok(bytes) = encode_datagram(&snap) {
                            let _ = conn.send_datagram(bytes);
                        }
                    }

                    if z.tick % (2 * 25) == 0 {
                        routes_to_refresh.push(ZoneRouteInfo {
                            zone_id: z.def.id.clone(),
                            node_id: node_id.clone(),
                            client_public_addr: client_pub.clone(),
                            mesh_addr: mesh_pub.clone(),
                            omen_level: z.omen.level(),
                            last_heartbeat_ms: now,
                        });
                    }
                }
                for r in &routes_to_refresh {
                    guard.cluster_routes.insert(r.zone_id.clone(), r.clone());
                }
                if let Some(self_node) = guard.cluster_nodes.get_mut(&node_id) {
                    self_node.last_heartbeat_ms = now;
                }
                drop(guard);

                for r in routes_to_refresh {
                    let _ = store.publish_zone_route(&r).await;
                }
            }
        });
    }

    // 4. Mirror Gossip Sync Loop (hält persistente mTLS-QUIC-Verbindung zum Hub)
    if cfg.role == NodeRole::Mirror {
        if let Some(hub_addr_str) = cfg.hub_mesh_addr.clone() {
            let st = state.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                let mut registered = false;
                let mut cached_hub_conn: Option<Connection> = None;
                loop {
                    interval.tick().await;
                    let (mesh_client, announce_msg, hb_msg) = {
                        let g = st.lock().await;
                        let omens: Vec<(String, u8)> = g
                            .zones
                            .values()
                            .map(|z| (z.def.id.clone(), z.omen.level()))
                            .collect();
                        let node_info = ClusterNodeInfo {
                            node_id: g.cfg.node_id.clone(),
                            role: g.cfg.role.as_str().to_string(),
                            client_public_addr: g.cfg.client_public_addr.clone(),
                            mesh_addr: g.cfg.mesh_public_addr.clone(),
                            zones: g.cfg.zones.clone(),
                            last_heartbeat_ms: now_epoch_ms(),
                        };
                        (
                            g.mesh_client.clone(),
                            MeshMessage::GossipAnnounce {
                                node: node_info,
                                zone_omens: omens.clone(),
                            },
                            MeshMessage::Heartbeat {
                                node_id: g.cfg.node_id.clone(),
                                zone_omens: omens,
                            },
                        )
                    };

                    if cached_hub_conn
                        .as_ref()
                        .is_none_or(|c| c.close_reason().is_some())
                    {
                        registered = false;
                        cached_hub_conn = None;
                        if let Ok(hub_sock) = resolve_socket_addr(&hub_addr_str).await {
                            if let Ok(connecting) = mesh_client.connect(hub_sock, "ash-node") {
                                if let Ok(conn) = connecting.await {
                                    cached_hub_conn = Some(conn);
                                }
                            }
                        }
                    }

                    if let Some(conn) = cached_hub_conn.as_ref() {
                        let req = if !registered { announce_msg } else { hb_msg };
                        match rpc_call::<MeshMessage, MeshResponse>(conn, &req).await {
                            Ok(MeshResponse::RoutingTable { nodes, routes }) => {
                                registered = true;
                                let mut g = st.lock().await;
                                for n in nodes {
                                    g.cluster_nodes.insert(n.node_id.clone(), n);
                                }
                                for r in routes {
                                    g.cluster_routes.insert(r.zone_id.clone(), r);
                                }
                            }
                            Err(_) => {
                                cached_hub_conn = None;
                                registered = false;
                            }
                            _ => {}
                        }
                    }
                }
            });
        }
    }

    Ok(RunningGameNode {
        client_endpoint: client_ep,
        mesh_endpoint: mesh_ep,
    })
}

async fn handle_mesh_message(msg: MeshMessage, st: Arc<Mutex<NodeSharedState>>) -> MeshResponse {
    match msg {
        MeshMessage::GossipAnnounce { node, zone_omens } => {
            let mut g = st.lock().await;
            let now = now_epoch_ms();
            info!(
                "Mesh Gossip: Node '{}' mirrored zones {:?}",
                node.node_id, node.zones
            );
            let mut to_publish = Vec::new();
            for z_id in &node.zones {
                let omen = zone_omens
                    .iter()
                    .find(|(id, _)| id == z_id)
                    .map(|(_, o)| *o)
                    .unwrap_or(0);
                let route = ZoneRouteInfo {
                    zone_id: z_id.clone(),
                    node_id: node.node_id.clone(),
                    client_public_addr: node.client_public_addr.clone(),
                    mesh_addr: node.mesh_addr.clone(),
                    omen_level: omen,
                    last_heartbeat_ms: now,
                };
                g.cluster_routes.insert(z_id.clone(), route.clone());
                to_publish.push(route);
            }
            g.cluster_nodes.insert(node.node_id.clone(), node);
            let nodes: Vec<_> = g.cluster_nodes.values().cloned().collect();
            let routes: Vec<_> = g.cluster_routes.values().cloned().collect();
            let store = g.store.clone();
            drop(g);
            for r in to_publish {
                let _ = store.publish_zone_route(&r).await;
            }
            MeshResponse::RoutingTable { nodes, routes }
        }
        MeshMessage::Heartbeat {
            node_id,
            zone_omens,
        } => {
            let mut g = st.lock().await;
            let now = now_epoch_ms();
            if let Some(n) = g.cluster_nodes.get_mut(&node_id) {
                n.last_heartbeat_ms = now;
            }
            let mut to_publish = Vec::new();
            for (zid, omen) in zone_omens {
                if let Some(r) = g.cluster_routes.get_mut(&zid) {
                    r.omen_level = omen;
                    r.last_heartbeat_ms = now;
                    to_publish.push(r.clone());
                }
            }
            let nodes: Vec<_> = g.cluster_nodes.values().cloned().collect();
            let routes: Vec<_> = g.cluster_routes.values().cloned().collect();
            let store = g.store.clone();
            drop(g);
            for r in to_publish {
                let _ = store.publish_zone_route(&r).await;
            }
            MeshResponse::RoutingTable { nodes, routes }
        }
        MeshMessage::QueryRoutingTable => {
            let g = st.lock().await;
            let now = now_epoch_ms();
            let nodes = g
                .cluster_nodes
                .values()
                .filter(|n| now.saturating_sub(n.last_heartbeat_ms) <= 12_000)
                .cloned()
                .collect();
            let routes = g
                .cluster_routes
                .values()
                .filter(|r| now.saturating_sub(r.last_heartbeat_ms) <= 12_000)
                .cloned()
                .collect();
            MeshResponse::RoutingTable { nodes, routes }
        }
        MeshMessage::PlayerHandoverPrepare {
            handover_token,
            state,
        } => {
            let mut g = st.lock().await;
            if !g.zones.contains_key(&state.target_zone) {
                return MeshResponse::PlayerHandoverAck {
                    accepted: false,
                    client_public_addr: String::new(),
                };
            }
            info!(
                "Mesh Handover Prepare: staging '{}' for zone '{}'",
                state.character.name, state.target_zone
            );
            let addr = g.cfg.client_public_addr.clone();
            g.staged_handovers.insert(handover_token, state);
            MeshResponse::PlayerHandoverAck {
                accepted: true,
                client_public_addr: addr,
            }
        }
        MeshMessage::IrcSlashEvent {
            irc_nick,
            character_name,
            zone_id,
            command,
            ..
        } => execute_irc_slash_in_ecs(irc_nick, character_name, zone_id, command, st).await,
        MeshMessage::PollOmenAlerts { since_seq } => {
            let g = st.lock().await;
            let alerts = g
                .omen_alerts
                .iter()
                .filter(|a| a.seq > since_seq)
                .cloned()
                .collect();
            MeshResponse::OmenAlerts { alerts }
        }
        MeshMessage::BroadcastOmenAlert {
            zone_id,
            omen_level,
            irc_alert_line,
        } => {
            let mut g = st.lock().await;
            let seq = g.next_alert_seq;
            g.next_alert_seq += 1;
            g.omen_alerts.push(OmenAlertEvent {
                seq,
                zone_id,
                omen_level,
                irc_alert_line,
            });
            MeshResponse::Ack
        }
    }
}

async fn execute_irc_slash_in_ecs(
    irc_nick: String,
    character_name: String,
    zone_id: String,
    command: IrcSlashCommand,
    st: Arc<Mutex<NodeSharedState>>,
) -> MeshResponse {
    let g = st.lock().await;
    let store = g.store.clone();
    let quest_cat = g.quest_catalog.clone();

    let omen_level = g
        .zones
        .get(&zone_id)
        .map(|z| z.omen.level())
        .unwrap_or(0);
    let omen = OmenIndex::from_u8(omen_level);

    // Versuche aktiven Spieler in der Zone oder in der DB zu finden
    let lookup_name = if character_name.is_empty() {
        irc_nick.clone()
    } else {
        character_name
    };

    let mut char_opt: Option<CharacterSheet> = None;
    if let Some(zr) = g.zones.get(&zone_id) {
        for (_, (p, _)) in zr.world.query::<(&PlayerEcsComp, &PosComp)>().iter() {
            if p.sheet.name.eq_ignore_ascii_case(&lookup_name) {
                char_opt = Some(p.sheet.clone());
                break;
            }
        }
    }
    drop(g);

    if char_opt.is_none() {
        char_opt = store.load_character_by_name(&lookup_name).await.ok().flatten();
    }

    let mut quest_updated = None;
    let actor_label = char_opt
        .as_ref()
        .map(|c| c.name.clone())
        .unwrap_or(lookup_name);

    let reply_message = match command {
        IrcSlashCommand::Winken => format!("{actor_label} winkt durch den verrauchten Schankraum."),
        IrcSlashCommand::Gruessen => {
            format!("{actor_label} tippt sich zum Jäger-Gruß an die Hutkrempe.")
        }
        IrcSlashCommand::Prost => {
            format!("{actor_label} hebt ein eiskaltes Roadhouse-Bier: „Auf die Jagd!“")
        }
        IrcSlashCommand::Tacker => {
            if let Some(mut ch) = char_opt {
                ch.hp = (ch.hp + 50).min(ch.max_hp);
                let _ = store.save_character(&ch).await;
                sync_character_into_zone(&ch, &st).await;
            }
            format!("{actor_label} setzt den rostigen Winchester-Tacker an und klammert die Wunde zusammen (+50 HP).")
        }
        IrcSlashCommand::Exorzieren => format!(
            "{actor_label} rezitiert donnernd: „Exorcizamus te, omnis immundus spiritus, omnis satanica potestas!“"
        ),
        IrcSlashCommand::Salzen => {
            if let Some(mut ch) = char_opt {
                ch.salt_ward_active = true;
                if let Ok(quests) = store.list_quests(ch.id).await {
                    if let Some(mut q1) = quests
                        .into_iter()
                        .find(|q| q.quest_id == "quest_01_salz_auf_die_schwelle")
                    {
                        if q1.status != "completed" && q1.current_step < 2 {
                            q1.current_step = 2;
                            let _ = store.upsert_quest(ch.id, &q1).await;
                            quest_updated = Some(format!("{} (Schritt 2/3)", q1.title));
                        }
                    }
                }
                let _ = store.save_character(&ch).await;
                sync_character_into_zone(&ch, &st).await;
            }
            format!(
                "[ECS-SCHUTZLINIE] {actor_label} zieht eine durchgehende Linie aus reinem Steinsalz in {zone_id}!"
            )
        }
        IrcSlashCommand::Emf => {
            if let Some(mut ch) = char_opt {
                if let Ok(quests) = store.list_quests(ch.id).await {
                    if let Some(mut q1) = quests
                        .into_iter()
                        .find(|q| q.quest_id == "quest_01_salz_auf_die_schwelle")
                    {
                        if q1.status != "completed" && q1.current_step >= 1 {
                            q1.current_step = q1.total_steps;
                            q1.status = "completed".into();
                            let _ = store.upsert_quest(ch.id, &q1).await;
                            let reward = quest_cat
                                .find_quest(&q1.quest_id)
                                .map(|d| d.reward_amount)
                                .unwrap_or(5);
                            ch.essences.grant(
                                EssenceType::RockSaltAsh,
                                reward,
                                ch.race,
                                EssenceSource::QuestReward,
                            );
                            let _ = store.save_character(&ch).await;
                            sync_character_into_zone(&ch, &st).await;
                            quest_updated = Some(format!(
                                "{} ABGESCHLOSSEN (+{} Steinsalz-Asche)",
                                q1.title, reward
                            ));
                        }
                    }
                }
            }
            format!(
                "[EMF-SCAN {zone_id}] {actor_label} misst lokalen Omen-Index: {} (Essenz-Faktor x{:.2})",
                omen.label(),
                omen.essence_drop_multiplier()
            )
        }
    };

    MeshResponse::IrcSlashHandled {
        reply_message,
        omen_level,
        quest_updated,
    }
}

async fn sync_character_into_zone(ch: &CharacterSheet, st: &Arc<Mutex<NodeSharedState>>) {
    let mut g = st.lock().await;
    if let Some(zr) = g.zones.get_mut(&ch.current_zone) {
        if zr.player_entities.contains_key(&ch.id) {
            zr.spawn_or_update_player(ch.clone(), None);
        }
    }
}

async fn handle_client_reliable(
    msg: ClientReliableMessage,
    conn: Connection,
    st: Arc<Mutex<NodeSharedState>>,
) -> ServerReliableMessage {
    match msg {
        ClientReliableMessage::EnterZoneWithTicket { ticket_token } => {
            let store = st.lock().await.store.clone();
            let Ok(Some(ticket)) = store.consume_zone_ticket(&ticket_token).await else {
                return ServerReliableMessage::Error {
                    message: "Ungültiges oder abgelaufenes Zonen-Ticket".into(),
                };
            };
            let Ok(Some(mut ch)) = store.load_character(ticket.character_id).await else {
                return ServerReliableMessage::Error {
                    message: "Charakter nicht gefunden".into(),
                };
            };
            ch.current_zone = ticket.target_zone.clone();
            let _ = store.save_character(&ch).await;
            let quests = store.list_quests(ch.id).await.unwrap_or_default();

            let mut g = st.lock().await;
            let node_id = g.cfg.node_id.clone();
            let Some(zr) = g.zones.get_mut(&ticket.target_zone) else {
                return ServerReliableMessage::Error {
                    message: format!("Zone '{}' wird auf diesem Node nicht gehostet", ticket.target_zone),
                };
            };
            let zone_name = zr.def.name.clone();
            let irc_channel = zr.def.irc_channel.clone();
            let omen_level = zr.omen.level();
            let npcs: Vec<NpcDefinition> = zr.def.npcs.clone();
            zr.spawn_or_update_player(ch.clone(), Some(conn));

            ServerReliableMessage::ZoneEntered {
                node_id,
                zone_id: ticket.target_zone,
                zone_name,
                irc_channel,
                omen_level,
                character: ch,
                npcs,
                quests,
            }
        }
        ClientReliableMessage::AcceptHandover { handover_token } => {
            let (staged_opt, store) = {
                let mut g = st.lock().await;
                (
                    g.staged_handovers.remove(&handover_token),
                    g.store.clone(),
                )
            };
            let redis_opt = store.consume_handover_token(&handover_token).await.ok().flatten();
            let Some(handover) = staged_opt.or(redis_opt) else {
                return ServerReliableMessage::Error {
                    message: "Ungültiger oder abgelaufener PLAYER_HANDOVER-Token".into(),
                };
            };

            let mut ch = handover.character;
            ch.current_zone = handover.target_zone.clone();
            ch.pos_x = handover.spawn_x;
            ch.pos_y = handover.spawn_y;
            let _ = store.save_character(&ch).await;

            let mut g = st.lock().await;
            let node_id = g.cfg.node_id.clone();
            let Some(zr) = g.zones.get_mut(&handover.target_zone) else {
                return ServerReliableMessage::Error {
                    message: "Zielzone des Handovers existiert hier nicht".into(),
                };
            };
            let zone_name = zr.def.name.clone();
            let irc_channel = zr.def.irc_channel.clone();
            let omen_level = zr.omen.level();
            zr.spawn_or_update_player(ch.clone(), Some(conn));

            info!(
                "PLAYER_HANDOVER completed: '{}' spawned in '{}' on '{}'",
                ch.name, handover.target_zone, node_id
            );

            ServerReliableMessage::HandoverCompleted {
                node_id,
                zone_id: handover.target_zone,
                zone_name,
                irc_channel,
                omen_level,
                character: ch,
            }
        }
        ClientReliableMessage::InteractNpc {
            character_id,
            npc_id,
        } => {
            let (store, quest_cat, zone_cat) = {
                let g = st.lock().await;
                (g.store.clone(), g.quest_catalog.clone(), g.zone_catalog.clone())
            };
            let Ok(Some(ch)) = store.load_character(character_id).await else {
                return ServerReliableMessage::Error {
                    message: "Charakter nicht gefunden".into(),
                };
            };

            let npc_def = zone_cat
                .find_zone(&ch.current_zone)
                .and_then(|z| z.npcs.iter().find(|n| n.id == npc_id))
                .cloned();
            let Some(npc) = npc_def else {
                return ServerReliableMessage::Error {
                    message: format!("NPC/Objekt '{npc_id}' ist nicht in Zone '{}'", ch.current_zone),
                };
            };

            // Starte zugehörige Einstiegs-Quests datengetrieben
            let existing = store.list_quests(ch.id).await.unwrap_or_default();
            for qdef in &quest_cat.quests {
                if qdef.giver_npc == npc_id && !existing.iter().any(|e| e.quest_id == qdef.id) {
                    let dto = QuestStateDto {
                        quest_id: qdef.id.clone(),
                        title: qdef.title.clone(),
                        status: "active".into(),
                        current_step: 1,
                        total_steps: qdef.steps.len() as u32,
                    };
                    let _ = store.upsert_quest(ch.id, &dto).await;
                }
            }
            let quests = store.list_quests(ch.id).await.unwrap_or_default();
            ServerReliableMessage::NpcInteractionResult {
                npc_name: npc.name,
                dialogue: npc.greeting,
                quests,
                character: ch,
            }
        }
        ClientReliableMessage::QueryAshTerminal { .. } => {
            let g = st.lock().await;
            let now = now_epoch_ms();
            let active_nodes: Vec<_> = g
                .cluster_nodes
                .values()
                .filter(|n| now.saturating_sub(n.last_heartbeat_ms) <= 12_000)
                .cloned()
                .collect();
            let zone_omens: Vec<_> = g
                .cluster_routes
                .values()
                .map(|r| {
                    let oi = OmenIndex::from_u8(r.omen_level);
                    (r.zone_id.clone(), r.omen_level, oi.label().to_string())
                })
                .collect();
            let active_bounties = vec![
                "Kopfgeld: Kult-Saboteur an Kanal-4-Relais (zone_new_lawrence_west)".to_string(),
                "Kopfgeld: Höllenhund-Rudel am Schwefelriss (zone_salzminen)".to_string(),
            ];
            ServerReliableMessage::AshTerminalStatus {
                report: AshTerminalReport {
                    active_nodes,
                    zone_omens,
                    active_bounties,
                },
            }
        }
        ClientReliableMessage::UpgradeEquipment {
            character_id,
            slot,
            essence,
        } => {
            let store = st.lock().await.store.clone();
            let Ok(Some(mut ch)) = store.load_character(character_id).await else {
                return ServerReliableMessage::Error {
                    message: "Charakter nicht gefunden".into(),
                };
            };
            if let Err(e) = ch
                .equipment
                .upgrade_slot(ch.race, slot, essence, &mut ch.essences)
            {
                return ServerReliableMessage::Error {
                    message: e.to_string(),
                };
            }
            if let Err(e) = store.save_character(&ch).await {
                return ServerReliableMessage::Error {
                    message: e.to_string(),
                };
            }
            sync_character_into_zone(&ch, &st).await;
            ServerReliableMessage::EquipmentUpgraded {
                slot,
                character: ch,
            }
        }
        ClientReliableMessage::MoveAndCheckBoundary {
            character_id,
            pos_x,
            pos_y,
        } => match process_player_move(character_id, pos_x, pos_y, st).await {
            Ok(Some(redirect)) => ServerReliableMessage::PlayerHandoverRedirect { redirect },
            Ok(None) => ServerReliableMessage::PositionUpdated {
                zone_id: String::new(),
                pos_x,
                pos_y,
            },
            Err(e) => ServerReliableMessage::Error {
                message: e.to_string(),
            },
        },
        ClientReliableMessage::PerformCombat {
            character_id,
            action,
        } => execute_zone_combat(character_id, action, st).await,
        ClientReliableMessage::TriggerZoneOmen {
            zone_id,
            new_omen_level,
            reason,
        } => trigger_zone_omen_change(zone_id, new_omen_level, reason, st).await,
    }
}

async fn process_player_move(
    character_id: Uuid,
    pos_x: f32,
    pos_y: f32,
    st: Arc<Mutex<NodeSharedState>>,
) -> Result<Option<HandoverRedirectInfo>> {
    let (crossing, store, mesh_client, from_node) = {
        let mut g = st.lock().await;
        let mut current_zone_id = None;
        for (zid, zr) in &mut g.zones {
            if let Some(&entity) = zr.player_entities.get(&character_id) {
                if let Ok(mut pos) = zr.world.get::<&mut PosComp>(entity) {
                    pos.x = pos_x;
                    pos.y = pos_y;
                }
                if let Ok(mut p) = zr.world.get::<&mut PlayerEcsComp>(entity) {
                    p.sheet.pos_x = pos_x;
                    p.sheet.pos_y = pos_y;
                }
                current_zone_id = Some(zid.clone());
                break;
            }
        }
        let Some(zid) = current_zone_id else {
            return Ok(None);
        };
        let crossing = g
            .zone_catalog
            .check_boundary_crossing(&zid, pos_x)
            .map(|(target, sx, sy)| (zid, target, sx, sy));
        (
            crossing,
            g.store.clone(),
            g.mesh_client.clone(),
            g.cfg.node_id.clone(),
        )
    };

    let Some((from_zone, target_zone, spawn_x, spawn_y)) = crossing else {
        return Ok(None);
    };

    // Zielroute aus Gossip-Tabelle oder Redis ermitteln
    let route_opt = {
        let g = st.lock().await;
        g.cluster_routes.get(&target_zone).cloned()
    };
    let route = match route_opt {
        Some(r) => r,
        None => match store.get_active_zone_route(&target_zone, 12_000).await? {
            Some(r) => r,
            None => return Ok(None),
        },
    };

    let sheet_opt = {
        let mut g = st.lock().await;
        g.zones
            .get_mut(&from_zone)
            .and_then(|zr| zr.remove_player(character_id))
    };
    let Some(mut ch) = sheet_opt else {
        return Ok(None);
    };
    ch.current_zone = target_zone.clone();
    ch.pos_x = spawn_x;
    ch.pos_y = spawn_y;
    store.save_character(&ch).await?;
    let quests = store.list_quests(ch.id).await.unwrap_or_default();

    let handover_token = Uuid::new_v4().to_string();
    let state = PlayerHandoverState {
        character: ch,
        quests,
        from_node: from_node.clone(),
        from_zone: from_zone.clone(),
        target_zone: target_zone.clone(),
        spawn_x,
        spawn_y,
    };

    // 1. In Redis sichern (kurzlebiger Handover-Token)
    store.store_handover_token(&handover_token, &state).await?;

    // 2. Per Reliable mTLS QUIC-Stream direkt an Ziel-Knoten übertragen
    let mut target_client_addr = route.client_public_addr.clone();
    if route.node_id == from_node {
        st.lock()
            .await
            .staged_handovers
            .insert(handover_token.clone(), state);
    } else if let Ok(peer_sock) = resolve_socket_addr(&route.mesh_addr).await {
        if let Ok(connecting) = mesh_client.connect(peer_sock, "ash-node") {
            if let Ok(peer_conn) = connecting.await {
                if let Ok(MeshResponse::PlayerHandoverAck {
                    accepted: true,
                    client_public_addr,
                }) = rpc_call::<MeshMessage, MeshResponse>(
                    &peer_conn,
                    &MeshMessage::PlayerHandoverPrepare {
                        handover_token: handover_token.clone(),
                        state,
                    },
                )
                .await
                {
                    target_client_addr = client_public_addr;
                }
                peer_conn.close(0u32.into(), b"handover_done");
            }
        }
    }

    let redirect = HandoverRedirectInfo {
        target_zone,
        target_client_addr,
        handover_token,
    };

    Ok(Some(redirect))
}

async fn execute_zone_combat(
    character_id: Uuid,
    action: CombatActionNet,
    st: Arc<Mutex<NodeSharedState>>,
) -> ServerReliableMessage {
    let mut g = st.lock().await;
    for zr in g.zones.values_mut() {
        let Some(&entity) = zr.player_entities.get(&character_id) else {
            continue;
        };
        if zr.def.safe_sanctuary {
            return ServerReliableMessage::Error {
                message: "Harvelle's Roadhouse ist eine PvE-Schutzzone — hier ruhen die Waffen!".into(),
            };
        }
        let Ok(p) = zr.world.get::<&PlayerEcsComp>(entity) else {
            continue;
        };
        let sheet = p.sheet.clone();
        drop(p);

        let Some(first_enemy) = zr.enemies.first_mut() else {
            return ServerReliableMessage::Error {
                message: "Keine aktiven Feinde in Reichweite".into(),
            };
        };

        match action {
            CombatActionNet::MeleeStrike { damage } => {
                let _ = first_enemy.apply_direct_damage(damage);
                let hp = first_enemy.hp;
                let banished = first_enemy.is_banished();
                zr.aggro.record_damage_dealt(
                    sheet.id,
                    sheet.race,
                    sheet.class,
                    damage as f32,
                    false,
                );
                return ServerReliableMessage::CombatOutcome {
                    summary: format!("{} trifft für {damage} Schaden", sheet.name),
                    enemy_hp: hp,
                    enemy_banished: banished,
                    top_aggro_holder: zr.aggro.top_target(),
                };
            }
            CombatActionNet::ActiveBlock { mitigated } => {
                let hp = first_enemy.hp;
                let banished = first_enemy.is_banished();
                zr.aggro
                    .record_active_block_or_hit_taken(sheet.id, sheet.class, mitigated as f32);
                return ServerReliableMessage::CombatOutcome {
                    summary: format!("{} blockt aktiv ({mitigated} absorbiert)", sheet.name),
                    enemy_hp: hp,
                    enemy_banished: banished,
                    top_aggro_holder: zr.aggro.top_target(),
                };
            }
            CombatActionNet::PassiveDodge => {
                let hp = first_enemy.hp;
                let banished = first_enemy.is_banished();
                zr.aggro.record_passive_dodge_or_run_away(sheet.id);
                return ServerReliableMessage::CombatOutcome {
                    summary: format!("{} weicht passiv aus -> starker Aggro-Verlust!", sheet.name),
                    enemy_hp: hp,
                    enemy_banished: banished,
                    top_aggro_holder: zr.aggro.top_target(),
                };
            }
            CombatActionNet::CastBanishSheep => match apply_banish_sheep(sheet.class, first_enemy) {
                Ok(spell) => {
                    let hp = first_enemy.hp;
                    let banished = first_enemy.is_banished();
                    return ServerReliableMessage::CombatOutcome {
                        summary: format!("{} wirkt 30s Bann-Sheep: {spell}", sheet.name),
                        enemy_hp: hp,
                        enemy_banished: banished,
                        top_aggro_holder: zr.aggro.top_target(),
                    };
                }
                Err(e) => {
                    return ServerReliableMessage::Error {
                        message: e.to_string(),
                    }
                }
            },
            CombatActionNet::CastPyromancerWildfire => {
                for e in zr.enemies.iter_mut().take(3) {
                    cast_pyromancer_fire_spell(PyromancerFireSpell::QuickMolotow, e);
                }
                match cast_pyromancer_wildfire(&mut zr.enemies, (17.0, 0.0)) {
                    Ok(hit) => {
                        return ServerReliableMessage::CombatOutcome {
                            summary: format!("Flächenbrand trifft {hit} Ziele im 5m-Radius!"),
                            enemy_hp: zr.enemies[0].hp,
                            enemy_banished: zr.enemies[0].is_banished(),
                            top_aggro_holder: zr.aggro.top_target(),
                        }
                    }
                    Err(e) => {
                        return ServerReliableMessage::Error {
                            message: e.to_string(),
                        }
                    }
                }
            }
            CombatActionNet::CastEigenblutSpende => {
                let mut h_hp = sheet.hp;
                let mut ally_hp = (sheet.hp / 2).max(1);
                let healed = cast_eigenblut_spende(&mut h_hp, &mut ally_hp, sheet.max_hp);
                return ServerReliableMessage::CombatOutcome {
                    summary: format!("Eigenblut-Spende heilt {healed} HP trotz Boss-Immunität"),
                    enemy_hp: zr.enemies[0].hp,
                    enemy_banished: zr.enemies[0].is_banished(),
                    top_aggro_holder: zr.aggro.top_target(),
                };
            }
        }
    }

    ServerReliableMessage::Error {
        message: "Spieler ist aktuell in keiner Zone dieses Nodes aktiv".into(),
    }
}

async fn trigger_zone_omen_change(
    zone_id: String,
    new_omen_level: u8,
    reason: String,
    st: Arc<Mutex<NodeSharedState>>,
) -> ServerReliableMessage {
    let omen = OmenIndex::from_u8(new_omen_level);
    let alert_line = format!("{} (Ursache: {})", omen.format_irc_alert(&zone_id), reason);
    let should_alert = omen.requires_irc_radio_alert();

    let (store, role, hub_addr, mesh_client, route_opt) = {
        let mut g = st.lock().await;
        if let Some(zr) = g.zones.get_mut(&zone_id) {
            zr.omen = omen;
        }
        let mut route_updated = None;
        if let Some(r) = g.cluster_routes.get_mut(&zone_id) {
            r.omen_level = omen.level();
            r.last_heartbeat_ms = now_epoch_ms();
            route_updated = Some(r.clone());
        }
        if should_alert {
            let seq = g.next_alert_seq;
            g.next_alert_seq += 1;
            g.omen_alerts.push(OmenAlertEvent {
                seq,
                zone_id: zone_id.clone(),
                omen_level: omen.level(),
                irc_alert_line: alert_line.clone(),
            });
        }
        (
            g.store.clone(),
            g.cfg.role.clone(),
            g.cfg.hub_mesh_addr.clone(),
            g.mesh_client.clone(),
            route_updated,
        )
    };

    if let Some(r) = route_opt {
        let _ = store.publish_zone_route(&r).await;
    }

    // Falls dies ein Mirror-Node ist und ein Funk-Alarm (Omen >= 2) ausgelöst wurde,
    // zusätzlich per mTLS an den Hub melden, damit `irc-bridge` ihn sofort in `#new_lawrence` postet!
    if should_alert && role == NodeRole::Mirror {
        if let Some(hub_str) = hub_addr {
            if let Ok(sock) = resolve_socket_addr(&hub_str).await {
                if let Ok(connecting) = mesh_client.connect(sock, "ash-node") {
                    if let Ok(conn) = connecting.await {
                        let _ = rpc_call::<MeshMessage, MeshResponse>(
                            &conn,
                            &MeshMessage::BroadcastOmenAlert {
                                zone_id: zone_id.clone(),
                                omen_level: omen.level(),
                                irc_alert_line: alert_line,
                            },
                        )
                        .await;
                        conn.close(0u32.into(), b"alert_sent");
                    }
                }
            }
        }
    }

    ServerReliableMessage::OmenUpdated {
        zone_id,
        omen_level: omen.level(),
        alert_broadcasted: should_alert,
    }
}
