// SPDX-License-Identifier: GPL-3.0-or-later

use ash_core::character::CharacterSheet;
use ash_core::world::IrcSlashCommand;
use serde::{Deserialize, Serialize};

use crate::client::{ClusterNodeInfo, QuestStateDto};

// ============================================================================
// Server-to-Server Mesh & IRC-Bridge (mTLS QUIC Streams — nur Zone `server`!)
// ============================================================================

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
