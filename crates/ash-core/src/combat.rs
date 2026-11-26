// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::character::{CharacterClass, PlayableRace};
use crate::error::DomainError;

pub const BANISH_SHEEP_DURATION_SECS: f32 = 30.0;
pub const PYROMANCER_WILDFIRE_MIN_TARGETS: usize = 3;
pub const PYROMANCER_WILDFIRE_RADIUS_METERS: f32 = 5.0;
pub const RUNE_CLERIC_SHIELD_RATIO: f32 = 0.20;
pub const DOPPELAGENT_DURATION_SECS: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageElement {
    Physical,
    PureIron,
    ConsecratedSilver,
    Fire,
    RockSalt,
    EnochianHoly,
    HellfireCurse,
}

/// Berechnet Elementar- & Rassen-Modifikatoren (Fae Eisen-Tabu/Steinsalz, Vampir/Werwolf/Wandler Silber-Anfälligkeit).
pub fn racial_incoming_damage_multiplier(race: PlayableRace, element: DamageElement) -> f32 {
    match (race, element) {
        (PlayableRace::Fae, DamageElement::PureIron) => 1.50,
        (PlayableRace::Fae, DamageElement::RockSalt) => 1.25,
        (PlayableRace::VampireAlpha, DamageElement::ConsecratedSilver | DamageElement::Fire) => {
            1.15
        }
        (PlayableRace::WerewolfPack, DamageElement::ConsecratedSilver) => 1.25,
        (PlayableRace::Shapeshifter, DamageElement::ConsecratedSilver) => 1.10,
        _ => 1.0,
    }
}

/// Berechnet ausgehenden Schadensbonus (Winchester „Family Business“ gegen Bosse/Dämonen).
pub fn racial_outgoing_damage_multiplier(
    race: PlayableRace,
    target_is_supernatural_boss: bool,
) -> f32 {
    if race == PlayableRace::WinchesterDescendant && target_is_supernatural_boss {
        1.15
    } else {
        1.0
    }
}

/// Tank-Aggro-Regel: Ein Tank hält Aggro NUR, wenn er ausgewogen Schaden austeilt UND einsteckt
/// (bzw. im Nahkontakt aktiv blockt/pariert). Passives Ausweichen oder Weglaufen führt zu massivem Aggro-Verlust.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TankAggroTracker {
    pub threat_table: BTreeMap<Uuid, f32>,
    pub window_damage_dealt: BTreeMap<Uuid, f32>,
    pub window_risk_absorbed: BTreeMap<Uuid, f32>,
    pub enochian_rune_charge: BTreeMap<Uuid, f32>,
}

impl TankAggroTracker {
    pub fn current_threat(&self, actor: Uuid) -> f32 {
        self.threat_table.get(&actor).copied().unwrap_or(0.0)
    }

    pub fn top_target(&self) -> Option<Uuid> {
        self.threat_table
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| *id)
    }

    /// Aktiver Nahkampf-Angriff durch Tank oder DD.
    pub fn record_damage_dealt(
        &mut self,
        actor: Uuid,
        race: PlayableRace,
        class: CharacterClass,
        amount: f32,
        is_supernatural_boss: bool,
    ) -> f32 {
        let dealt = self.window_damage_dealt.entry(actor).or_insert(0.0);
        *dealt += amount;

        let absorbed = self
            .window_risk_absorbed
            .get(&actor)
            .copied()
            .unwrap_or(0.0);
        let base_mult = if class.category() == crate::character::ClassCategory::Tank {
            // Balance-Faktor: braucht sowohl ausgeteilten Schaden als auch eingestecktes/aktiv geblocktes Risiko!
            if *dealt > 0.0 && absorbed > 0.0 {
                let ratio = (*dealt / absorbed).clamp(0.25, 4.0);
                if (0.4..=2.5).contains(&ratio) {
                    2.2 // Ausgewogenes Verhältnis -> volle Tank-Aggro!
                } else {
                    1.2
                }
            } else {
                0.65 // Noch kein ausgewogener Schlagabtausch
            }
        } else {
            1.0
        };

        let family_mult = if race == PlayableRace::WinchesterDescendant && is_supernatural_boss {
            1.25
        } else {
            1.0
        };

        let added_threat = amount * base_mult * family_mult;
        *self.threat_table.entry(actor).or_insert(0.0) += added_threat;
        self.current_threat(actor)
    }

    /// Eingesteckter Schaden oder aktives Blocken/Parieren im Nahkontakt zählt positiv für die Tank-Aggro-Balance!
    pub fn record_active_block_or_hit_taken(
        &mut self,
        tank: Uuid,
        class: CharacterClass,
        mitigated_or_taken: f32,
    ) -> f32 {
        *self.window_risk_absorbed.entry(tank).or_insert(0.0) += mitigated_or_taken;
        let dealt = self.window_damage_dealt.get(&tank).copied().unwrap_or(0.0);
        let balance_bonus = if dealt > 0.0 { 1.8 } else { 0.8 };
        *self.threat_table.entry(tank).or_insert(0.0) += mitigated_or_taken * balance_bonus;

        if class == CharacterClass::BannWaechter {
            *self.enochian_rune_charge.entry(tank).or_insert(0.0) += mitigated_or_taken * 0.5;
        }
        self.current_threat(tank)
    }

    /// Bann-Wächter entlädt absorbierte Schläge als Henochische AoE-Schockwelle für Gruppen-Aggro.
    pub fn release_bann_waechter_shockwave(&mut self, tank: Uuid) -> f32 {
        let charge = self.enochian_rune_charge.insert(tank, 0.0).unwrap_or(0.0);
        let threat_gain = charge * 2.5;
        *self.threat_table.entry(tank).or_insert(0.0) += threat_gain;
        threat_gain
    }

    /// Rein passives Ausweichen oder Weglaufen/Kiten zerstört massiv die Tank-Aggro!
    pub fn record_passive_dodge_or_run_away(&mut self, tank: Uuid) -> f32 {
        let entry = self.threat_table.entry(tank).or_insert(0.0);
        *entry *= 0.25; // -75% Aggro-Einbruch!
        if let Some(absorbed) = self.window_risk_absorbed.get_mut(&tank) {
            *absorbed *= 0.25;
        }
        *entry
    }
}

/// Exklusives 30-Sekunden-„Bann-Sheep“ der 3 Fernkampf-DDs; bricht sofort bei direktem Schaden.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnemyCombatState {
    pub id: Uuid,
    pub name: String,
    pub is_boss: bool,
    pub invulnerable_phase: bool,
    pub hp: i32,
    pub max_hp: i32,
    pub pos_x: f32,
    pub pos_y: f32,
    pub banish_remaining_secs: f32,
    pub banish_spell: Option<String>,
    pub flame_stacks: u32,
    pub bleed_stacks: u32,
    pub drunkenness_stacks: u32,
    pub blinded: bool,
    pub stunned_secs: f32,
    pub active_boss_buffs: Vec<String>,
}

impl EnemyCombatState {
    pub fn new(name: impl Into<String>, hp: i32, is_boss: bool, pos_x: f32, pos_y: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            is_boss,
            invulnerable_phase: false,
            hp,
            max_hp: hp,
            pos_x,
            pos_y,
            banish_remaining_secs: 0.0,
            banish_spell: None,
            flame_stacks: 0,
            bleed_stacks: 0,
            drunkenness_stacks: 0,
            blinded: false,
            stunned_secs: 0.0,
            active_boss_buffs: Vec::new(),
        }
    }

    pub fn is_banished(&self) -> bool {
        self.banish_remaining_secs > 0.0
    }

    pub fn hp_ratio(&self) -> f32 {
        if self.max_hp <= 0 {
            0.0
        } else {
            self.hp as f32 / self.max_hp as f32
        }
    }

    pub fn tick(&mut self, dt_secs: f32) {
        if self.banish_remaining_secs > 0.0 {
            self.banish_remaining_secs = (self.banish_remaining_secs - dt_secs).max(0.0);
            if self.banish_remaining_secs == 0.0 {
                self.banish_spell = None;
            }
        }
        if self.stunned_secs > 0.0 {
            self.stunned_secs = (self.stunned_secs - dt_secs).max(0.0);
        }
    }

    /// Direkter Schaden bricht das 30-Sek.-Bann-Sheep sofort!
    pub fn apply_direct_damage(&mut self, dmg: i32) -> Result<i32, DomainError> {
        if self.invulnerable_phase {
            return Err(DomainError::BossCurrentlyInvulnerable);
        }
        if dmg > 0 && self.is_banished() {
            self.banish_remaining_secs = 0.0;
            self.banish_spell = None;
        }
        self.hp = (self.hp - dmg.max(0)).max(0);
        Ok(self.hp)
    }
}

/// Wendet das exklusive 30-Sek.-Bann-Sheep an (nur Winchester-Sniper, Runen-Kleriker, Salz-&-Benzin-Pyromant).
pub fn apply_banish_sheep(
    caster_class: CharacterClass,
    target: &mut EnemyCombatState,
) -> Result<&'static str, DomainError> {
    let spell_name = caster_class.banish_spell_name().ok_or_else(|| {
        DomainError::BanishRestrictedToRangedDd(caster_class.display_name().to_string())
    })?;
    target.banish_remaining_secs = BANISH_SHEEP_DURATION_SECS;
    target.banish_spell = Some(spell_name.to_string());
    Ok(spell_name)
}

/// Trickster-Köder / Der Verrückte: Notfall-Spott vom Tank -> kassiert 1 Treffer -> Chaos-Sprung
/// -> wandelt Schaden in AoE-Gruppenbuff um -> Aggro springt sofort sauber zurück zum Tank (Trolling-Schutz!).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TricksterTauntOutcome {
    pub absorbed_damage: i32,
    pub chaos_jump_pos: (f32, f32),
    pub group_buff_power: f32,
    pub aggro_restored_to_tank: Uuid,
}

pub fn execute_trickster_emergency_taunt(
    aggro: &mut TankAggroTracker,
    trickster_id: Uuid,
    tank_id: Uuid,
    incoming_boss_hit: i32,
    current_pos: (f32, f32),
    chaos_angle_rad: f32,
) -> TricksterTauntOutcome {
    let tank_threat_before = aggro.current_threat(tank_id).max(100.0);
    // Kurzfristig Spott übernehmen für exakt den eingehenden Treffer
    aggro
        .threat_table
        .insert(trickster_id, tank_threat_before + 50.0);

    // Treffer einstecken -> Chaos-Sprung (4m Versatz) -> Gruppenbuff berechnen
    let jump_x = current_pos.0 + chaos_angle_rad.cos() * 4.0;
    let jump_y = current_pos.1 + chaos_angle_rad.sin() * 4.0;
    let group_buff_power = (incoming_boss_hit.max(1) as f32) * 0.35;

    // Trolling-Schutz: Nach dem Treffer springt die Aggro automatisch sauber zurück zum Tank!
    aggro
        .threat_table
        .insert(tank_id, tank_threat_before + incoming_boss_hit as f32);
    aggro
        .threat_table
        .insert(trickster_id, tank_threat_before * 0.40);

    TricksterTauntOutcome {
        absorbed_damage: incoming_boss_hit,
        chaos_jump_pos: (jump_x, jump_y),
        group_buff_power,
        aggro_restored_to_tank: aggro.top_target().unwrap_or(tank_id),
    }
}

/// Winchester-Sniper: Vorderlader/Colt (langsam), Armbrust (mittel), Bogen (ultraschnell).
/// Stillstand-Feuern stapelt Bonusschaden; jede Bewegung resettet sofort auf Standard (0 Stacks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SniperWeaponMode {
    ColtRifleSlow,
    CrossbowMedium,
    HuntingBowUltraFast,
}

impl SniperWeaponMode {
    pub fn cooldown_ms(self) -> u32 {
        match self {
            Self::ColtRifleSlow => 2400,
            Self::CrossbowMedium => 1400,
            Self::HuntingBowUltraFast => 650,
        }
    }

    pub fn base_damage(self) -> f32 {
        match self {
            Self::ColtRifleSlow => 210.0,
            Self::CrossbowMedium => 130.0,
            Self::HuntingBowUltraFast => 68.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SniperStance {
    pub weapon_mode: SniperWeaponMode,
    pub stationary_stacks: u32,
    pub max_stacks: u32,
}

impl Default for SniperStance {
    fn default() -> Self {
        Self {
            weapon_mode: SniperWeaponMode::ColtRifleSlow,
            stationary_stacks: 0,
            max_stacks: 5,
        }
    }
}

impl SniperStance {
    /// Jede Bewegung (> 0.001m) setzt den Stillstand-Bonus sofort auf Standard zurück!
    pub fn on_movement(&mut self, dx: f32, dy: f32) {
        if (dx * dx + dy * dy) > 0.000_001 {
            self.stationary_stacks = 0;
        }
    }

    /// Feuert im Stillstand: berechnet Schaden mit aktuellem Bonus und erhöht danach den Stack.
    pub fn fire_stationary_shot(&mut self) -> f32 {
        let bonus_mult = 1.0 + (self.stationary_stacks as f32 * 0.12);
        let dmg = self.weapon_mode.base_damage() * bonus_mult;
        self.stationary_stacks = (self.stationary_stacks + 1).min(self.max_stacks);
        dmg
    }
}

/// Runen-Kleriker: Bis zu 20% (1/5) des getroffenen Schadens werden zu Gruppen-/Eigenschilde,
/// begrenzt durch ein faires Schild-Cap pro Ziel (bei leicht geringerem Basisschaden).
pub fn execute_rune_cleric_salvo(
    raw_spell_power: f32,
    race: PlayableRace,
    current_target_shield: f32,
    max_shield_cap: f32,
) -> (i32, f32) {
    // Leicht geringerer Basisschaden (0.90x) als Ausgleich für die Schild-Konversion
    let dealt_damage = (raw_spell_power * 0.90).round() as i32;
    let glamour_mult = if race == PlayableRace::Fae { 1.20 } else { 1.0 };
    let generated_shield = (dealt_damage as f32) * RUNE_CLERIC_SHIELD_RATIO * glamour_mult;
    let new_shield = (current_target_shield + generated_shield).min(max_shield_cap);
    (dealt_damage, new_shield)
}

/// Salz-&-Benzin-Pyromant: 3 Feuerzauber/Molotows mit unterschiedlichen Castzeiten stapeln „Flamme“.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PyromancerFireSpell {
    QuickMolotow,      // 0.8s Castzeit
    SaltedBenzinBlast, // 1.6s Castzeit
    HellfireJerrycan,  // 2.6s Castzeit
}

impl PyromancerFireSpell {
    pub fn cast_time_ms(self) -> u32 {
        match self {
            Self::QuickMolotow => 800,
            Self::SaltedBenzinBlast => 1600,
            Self::HellfireJerrycan => 2600,
        }
    }

    pub fn flame_stacks_applied(self) -> u32 {
        match self {
            Self::QuickMolotow => 1,
            Self::SaltedBenzinBlast => 2,
            Self::HellfireJerrycan => 3,
        }
    }
}

pub fn cast_pyromancer_fire_spell(spell: PyromancerFireSpell, target: &mut EnemyCombatState) {
    target.flame_stacks += spell.flame_stacks_applied();
}

/// Instant-Spell „Flächenbrand“: Liegt „Flamme“ auf >= 3 Zielen, breitet er sie im 5-m-Radius um das Primärziel aus.
pub fn cast_pyromancer_wildfire(
    enemies: &mut [EnemyCombatState],
    epicenter: (f32, f32),
) -> Result<usize, DomainError> {
    let burning_count = enemies
        .iter()
        .filter(|e| e.flame_stacks >= 1 && e.hp > 0)
        .count();
    if burning_count < PYROMANCER_WILDFIRE_MIN_TARGETS {
        return Err(DomainError::PyromancerWildfireNeedsThreeTargets(
            burning_count,
        ));
    }

    let mut affected = 0usize;
    let r2 = PYROMANCER_WILDFIRE_RADIUS_METERS * PYROMANCER_WILDFIRE_RADIUS_METERS;
    for enemy in enemies.iter_mut().filter(|e| e.hp > 0) {
        let dx = enemy.pos_x - epicenter.0;
        let dy = enemy.pos_y - epicenter.1;
        if dx * dx + dy * dy <= r2 {
            enemy.flame_stacks += 2;
            affected += 1;
        }
    }
    Ok(affected)
}

/// Hoodoo-Brauer: Direkter Schaden ausschließlich über Voodoo-Puppen!
pub fn cast_hoodoo_direct_damage(
    via_voodoo_doll: bool,
    base_damage: i32,
    target: &mut EnemyCombatState,
) -> Result<i32, DomainError> {
    if !via_voodoo_doll {
        return Err(DomainError::HoodooDirectDamageRestricted);
    }
    target.apply_direct_damage(base_damage)
}

/// Sigillen-Elementarist: Keine Heilung; Schilde/Buffs ausschließlich als Boden-Sigille;
/// besitzt Notfall-Skill „Eigenblut-Sigille“ für unangreifbare Boss-Phasen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundSigil {
    pub center: (f32, f32),
    pub radius_m: f32,
    pub shield_value: f32,
    pub haste_bonus_pct: f32,
    pub damage_bonus_pct: f32,
}

pub fn sigil_elementalist_attempt_heal() -> Result<i32, DomainError> {
    Err(DomainError::SigilElementalistCannotHeal)
}

pub fn place_elementalist_sigil(
    as_ground_sigil: bool,
    center: (f32, f32),
    energy_spent: f32,
) -> Result<GroundSigil, DomainError> {
    if !as_ground_sigil {
        return Err(DomainError::SigilMustBeGroundPlaced);
    }
    Ok(GroundSigil {
        center,
        radius_m: 4.5,
        shield_value: energy_spent * 2.5,
        haste_bonus_pct: 12.0,
        damage_bonus_pct: 15.0,
    })
}

/// Blut-Siphon Notfall-Skill „Eigenblut-Spende“ (funktioniert explizit während unangreifbarer Boss-Phasen!).
pub fn cast_eigenblut_spende(healer_hp: &mut i32, ally_hp: &mut i32, ally_max_hp: i32) -> i32 {
    let sacrifice = (*healer_hp / 5).max(1);
    *healer_hp = (*healer_hp - sacrifice).max(1);
    let heal = sacrifice * 2;
    *ally_hp = (*ally_hp + heal).min(ally_max_hp);
    heal
}

/// Macheten-Berserker: Crits lösen Jagd-Raserei + stapelbare Blutungen aus; stärkere Blutungen erhöhen Schaden weiter.
pub fn execute_machete_berserker_strike(
    base_damage: f32,
    is_crit: bool,
    target: &mut EnemyCombatState,
) -> Result<(i32, bool), DomainError> {
    if is_crit {
        target.bleed_stacks = (target.bleed_stacks + 2).min(10);
    }
    let bleed_amp = 1.0 + (target.bleed_stacks as f32 * 0.08);
    let frenzy_amp = if is_crit { 1.50 } else { 1.0 };
    let total = (base_damage * bleed_amp * frenzy_amp).round() as i32;
    target.apply_direct_damage(total)?;
    Ok((total, is_crit))
}

/// Nacht-Pirschjäger: Ressource ausschließlich über Auto-Attacks + Boss-Ulti „Doppelagent“ ab <= 50% Boss-HP
/// (8 Sek. zum Schein auf Boss-Seite, kein Friendly-Fire-Kill möglich, stiehlt Boss-Buffs für Verrats-Finisher).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NightStalkerState {
    pub focus_resource: u32,
    pub doppelagent_remaining_secs: f32,
    pub stolen_buffs: Vec<String>,
}

impl NightStalkerState {
    pub fn gain_resource(&mut self, is_auto_attack: bool) -> Result<u32, DomainError> {
        if !is_auto_attack {
            return Err(DomainError::StalkerResourceRequiresAutoAttack);
        }
        self.focus_resource = (self.focus_resource + 20).min(100);
        Ok(self.focus_resource)
    }

    pub fn activate_doppelagent(
        &mut self,
        boss: &mut EnemyCombatState,
    ) -> Result<Vec<String>, DomainError> {
        if !boss.is_boss || boss.hp_ratio() > 0.50 {
            return Err(DomainError::BossUltiRequiresBossBelowHalfHp);
        }
        self.doppelagent_remaining_secs = DOPPELAGENT_DURATION_SECS;
        let stolen = std::mem::take(&mut boss.active_boss_buffs);
        self.stolen_buffs = stolen.clone();
        Ok(stolen)
    }

    /// Schein-Angriff während Doppelagent: darf Verbündete NIEMALS töten (HP bleibt immer >= 1).
    pub fn apply_feigned_friendly_hit(ally_hp: &mut i32, raw_hit: i32) -> i32 {
        *ally_hp = (*ally_hp - raw_hit.max(0)).max(1);
        *ally_hp
    }
}
