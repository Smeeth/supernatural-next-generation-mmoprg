// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::character::{
    validate_race_class, CharacterClass, CharacterSheet, PlayableRace, STARTING_ZONE_ROADHOUSE,
};
use crate::combat::{
    apply_banish_sheep, cast_pyromancer_fire_spell, cast_pyromancer_wildfire,
    execute_rune_cleric_salvo, execute_trickster_emergency_taunt, EnemyCombatState,
    PyromancerFireSpell, SniperStance, TankAggroTracker, BANISH_SHEEP_DURATION_SECS,
};
use crate::equipment::{
    EquipmentItem, EquipmentLoadout, EquipmentSlot, EssenceSource, EssenceType, EssenceWallet,
    ItemQuality, MaterialTag, WeaponArchetype,
};
use crate::error::DomainError;

/// Reiner deterministischer 64-Bit SplitMix64-PRNG für I/O- und Systemzeit-freie `ash-core`-Simulationen.
#[derive(Debug, Clone, Copy)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_uuid(&mut self) -> Uuid {
        let hi = self.next_u64() as u128;
        let lo = self.next_u64() as u128;
        Uuid::from_u128((hi << 64) | lo)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScenarioMode {
    Sim,
    E2e,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioSpec {
    pub id: String,
    pub description: String,
    pub mode: ScenarioMode,
    pub min_sprint: u32,
    pub seed: u64,
    #[serde(default)]
    pub actors: Vec<ScenarioActorSpec>,
    #[serde(default)]
    pub steps: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioActorSpec {
    pub id: String,
    pub name: String,
    pub race: String,
    pub class: String,
    #[serde(default = "default_start_zone")]
    pub start_zone: String,
}

fn default_start_zone() -> String {
    STARTING_ZONE_ROADHOUSE.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ScenarioStep {
    RegisterAndCreate {
        actor: String,
    },
    ExpectRejectedCreation {
        name: String,
        race: String,
        class: String,
        expected_reason: String,
    },
    #[serde(rename = "expect_16_gray_slots")]
    Expect16GraySlots {
        actor: String,
    },
    ExpectEquipRejected {
        actor: String,
        slot: u8,
        item_name: String,
        material: String,
        weapon_archetype: Option<String>,
        expected_error: String,
    },
    GrantAndUpgrade {
        actor: String,
        essence: String,
        amount: u32,
        slot: u8,
        expected_quality: String,
    },
    EnterRoadhouse {
        actor: String,
    },
    InteractNpc {
        actor: String,
        npc_id: String,
    },
    QueryAshTerminal {
        actor: String,
        min_active_nodes: usize,
    },
    IrcCommand {
        actor: String,
        channel: String,
        command_line: String,
        expect_reply_contains: String,
    },
    MoveAndHandover {
        actor: String,
        pos_x: f32,
        pos_y: f32,
        expected_zone: String,
    },
    UpgradeMainHandGreen {
        actor: String,
    },
    PerformBanishSheep {
        actor: String,
    },
    TriggerOmen {
        actor: String,
        zone_id: String,
        omen_level: u8,
        reason: String,
        expect_irc_alert_contains: Option<String>,
    },
    AssertState {
        actor: String,
        expected_zone: Option<String>,
        expected_salt_ward: Option<bool>,
        min_slots: Option<usize>,
    },
    SimCombatInvariant {
        invariant: String,
    },
}

impl ScenarioStep {
    pub fn action_name(&self) -> &'static str {
        match self {
            Self::RegisterAndCreate { .. } => "register_and_create",
            Self::ExpectRejectedCreation { .. } => "expect_rejected_creation",
            Self::Expect16GraySlots { .. } => "expect_16_gray_slots",
            Self::ExpectEquipRejected { .. } => "expect_equip_rejected",
            Self::GrantAndUpgrade { .. } => "grant_and_upgrade",
            Self::EnterRoadhouse { .. } => "enter_roadhouse",
            Self::InteractNpc { .. } => "interact_npc",
            Self::QueryAshTerminal { .. } => "query_ash_terminal",
            Self::IrcCommand { .. } => "irc_command",
            Self::MoveAndHandover { .. } => "move_and_handover",
            Self::UpgradeMainHandGreen { .. } => "upgrade_main_hand_green",
            Self::PerformBanishSheep { .. } => "perform_banish_sheep",
            Self::TriggerOmen { .. } => "trigger_omen",
            Self::AssertState { .. } => "assert_state",
            Self::SimCombatInvariant { .. } => "sim_combat_invariant",
        }
    }

    pub fn primary_actor(&self) -> &str {
        match self {
            Self::RegisterAndCreate { actor }
            | Self::Expect16GraySlots { actor }
            | Self::ExpectEquipRejected { actor, .. }
            | Self::GrantAndUpgrade { actor, .. }
            | Self::EnterRoadhouse { actor }
            | Self::InteractNpc { actor, .. }
            | Self::QueryAshTerminal { actor, .. }
            | Self::IrcCommand { actor, .. }
            | Self::MoveAndHandover { actor, .. }
            | Self::UpgradeMainHandGreen { actor }
            | Self::PerformBanishSheep { actor }
            | Self::TriggerOmen { actor, .. }
            | Self::AssertState { actor, .. } => actor,
            Self::ExpectRejectedCreation { name, .. } => name,
            Self::SimCombatInvariant { invariant } => invariant,
        }
    }
}

/// Maschinenlesbarer NDJSON-Trace-Eintrag pro Step/Tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioStepTrace {
    pub scenario_id: String,
    pub step_index: usize,
    pub tick: u64,
    pub actor: String,
    pub action: String,
    pub state_before: String,
    pub state_after: String,
    pub assertion_passed: bool,
    pub detail: String,
}

/// Kompakter, strukturierter Fehlerblock für schnelle KI-Fehlerdiagnose ohne Stacktrace-Rauschen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioFailureReport {
    pub scenario_id: String,
    pub failed_step_index: usize,
    pub failed_step: String,
    pub expected: String,
    pub actual: String,
    pub state_snapshot: String,
}

impl ScenarioFailureReport {
    pub fn format_compact_block(&self) -> String {
        format!(
            "=== HARNESS SCENARIO FAILURE ===\n\
             SCENARIO:       {}\n\
             FAILED_STEP:    #{} ({})\n\
             EXPECTED:       {}\n\
             ACTUAL:         {}\n\
             STATE_SNAPSHOT: {}\n\
             ================================",
            self.scenario_id,
            self.failed_step_index,
            self.failed_step,
            self.expected,
            self.actual,
            self.state_snapshot
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScenarioRunOutcome {
    Skipped {
        scenario_id: String,
        min_sprint: u32,
        current_sprint: u32,
    },
    Passed {
        scenario_id: String,
        steps_executed: usize,
        traces: Vec<ScenarioStepTrace>,
    },
}

impl ScenarioSpec {
    pub fn from_toml_str(raw: &str) -> Result<Self, DomainError> {
        toml::from_str(raw).map_err(|e| DomainError::DataConfigError(e.to_string()))
    }
}

fn summarize_hunter(h: &CharacterSheet) -> String {
    let main_q = h
        .equipment
        .get(EquipmentSlot::MainHand)
        .map(|i| i.quality.display_name())
        .unwrap_or("none");
    let pouch_q = h
        .equipment
        .get(EquipmentSlot::Pouch)
        .map(|i| i.quality.display_name())
        .unwrap_or("none");
    format!(
        "name={} zone={} pos=({:.1},{:.1}) slots={} main_hand='{}' pouch='{}' salt_ward={}",
        h.name,
        h.current_zone,
        h.pos_x,
        h.pos_y,
        h.equipment.slots.len(),
        main_q,
        pouch_q,
        h.salt_ward_active
    )
}

fn snapshot_all_actors(actors: &BTreeMap<String, CharacterSheet>) -> String {
    if actors.is_empty() {
        return "actors={}".to_string();
    }
    actors
        .iter()
        .map(|(k, v)| format!("{k}:[{}]", summarize_hunter(v)))
        .collect::<Vec<_>>()
        .join("; ")
}

fn parse_material_tag(s: &str) -> Option<MaterialTag> {
    match s.trim() {
        "PureIron" | "pure_iron" => Some(MaterialTag::PureIron),
        "HeavySteel" | "heavy_steel" => Some(MaterialTag::HeavySteel),
        "Cloth" | "cloth" => Some(MaterialTag::Cloth),
        "Leather" | "leather" => Some(MaterialTag::Leather),
        "AshWood" | "ash_wood" => Some(MaterialTag::AshWood),
        "BrassRune" | "brass_rune" => Some(MaterialTag::BrassRune),
        "ConsecratedSilver" | "consecrated_silver" => Some(MaterialTag::ConsecratedSilver),
        _ => None,
    }
}

fn parse_weapon_archetype(s: &str) -> Option<WeaponArchetype> {
    match s.trim() {
        "HeavyMeleeSteel" | "heavy_melee_steel" => Some(WeaponArchetype::HeavyMeleeSteel),
        "IronBladeOrRod" | "iron_blade_or_rod" => Some(WeaponArchetype::IronBladeOrRod),
        _ => None,
    }
}

fn parse_quality_name(s: &str) -> Option<ItemQuality> {
    match s.trim().to_lowercase().as_str() {
        "grey" | "gray" | "grau" => Some(ItemQuality::Grey),
        "green" | "grün" | "gruen" => Some(ItemQuality::Green),
        "blue" | "blau" => Some(ItemQuality::Blue),
        "purple" | "violett" => Some(ItemQuality::Purple),
        "gold" => Some(ItemQuality::Gold),
        _ => None,
    }
}

pub type ScenarioExecError = Box<(ScenarioFailureReport, Vec<ScenarioStepTrace>)>;

/// Führt ein deterministisches `mode = "sim"`-Szenario direkt in `ash-core` aus (null I/O, null Systemzeit).
pub fn execute_sim_scenario(
    spec: &ScenarioSpec,
    current_sprint: u32,
) -> Result<ScenarioRunOutcome, ScenarioExecError> {
    if spec.min_sprint > current_sprint {
        return Ok(ScenarioRunOutcome::Skipped {
            scenario_id: spec.id.clone(),
            min_sprint: spec.min_sprint,
            current_sprint,
        });
    }

    let mut rng = DeterministicRng::new(spec.seed);
    let actor_defs: BTreeMap<String, &ScenarioActorSpec> =
        spec.actors.iter().map(|a| (a.id.clone(), a)).collect();
    let mut actors: BTreeMap<String, CharacterSheet> = BTreeMap::new();
    let mut traces = Vec::with_capacity(spec.steps.len());

    for (idx, step) in spec.steps.iter().enumerate() {
        let tick = (idx as u64 + 1) * 10;
        let before = snapshot_all_actors(&actors);
        let action_str = step.action_name().to_string();
        let actor_str = step.primary_actor().to_string();

        let fail = |expected: String,
                    actual: String,
                    current_actors: &BTreeMap<String, CharacterSheet>| {
            Box::new((
                ScenarioFailureReport {
                    scenario_id: spec.id.clone(),
                    failed_step_index: idx,
                    failed_step: format!("{action_str} ({actor_str})"),
                    expected,
                    actual,
                    state_snapshot: snapshot_all_actors(current_actors),
                },
                traces.clone(),
            ))
        };

        let detail = match step {
            ScenarioStep::RegisterAndCreate { actor } => {
                let Some(def) = actor_defs.get(actor) else {
                    return Err(fail(
                        format!("defined actor '{actor}'"),
                        "actor missing in [[actors]]".into(),
                        &actors,
                    ));
                };
                let race = PlayableRace::parse_strict(&def.race).map_err(|e| {
                    fail(format!("valid race '{}'", def.race), e.to_string(), &actors)
                })?;
                let class = CharacterClass::parse_strict(&def.class).map_err(|e| {
                    fail(
                        format!("valid class '{}'", def.class),
                        e.to_string(),
                        &actors,
                    )
                })?;
                validate_race_class(race, class)
                    .map_err(|e| fail("valid race/class pair".into(), e.to_string(), &actors))?;

                let equipment = EquipmentLoadout::generate_starter_set(race, class)
                    .map_err(|e| fail("16-slot starter gear".into(), e.to_string(), &actors))?;
                let base_hp = 1000;
                let sheet = CharacterSheet {
                    id: rng.next_uuid(),
                    account_id: rng.next_uuid(),
                    name: def.name.clone(),
                    race,
                    class,
                    current_zone: STARTING_ZONE_ROADHOUSE.to_string(),
                    pos_x: 0.0,
                    pos_y: 0.0,
                    hp: base_hp,
                    max_hp: base_hp,
                    equipment,
                    essences: EssenceWallet::default(),
                    salt_ward_active: false,
                };
                actors.insert(actor.clone(), sheet);
                format!("created '{}' in {}", def.name, STARTING_ZONE_ROADHOUSE)
            }
            ScenarioStep::ExpectRejectedCreation {
                race,
                class,
                expected_reason,
                ..
            } => {
                let outcome = PlayableRace::parse_strict(race).and_then(|r| {
                    let c = CharacterClass::parse_strict(class)?;
                    validate_race_class(r, c)
                });
                match outcome {
                    Ok(()) => {
                        return Err(fail(
                            format!("rejection matching '{expected_reason}'"),
                            "creation unexpectedly succeeded".into(),
                            &actors,
                        ));
                    }
                    Err(err) => {
                        let code = match err {
                            DomainError::ForbiddenSupernaturalOrigin(_) => {
                                "ForbiddenSupernaturalOrigin"
                            }
                            DomainError::TankRoleForbidden { .. } => "TankRoleForbidden",
                            DomainError::InvalidRaceClassCombination { .. } => {
                                "InvalidRaceClassCombination"
                            }
                            _ => "OtherDomainError",
                        };
                        if !code.eq_ignore_ascii_case(expected_reason)
                            && !err.to_string().contains(expected_reason)
                        {
                            return Err(fail(
                                expected_reason.clone(),
                                format!("{code}: {err}"),
                                &actors,
                            ));
                        }
                        format!("correctly rejected ({code})")
                    }
                }
            }
            ScenarioStep::Expect16GraySlots { actor } => {
                let Some(h) = actors.get(actor) else {
                    return Err(fail(
                        format!("actor '{actor}' present"),
                        "missing".into(),
                        &actors,
                    ));
                };
                if h.equipment.slots.len() != 16 {
                    return Err(fail(
                        "16 equipped slots".into(),
                        format!("{} slots", h.equipment.slots.len()),
                        &actors,
                    ));
                }
                for slot in EquipmentSlot::ALL {
                    let Some(item) = h.equipment.get(slot) else {
                        return Err(fail(format!("slot {slot:?}"), "missing".into(), &actors));
                    };
                    if item.quality != ItemQuality::Grey {
                        return Err(fail(
                            "ItemQuality::Grey".into(),
                            format!("{:?} in {slot:?}", item.quality),
                            &actors,
                        ));
                    }
                }
                "verified all 16 starter slots are Grey".into()
            }
            ScenarioStep::ExpectEquipRejected {
                actor,
                slot,
                item_name,
                material,
                weapon_archetype,
                expected_error,
            } => {
                let eq_slot = EquipmentSlot::from_index(*slot)
                    .ok_or_else(|| fail("slot 1..=16".into(), format!("slot {slot}"), &actors))?;
                let mat = parse_material_tag(material)
                    .ok_or_else(|| fail("known MaterialTag".into(), material.clone(), &actors))?;
                let arch = weapon_archetype.as_deref().and_then(parse_weapon_archetype);
                let test_item = EquipmentItem {
                    slot: eq_slot,
                    item_code: "harness_probe_item".into(),
                    display_name: item_name.clone(),
                    quality: ItemQuality::Grey,
                    material: mat,
                    weapon_archetype: arch,
                    power_score: 15,
                    capacity: 0,
                    access_time_ms: 0,
                };
                let equip_res = {
                    let Some(h) = actors.get_mut(actor) else {
                        return Err(fail(
                            format!("actor '{actor}' present"),
                            "missing".into(),
                            &actors,
                        ));
                    };
                    h.equipment.equip_item(h.race, test_item)
                };
                match equip_res {
                    Ok(_) => {
                        return Err(fail(
                            format!("equip rejection '{expected_error}'"),
                            "equip unexpectedly succeeded".into(),
                            &actors,
                        ));
                    }
                    Err(err) => {
                        let tag = match err {
                            DomainError::FaeIronTaboo { .. } => "FaeIronTaboo",
                            DomainError::WitchHeavySteelTaboo { .. } => "WitchHeavySteelTaboo",
                            _ => "OtherDomainError",
                        };
                        if !tag.eq_ignore_ascii_case(expected_error) {
                            return Err(fail(
                                expected_error.clone(),
                                format!("{tag}: {err}"),
                                &actors,
                            ));
                        }
                        format!("equip rejected as expected ({tag})")
                    }
                }
            }
            ScenarioStep::GrantAndUpgrade {
                actor,
                essence,
                amount,
                slot,
                expected_quality,
            } => {
                let ess = EssenceType::parse_slug(essence)
                    .ok_or_else(|| fail("valid EssenceType".into(), essence.clone(), &actors))?;
                let eq_slot = EquipmentSlot::from_index(*slot)
                    .ok_or_else(|| fail("slot 1..=16".into(), format!("slot {slot}"), &actors))?;
                let exp_q = parse_quality_name(expected_quality).ok_or_else(|| {
                    fail(
                        "valid ItemQuality".into(),
                        expected_quality.clone(),
                        &actors,
                    )
                })?;
                let upgrade_res = {
                    let Some(h) = actors.get_mut(actor) else {
                        return Err(fail(
                            format!("actor '{actor}' present"),
                            "missing".into(),
                            &actors,
                        ));
                    };
                    h.essences
                        .grant(ess, *amount, h.race, EssenceSource::QuestReward);
                    h.equipment
                        .upgrade_slot(h.race, eq_slot, ess, &mut h.essences)
                };
                let new_q = upgrade_res
                    .map_err(|e| fail(format!("upgrade to {exp_q:?}"), e.to_string(), &actors))?;
                if new_q != exp_q {
                    return Err(fail(format!("{exp_q:?}"), format!("{new_q:?}"), &actors));
                }
                format!("upgraded slot {} to {:?}", eq_slot.display_name(), new_q)
            }
            ScenarioStep::AssertState {
                actor,
                expected_zone,
                expected_salt_ward,
                min_slots,
            } => {
                let Some(h) = actors.get(actor) else {
                    return Err(fail(
                        format!("actor '{actor}' present"),
                        "missing".into(),
                        &actors,
                    ));
                };
                if let Some(z) = expected_zone {
                    if &h.current_zone != z {
                        return Err(fail(z.clone(), h.current_zone.clone(), &actors));
                    }
                }
                if let Some(sw) = expected_salt_ward {
                    if h.salt_ward_active != *sw {
                        return Err(fail(
                            format!("salt_ward={sw}"),
                            format!("salt_ward={}", h.salt_ward_active),
                            &actors,
                        ));
                    }
                }
                if let Some(ms) = min_slots {
                    if h.equipment.slots.len() < *ms {
                        return Err(fail(
                            format!("slots >= {ms}"),
                            format!("slots = {}", h.equipment.slots.len()),
                            &actors,
                        ));
                    }
                }
                "state assertions matched".into()
            }
            ScenarioStep::SimCombatInvariant { invariant } => {
                verify_sim_combat_invariant(invariant, &mut rng)
                    .map_err(|(exp, act)| fail(exp, act, &actors))?
            }
            other => {
                return Err(fail(
                    "sim-compatible step".into(),
                    format!("step '{}' requires mode = 'e2e'", other.action_name()),
                    &actors,
                ));
            }
        };

        let after = snapshot_all_actors(&actors);
        traces.push(ScenarioStepTrace {
            scenario_id: spec.id.clone(),
            step_index: idx,
            tick,
            actor: actor_str,
            action: action_str,
            state_before: before,
            state_after: after,
            assertion_passed: true,
            detail,
        });
    }

    Ok(ScenarioRunOutcome::Passed {
        scenario_id: spec.id.clone(),
        steps_executed: traces.len(),
        traces,
    })
}

fn verify_sim_combat_invariant(
    invariant: &str,
    rng: &mut DeterministicRng,
) -> Result<String, (String, String)> {
    match invariant {
        "tank_aggro_balance" => {
            let mut tracker = TankAggroTracker::default();
            let tank = rng.next_uuid();
            let dd = rng.next_uuid();
            tracker.record_active_block_or_hit_taken(tank, CharacterClass::EisenKrieger, 200.0);
            tracker.record_damage_dealt(
                tank,
                PlayableRace::WinchesterDescendant,
                CharacterClass::EisenKrieger,
                220.0,
                true,
            );
            tracker.record_damage_dealt(
                dd,
                PlayableRace::VampireAlpha,
                CharacterClass::MachetenBerserker,
                400.0,
                true,
            );
            if tracker.top_target() != Some(tank) {
                return Err((
                    "tank holds top aggro".into(),
                    format!("{:?}", tracker.top_target()),
                ));
            }
            tracker.record_passive_dodge_or_run_away(tank);
            if tracker.top_target() != Some(dd) {
                return Err((
                    "passive dodge loses aggro to DD".into(),
                    format!("{:?}", tracker.top_target()),
                ));
            }
            Ok("tank aggro balance & passive dodge penalty verified".into())
        }
        "banish_sheep_30s" => {
            let mut mob = EnemyCombatState::new("Höllenhund", 900, false, 0.0, 0.0);
            if apply_banish_sheep(CharacterClass::EisenKrieger, &mut mob).is_ok() {
                return Err((
                    "non-ranged-DD rejected".into(),
                    "EisenKrieger banished".into(),
                ));
            }
            apply_banish_sheep(CharacterClass::WinchesterSniper, &mut mob)
                .map_err(|e| ("30s banish".into(), e.to_string()))?;
            if (mob.banish_remaining_secs - BANISH_SHEEP_DURATION_SECS).abs() > f32::EPSILON {
                return Err((
                    "30.0s duration".into(),
                    format!("{}", mob.banish_remaining_secs),
                ));
            }
            let _ = mob.apply_direct_damage(10);
            if mob.is_banished() {
                return Err((
                    "banish breaks on direct damage".into(),
                    "still banished".into(),
                ));
            }
            Ok("30s ranged-DD banish-sheep & break-on-damage verified".into())
        }
        "pyromancer_wildfire_5m" => {
            let mut mobs = vec![
                EnemyCombatState::new("M1", 400, false, 0.0, 0.0),
                EnemyCombatState::new("M2", 400, false, 2.0, 0.0),
                EnemyCombatState::new("M3", 400, false, 3.0, 1.0),
                EnemyCombatState::new("M4_near", 400, false, 4.5, 0.0),
                EnemyCombatState::new("M5_far", 400, false, 8.0, 0.0),
            ];
            cast_pyromancer_fire_spell(PyromancerFireSpell::QuickMolotow, &mut mobs[0]);
            cast_pyromancer_fire_spell(PyromancerFireSpell::QuickMolotow, &mut mobs[1]);
            if cast_pyromancer_wildfire(&mut mobs, (0.0, 0.0)).is_ok() {
                return Err((
                    "wildfire rejected at <3 targets".into(),
                    "succeeded at 2".into(),
                ));
            }
            cast_pyromancer_fire_spell(PyromancerFireSpell::QuickMolotow, &mut mobs[2]);
            let hit = cast_pyromancer_wildfire(&mut mobs, (0.0, 0.0))
                .map_err(|e| ("wildfire spread".into(), e.to_string()))?;
            if hit != 4 || mobs[4].flame_stacks != 0 {
                return Err(("4 targets within 5m hit".into(), format!("hit={hit}")));
            }
            Ok("pyromancer >=3 targets & 5m wildfire radius verified".into())
        }
        "sniper_and_cleric_and_trickster" => {
            let mut sniper = SniperStance::default();
            let d1 = sniper.fire_stationary_shot();
            let d2 = sniper.fire_stationary_shot();
            sniper.on_movement(1.0, 0.0);
            if !(d2 > d1 && sniper.stationary_stacks == 0) {
                return Err((
                    "sniper stacks reset on move".into(),
                    format!("stacks={}", sniper.stationary_stacks),
                ));
            }
            let (_, capped) = execute_rune_cleric_salvo(300.0, PlayableRace::Fae, 45.0, 50.0);
            if (capped - 50.0).abs() > 0.01 {
                return Err(("shield capped at 50.0".into(), format!("{capped}")));
            }
            let mut aggro = TankAggroTracker::default();
            let tank = rng.next_uuid();
            let trickster = rng.next_uuid();
            aggro.record_active_block_or_hit_taken(tank, CharacterClass::BannWaechter, 300.0);
            let res = execute_trickster_emergency_taunt(
                &mut aggro,
                trickster,
                tank,
                250,
                (0.0, 0.0),
                0.5,
            );
            if res.aggro_restored_to_tank != tank {
                return Err((
                    "aggro returned to tank".into(),
                    format!("{:?}", res.aggro_restored_to_tank),
                ));
            }
            Ok("sniper reset, rune-cleric shield cap & trickster aggro return verified".into())
        }
        other => Err(("known combat invariant".into(), other.to_string())),
    }
}
