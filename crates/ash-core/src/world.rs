// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::character::{CharacterClass, PlayableRace};
use crate::combat::EnemyCombatState;
use crate::equipment::EssenceType;
use crate::error::DomainError;

pub const EMBEDDED_ZONES_TOML: &str = include_str!("../../../data/zones.toml");
pub const EMBEDDED_QUESTS_TOML: &str = include_str!("../../../data/quests.toml");

/// Dynamischer Omen-Index (0–3) pro Zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OmenIndex {
    /// 0 — Ruhige Front: Standard-Mobs, normale Händler.
    CalmFront = 0,
    /// 1 — Unheilvolle Omen: Permanenter EMF-Ausschlag, Blutregen, Händler ziehen sich zurück, +Essenz-Drop-Rate.
    UnsettlingOmens = 1,
    /// 2 — Durchbruch / Höllentor aktiv: Riss spawnt, Wellen von Besessenen & Höllenhunden, IRC-Alarm in #new_lawrence.
    HellgateBreach = 2,
    /// 3 — Invasion / Himmels-Eingriff: Spawnt Dämonen-Weltboss oder gnadenlose Engels-Garnison, IRC-Alarm in #new_lawrence.
    InvasionOrHeavenIntervention = 3,
}

impl OmenIndex {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::CalmFront,
            1 => Self::UnsettlingOmens,
            2 => Self::HellgateBreach,
            _ => Self::InvasionOrHeavenIntervention,
        }
    }

    pub fn level(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::CalmFront => "0 - Ruhige Front",
            Self::UnsettlingOmens => "1 - Unheilvolle Omen (Blutregen & EMF-Ausschlag)",
            Self::HellgateBreach => "2 - Durchbruch / Höllentor aktiv",
            Self::InvasionOrHeavenIntervention => "3 - Invasion / Himmels-Eingriff",
        }
    }

    pub fn vendors_open(self) -> bool {
        self == Self::CalmFront
    }

    pub fn essence_drop_multiplier(self) -> f32 {
        match self {
            Self::CalmFront => 1.0,
            Self::UnsettlingOmens => 1.35,
            Self::HellgateBreach => 1.75,
            Self::InvasionOrHeavenIntervention => 2.25,
        }
    }

    pub fn requires_irc_radio_alert(self) -> bool {
        matches!(self, Self::HellgateBreach | Self::InvasionOrHeavenIntervention)
    }

    pub fn format_irc_alert(self, zone_id: &str) -> String {
        match self {
            Self::HellgateBreach => format!(
                "[ASH-NET FUNK-ALARM][OMEN-STUFE 2] Höllentor-Durchbruch in {}! Besessene & Höllenhunde gemeldet — alle Jäger zum Riss!",
                zone_id
            ),
            Self::InvasionOrHeavenIntervention => format!(
                "[ASH-NET NOTRUF][OMEN-STUFE 3] Kritische Invasion / Himmels-Eingriff in {}! Dämonen-Weltboss oder Engels-Garnison im Anmarsch!",
                zone_id
            ),
            _ => format!("[ASH-NET EMF] Zone {} meldet Status: {}", zone_id, self.label()),
        }
    }
}

/// Raid-Boss 1: Malphas, Ritter der Hölle (`zone_hoellentor_alpha`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalphasEncounter {
    pub boss: EnemyCombatState,
    pub hellhound_timer_secs: f32,
    pub hellhound_alphas: Vec<EnemyCombatState>,
    pub phase_two_triggered: bool,
}

impl Default for MalphasEncounter {
    fn default() -> Self {
        Self::new()
    }
}

impl MalphasEncounter {
    pub fn new() -> Self {
        let mut boss = EnemyCombatState::new("Malphas, Ritter der Hölle", 50_000, true, 0.0, 0.0);
        boss.active_boss_buffs
            .push("Höllenritter-Aegis".to_string());
        Self {
            boss,
            hellhound_timer_secs: 60.0,
            hellhound_alphas: Vec::new(),
            phase_two_triggered: false,
        }
    }

    /// Tickt den Encounter: alle 60s spawnen 2 unverwundbare Höllenhund-Alphas (erfordern 30-Sek.-Bann-Sheep!).
    pub fn tick(&mut self, dt_secs: f32) -> Vec<String> {
        let mut events = Vec::new();
        self.boss.tick(dt_secs);
        for hound in &mut self.hellhound_alphas {
            hound.tick(dt_secs);
        }

        self.hellhound_timer_secs -= dt_secs;
        if self.hellhound_timer_secs <= 0.0 {
            self.hellhound_timer_secs += 60.0;
            for idx in 1..=2 {
                let mut alpha =
                    EnemyCombatState::new(format!("Höllenhund-Alpha #{idx}"), 8_000, false, 4.0 * idx as f32, 0.0);
                alpha.invulnerable_phase = true; // Unverwundbar — erfordert 30-Sek.-Bann-Sheep!
                self.hellhound_alphas.push(alpha);
            }
            events.push("Malphas ruft zwei unverwundbare Höllenhund-Alphas! Fernkampf-DD Bann-Sheep erforderlich!".to_string());
        }

        if !self.phase_two_triggered && self.boss.hp_ratio() <= 0.50 {
            self.phase_two_triggered = true;
            self.boss
                .active_boss_buffs
                .push("Entfesselter Schwefelzorn".to_string());
            events.push("Malphas erreicht 50% HP — Phase 2 (Doppelagent-Fenster offen)!".to_string());
        }

        events
    }

    /// Berechnet Schwefel-Nova-AoE-Schaden unter Berücksichtigung von Bunker-Tüftler-Barrikaden & Boden-Sigillen.
    pub fn compute_sulfur_nova_damage(has_barricade_cover: bool, has_ground_sigil: bool) -> i32 {
        let mut dmg = 1200.0_f32;
        if has_barricade_cover {
            dmg *= 0.45;
        }
        if has_ground_sigil {
            dmg *= 0.70;
        }
        dmg.round() as i32
    }
}

/// Raid-Boss 2: Sariel, Zelot der Himmelsgarnison (`zone_relais_kathedrale`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SarielEncounter {
    pub boss: EnemyCombatState,
    pub sky_phase_remaining_secs: f32,
    pub angel_guards: Vec<EnemyCombatState>,
}

impl Default for SarielEncounter {
    fn default() -> Self {
        Self::new()
    }
}

impl SarielEncounter {
    pub fn new() -> Self {
        let boss = EnemyCombatState::new("Sariel, Zelot der Himmelsgarnison", 46_000, true, 0.0, 0.0);
        let guard = EnemyCombatState::new("Engels-Wache der Garnison", 9_500, false, 6.0, 2.0);
        Self {
            boss,
            sky_phase_remaining_secs: 0.0,
            angel_guards: vec![guard],
        }
    }

    /// Startet die 12-sekündige unangreifbare Himmelsphase (triggert Eigenblut-Notfallskills).
    pub fn enter_sky_phase(&mut self) {
        self.sky_phase_remaining_secs = 12.0;
        self.boss.invulnerable_phase = true;
    }

    pub fn tick(&mut self, dt_secs: f32) {
        self.boss.tick(dt_secs);
        for g in &mut self.angel_guards {
            g.tick(dt_secs);
        }
        if self.sky_phase_remaining_secs > 0.0 {
            self.sky_phase_remaining_secs = (self.sky_phase_remaining_secs - dt_secs).max(0.0);
            if self.sky_phase_remaining_secs == 0.0 {
                self.boss.invulnerable_phase = false;
            }
        }
    }

    /// Wählt vorrangig nicht-menschliche Rassen für gezielte Anti-Monster-Bann-Strahlen aus.
    pub fn select_anti_monster_beam_target(party: &[(Uuid, PlayableRace)]) -> Option<Uuid> {
        party
            .iter()
            .find(|(_, race)| race.is_non_human_monster_or_fae())
            .or_else(|| party.first())
            .map(|(id, _)| *id)
    }

    /// Engels-Wachen lassen sich gezielt via Pyromanten-Bann „Heiliger-Öl-Feuerring“ einfrieren.
    pub fn freeze_guard_with_holy_oil(
        caster_class: CharacterClass,
        guard_idx: usize,
        guards: &mut [EnemyCombatState],
    ) -> Result<(), DomainError> {
        if caster_class != CharacterClass::SalzUndBenzinPyromant {
            return Err(DomainError::BanishRestrictedToRangedDd(
                caster_class.display_name().to_string(),
            ));
        }
        if let Some(guard) = guards.get_mut(guard_idx) {
            crate::combat::apply_banish_sheep(caster_class, guard)?;
        }
        Ok(())
    }
}

/// Datengetriebene Zonen- & NPC-Konfiguration (`data/zones.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneCatalog {
    pub zones: Vec<ZoneDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneDefinition {
    pub id: String,
    pub name: String,
    pub irc_channel: String,
    pub safe_sanctuary: bool,
    pub default_omen: u8,
    pub boundary_west_x: Option<f32>,
    pub boundary_west_target: Option<String>,
    pub boundary_west_spawn_x: Option<f32>,
    pub boundary_west_spawn_y: Option<f32>,
    pub boundary_east_x: Option<f32>,
    pub boundary_east_target: Option<String>,
    pub boundary_east_spawn_x: Option<f32>,
    pub boundary_east_spawn_y: Option<f32>,
    pub raid_boss: Option<String>,
    pub description: String,
    #[serde(default)]
    pub npcs: Vec<NpcDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcDefinition {
    pub id: String,
    pub name: String,
    pub role: String,
    pub pos_x: f32,
    pub pos_y: f32,
    pub greeting: String,
}

impl ZoneCatalog {
    pub fn load_embedded() -> Result<Self, DomainError> {
        Self::from_toml_str(EMBEDDED_ZONES_TOML)
    }

    pub fn from_toml_str(raw: &str) -> Result<Self, DomainError> {
        toml::from_str(raw).map_err(|e| DomainError::DataConfigError(e.to_string()))
    }

    pub fn find_zone(&self, zone_id: &str) -> Option<&ZoneDefinition> {
        self.zones.iter().find(|z| z.id == zone_id)
    }

    /// Prüft, ob eine Position eine Zonengrenze überschreitet und liefert `(target_zone, spawn_x, spawn_y)`.
    pub fn check_boundary_crossing(&self, zone_id: &str, pos_x: f32) -> Option<(String, f32, f32)> {
        let z = self.find_zone(zone_id)?;
        if let (Some(east_x), Some(target)) = (z.boundary_east_x, z.boundary_east_target.as_ref()) {
            if pos_x >= east_x {
                return Some((
                    target.clone(),
                    z.boundary_east_spawn_x.unwrap_or(-90.0),
                    z.boundary_east_spawn_y.unwrap_or(0.0),
                ));
            }
        }
        if let (Some(west_x), Some(target)) = (z.boundary_west_x, z.boundary_west_target.as_ref()) {
            if pos_x <= west_x {
                return Some((
                    target.clone(),
                    z.boundary_west_spawn_x.unwrap_or(90.0),
                    z.boundary_west_spawn_y.unwrap_or(0.0),
                ));
            }
        }
        None
    }
}

/// Datengetriebene Einstiegs-Quests (`data/quests.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestCatalog {
    pub quests: Vec<QuestDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestDefinition {
    pub id: String,
    pub title: String,
    pub giver_npc: String,
    pub secondary_npc: Option<String>,
    pub start_zone: String,
    pub summary: String,
    pub reward_essence: String,
    pub reward_amount: u32,
    pub steps: Vec<String>,
}

impl QuestDefinition {
    pub fn parsed_reward_essence(&self) -> Option<EssenceType> {
        EssenceType::parse_slug(&self.reward_essence)
    }
}

impl QuestCatalog {
    pub fn load_embedded() -> Result<Self, DomainError> {
        Self::from_toml_str(EMBEDDED_QUESTS_TOML)
    }

    pub fn from_toml_str(raw: &str) -> Result<Self, DomainError> {
        toml::from_str(raw).map_err(|e| DomainError::DataConfigError(e.to_string()))
    }

    pub fn find_quest(&self, quest_id: &str) -> Option<&QuestDefinition> {
        self.quests.iter().find(|q| q.id == quest_id)
    }
}

/// In-Game-Slash-Kommandos aus dem IRC-Chat (`irc-bridge` -> `game-node` ECS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IrcSlashCommand {
    // Klassisch
    Winken,
    Gruessen,
    Prost,
    // Supernatural-Flair
    Salzen,
    Emf,
    Exorzieren,
    Tacker,
}

impl IrcSlashCommand {
    pub fn parse_chat_line(line: &str) -> Option<Self> {
        let first = line.trim().split_whitespace().next()?.to_lowercase();
        match first.as_str() {
            "/winken" | "!winken" => Some(Self::Winken),
            "/grüßen" | "/gruessen" | "!grüßen" | "!gruessen" => Some(Self::Gruessen),
            "/prost" | "!prost" => Some(Self::Prost),
            "/salzen" | "!salzen" => Some(Self::Salzen),
            "/emf" | "!emf" => Some(Self::Emf),
            "/exorzieren" | "!exorzieren" => Some(Self::Exorzieren),
            "/tacker" | "!tacker" => Some(Self::Tacker),
            _ => None,
        }
    }
}
