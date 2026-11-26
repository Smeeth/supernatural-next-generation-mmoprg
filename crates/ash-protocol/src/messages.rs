// SPDX-License-Identifier: GPL-3.0-or-later

use ash_core::character::CharacterSheet;
use ash_core::equipment::{EquipmentSlot, EssenceType};
use ash_core::world::{IrcSlashCommand, NpcDefinition};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// 1. Auth-Gateway <-> Client (Reliable QUIC Stream RPC)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthRequest {
    RegisterAccount {
        username: String,
        password: String,
    },
    Login {
        username: String,
        password: String,
    },
    CreateCharacter {
        session_token: String,
        name: String,
        race: String,
        class: String,
    },
    ListCharacters {
        session_token: String,
    },
    IssueZoneTicket {
        session_token: String,
        character_id: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneRoutingTicket {
    pub ticket_token: String,
    pub character_id: Uuid,
    pub target_zone: String,
    pub target_client_addr: String,
    pub fallback_to_roadhouse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthResponse {
    AccountRegistered {
        account_id: Uuid,
        session_token: String,
    },
    LoggedIn {
        account_id: Uuid,
        session_token: String,
    },
    CharacterCreated {
        character: CharacterSheet,
    },
    CharacterList {
        characters: Vec<CharacterSheet>,
    },
    ZoneTicketIssued {
        ticket: ZoneRoutingTicket,
    },
    Error {
        code: String,
        message: String,
    },
}

// ============================================================================
// 2. Game-Node <-> Client (Unreliable QUIC Datagrams)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientDatagram {
    MoveInput {
        character_id: Uuid,
        seq: u64,
        pos_x: f32,
        pos_y: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityPositionNet {
    pub id: Uuid,
    pub name: String,
    pub pos_x: f32,
    pub pos_y: f32,
    pub hp: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoverRedirectInfo {
    pub target_zone: String,
    pub target_client_addr: String,
    pub handover_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerDatagram {
    ZoneSnapshot {
        tick: u64,
        zone_id: String,
        omen_level: u8,
        entities: Vec<EntityPositionNet>,
        pending_handover: Option<HandoverRedirectInfo>,
    },
}

// ============================================================================
// 3. Game-Node <-> Client (Reliable QUIC Streams)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CombatActionNet {
    MeleeStrike { damage: i32 },
    ActiveBlock { mitigated: i32 },
    PassiveDodge,
    CastBanishSheep,
    CastPyromancerWildfire,
    CastEigenblutSpende,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestStateDto {
    pub quest_id: String,
    pub title: String,
    pub status: String,
    pub current_step: u32,
    pub total_steps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AshTerminalReport {
    pub active_nodes: Vec<ClusterNodeInfo>,
    pub zone_omens: Vec<(String, u8, String)>,
    pub active_bounties: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientReliableMessage {
    EnterZoneWithTicket {
        ticket_token: String,
    },
    AcceptHandover {
        handover_token: String,
    },
    InteractNpc {
        character_id: Uuid,
        npc_id: String,
    },
    QueryAshTerminal {
        character_id: Uuid,
    },
    UpgradeEquipment {
        character_id: Uuid,
        slot: EquipmentSlot,
        essence: EssenceType,
    },
    MoveAndCheckBoundary {
        character_id: Uuid,
        pos_x: f32,
        pos_y: f32,
    },
    PerformCombat {
        character_id: Uuid,
        action: CombatActionNet,
    },
    TriggerZoneOmen {
        zone_id: String,
        new_omen_level: u8,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerReliableMessage {
    ZoneEntered {
        node_id: String,
        zone_id: String,
        zone_name: String,
        irc_channel: String,
        omen_level: u8,
        character: CharacterSheet,
        npcs: Vec<NpcDefinition>,
        quests: Vec<QuestStateDto>,
    },
    HandoverCompleted {
        node_id: String,
        zone_id: String,
        zone_name: String,
        irc_channel: String,
        omen_level: u8,
        character: CharacterSheet,
    },
    NpcInteractionResult {
        npc_name: String,
        dialogue: String,
        quests: Vec<QuestStateDto>,
        character: CharacterSheet,
    },
    AshTerminalStatus {
        report: AshTerminalReport,
    },
    EquipmentUpgraded {
        slot: EquipmentSlot,
        character: CharacterSheet,
    },
    PositionUpdated {
        zone_id: String,
        pos_x: f32,
        pos_y: f32,
    },
    PlayerHandoverRedirect {
        redirect: HandoverRedirectInfo,
    },
    CombatOutcome {
        summary: String,
        enemy_hp: i32,
        enemy_banished: bool,
        top_aggro_holder: Option<Uuid>,
    },
    OmenUpdated {
        zone_id: String,
        omen_level: u8,
        alert_broadcasted: bool,
    },
    Error {
        message: String,
    },
}

// ============================================================================
// 4. Server-to-Server Mesh & IRC-Bridge (mTLS QUIC Streams)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterNodeInfo {
    pub node_id: String,
    pub role: String,
    pub client_public_addr: String,
    pub mesh_addr: String,
    pub zones: Vec<String>,
    pub last_heartbeat_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneRouteInfo {
    pub zone_id: String,
    pub node_id: String,
    pub client_public_addr: String,
    pub mesh_addr: String,
    pub omen_level: u8,
    pub last_heartbeat_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerHandoverState {
    pub character: CharacterSheet,
    pub quests: Vec<QuestStateDto>,
    pub from_node: String,
    pub from_zone: String,
    pub target_zone: String,
    pub spawn_x: f32,
    pub spawn_y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshMessage {
    GossipAnnounce {
        node: ClusterNodeInfo,
        zone_omens: Vec<(String, u8)>,
    },
    Heartbeat {
        node_id: String,
        zone_omens: Vec<(String, u8)>,
    },
    QueryRoutingTable,
    PlayerHandoverPrepare {
        handover_token: String,
        state: PlayerHandoverState,
    },
    IrcSlashEvent {
        irc_nick: String,
        character_name: String,
        channel: String,
        zone_id: String,
        command: IrcSlashCommand,
    },
    PollOmenAlerts {
        since_seq: u64,
    },
    BroadcastOmenAlert {
        zone_id: String,
        omen_level: u8,
        irc_alert_line: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmenAlertEvent {
    pub seq: u64,
    pub zone_id: String,
    pub omen_level: u8,
    pub irc_alert_line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshResponse {
    RoutingTable {
        nodes: Vec<ClusterNodeInfo>,
        routes: Vec<ZoneRouteInfo>,
    },
    PlayerHandoverAck {
        accepted: bool,
        client_public_addr: String,
    },
    IrcSlashHandled {
        reply_message: String,
        omen_level: u8,
        quest_updated: Option<String>,
    },
    OmenAlerts {
        alerts: Vec<OmenAlertEvent>,
    },
    Ack,
    Error {
        message: String,
    },
}
