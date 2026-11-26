// SPDX-License-Identifier: GPL-3.0-or-later

pub mod character;
pub mod combat;
pub mod equipment;
pub mod error;
pub mod world;

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::character::{
        allowed_classes_for_race, validate_race_class, CharacterClass, CharacterSheet,
        ClassCategory, PlayableRace, STARTING_ZONE_ROADHOUSE,
    };
    use crate::combat::{
        apply_banish_sheep, cast_eigenblut_spende, cast_hoodoo_direct_damage,
        cast_pyromancer_fire_spell, cast_pyromancer_wildfire, execute_machete_berserker_strike,
        execute_rune_cleric_salvo, execute_trickster_emergency_taunt, place_elementalist_sigil,
        racial_incoming_damage_multiplier, sigil_elementalist_attempt_heal, DamageElement,
        EnemyCombatState, NightStalkerState, PyromancerFireSpell, SniperStance, TankAggroTracker,
        BANISH_SHEEP_DURATION_SECS,
    };
    use crate::equipment::{
        EquipmentItem, EquipmentLoadout, EquipmentSlot, EssenceSource, EssenceType, ItemQuality,
        MaterialTag, WeaponArchetype,
    };
    use crate::error::DomainError;
    use crate::world::{
        IrcSlashCommand, MalphasEncounter, OmenIndex, QuestCatalog, SarielEncounter, ZoneCatalog,
    };

    #[test]
    fn rejects_angels_and_demons_strictly() {
        for forbidden in [
            "Engel",
            "angel",
            "Erzengel",
            "Dämon",
            "daemon",
            "Demon",
            "Ritter der Hölle",
        ] {
            let err = PlayableRace::parse_strict(forbidden).unwrap_err();
            assert!(
                matches!(err, DomainError::ForbiddenSupernaturalOrigin(_)),
                "Expected ForbiddenSupernaturalOrigin for {forbidden}, got {err:?}"
            );
        }
    }

    #[test]
    fn verifies_exact_race_class_matrix_and_tank_bans() {
        // Exakte Klassenanzahl pro Rasse gemäß Abschnitt 4.B
        assert_eq!(
            allowed_classes_for_race(PlayableRace::WinchesterDescendant).len(),
            8
        );
        assert_eq!(
            allowed_classes_for_race(PlayableRace::ManOfLetters).len(),
            7
        );
        assert_eq!(
            allowed_classes_for_race(PlayableRace::VampireAlpha).len(),
            7
        );
        assert_eq!(
            allowed_classes_for_race(PlayableRace::WerewolfPack).len(),
            7
        );
        assert_eq!(
            allowed_classes_for_race(PlayableRace::Shapeshifter).len(),
            7
        );
        assert_eq!(allowed_classes_for_race(PlayableRace::Witch).len(), 6);
        assert_eq!(allowed_classes_for_race(PlayableRace::Fae).len(), 6);

        // Exakt 3 Klassen pro Kategorie (15 gesamt)
        for cat in [
            ClassCategory::Tank,
            ClassCategory::Healer,
            ClassCategory::Support,
            ClassCategory::MeleeDd,
            ClassCategory::RangedDd,
        ] {
            let count = CharacterClass::ALL
                .iter()
                .filter(|c| c.category() == cat)
                .count();
            assert_eq!(count, 3, "Expected 3 classes in {cat:?}");
        }

        // Tank-Verbot für Gestaltwandler, Hexe und Fee
        for non_tank_race in [
            PlayableRace::Shapeshifter,
            PlayableRace::Witch,
            PlayableRace::Fae,
        ] {
            for tank_class in [
                CharacterClass::EisenKrieger,
                CharacterClass::BlutdurstTank,
                CharacterClass::BannWaechter,
            ] {
                let err = validate_race_class(non_tank_race, tank_class).unwrap_err();
                assert!(matches!(err, DomainError::TankRoleForbidden { .. }));
            }
        }
    }

    #[test]
    fn enforces_fae_iron_taboo_and_witch_heavy_steel_taboo() {
        let mut fae_gear =
            EquipmentLoadout::generate_starter_set(PlayableRace::Fae, CharacterClass::BannMoench)
                .unwrap();
        let iron_blade = EquipmentItem {
            slot: EquipmentSlot::MainHand,
            item_code: "cold_iron_poker".into(),
            display_name: "Reines Eisen-Schüreisen".into(),
            quality: ItemQuality::Grey,
            material: MaterialTag::PureIron,
            weapon_archetype: Some(WeaponArchetype::IronBladeOrRod),
            power_score: 15,
            capacity: 0,
            access_time_ms: 0,
        };
        assert!(matches!(
            fae_gear.equip_item(PlayableRace::Fae, iron_blade),
            Err(DomainError::FaeIronTaboo { .. })
        ));
        assert!(
            racial_incoming_damage_multiplier(PlayableRace::Fae, DamageElement::PureIron) > 1.4
        );

        let mut witch_gear = EquipmentLoadout::generate_starter_set(
            PlayableRace::Witch,
            CharacterClass::SalzUndBenzinPyromant,
        )
        .unwrap();
        let steel_greatsword = EquipmentItem {
            slot: EquipmentSlot::MainHand,
            item_code: "steel_cleaver".into(),
            display_name: "Schwere Stahl-Machete".into(),
            quality: ItemQuality::Grey,
            material: MaterialTag::HeavySteel,
            weapon_archetype: Some(WeaponArchetype::HeavyMeleeSteel),
            power_score: 15,
            capacity: 0,
            access_time_ms: 0,
        };
        assert!(matches!(
            witch_gear.equip_item(PlayableRace::Witch, steel_greatsword),
            Err(DomainError::WitchHeavySteelTaboo { .. })
        ));
    }

    #[test]
    fn initializes_all_16_grey_slots_in_roadhouse_and_upgrades_with_essences() {
        let mut hunter = CharacterSheet::create_new_hunter(
            Uuid::new_v4(),
            "Dean_II",
            PlayableRace::ManOfLetters,
            CharacterClass::WinchesterSniper,
        )
        .unwrap();

        assert_eq!(hunter.current_zone, STARTING_ZONE_ROADHOUSE);
        assert_eq!(hunter.equipment.slots.len(), 16);
        for slot in EquipmentSlot::ALL {
            let item = hunter.equipment.get(slot).expect("slot present");
            assert_eq!(item.quality, ItemQuality::Grey);
        }

        // Bauchtasche (Slot 12) und Rucksack (Slot 13) prüfen
        let pouch_before = hunter.equipment.get(EquipmentSlot::Pouch).unwrap().clone();
        assert_eq!(pouch_before.capacity, 4);
        assert_eq!(pouch_before.access_time_ms, 2500);

        // Falsche Essenz für Grau -> Grün wird abgelehnt
        hunter.essences.grant(
            EssenceType::EnochianBunkerRunes,
            10,
            hunter.race,
            EssenceSource::HiddenMenOfLettersBunker,
        );
        assert!(matches!(
            hunter.equipment.upgrade_slot(
                hunter.race,
                EquipmentSlot::Pouch,
                EssenceType::EnochianBunkerRunes,
                &mut hunter.essences
            ),
            Err(DomainError::InvalidEssenceForTier { .. })
        ));

        // Mann der Schriften Passiv „Archivwissen“: Basis 5 -> 4 Steinsalz-Asche!
        hunter.essences.grant(
            EssenceType::RockSaltAsh,
            4,
            hunter.race,
            EssenceSource::QuestReward,
        );
        let new_q = hunter
            .equipment
            .upgrade_slot(
                hunter.race,
                EquipmentSlot::Pouch,
                EssenceType::RockSaltAsh,
                &mut hunter.essences,
            )
            .unwrap();
        assert_eq!(new_q, ItemQuality::Green);
        assert_eq!(hunter.essences.amount(EssenceType::RockSaltAsh), 0);

        let pouch_after = hunter.equipment.get(EquipmentSlot::Pouch).unwrap();
        assert!(pouch_after.capacity > pouch_before.capacity);
        assert!(pouch_after.access_time_ms < pouch_before.access_time_ms);
    }

    #[test]
    fn verifies_tank_aggro_balance_block_vs_passive_dodge() {
        let mut tracker = TankAggroTracker::default();
        let tank_id = Uuid::new_v4();
        let dd_id = Uuid::new_v4();

        // Tank blockt aktiv / steckt Schläge ein UND teilt Schaden aus -> hält Top-Aggro über DD
        tracker.record_active_block_or_hit_taken(tank_id, CharacterClass::EisenKrieger, 200.0);
        tracker.record_damage_dealt(
            tank_id,
            PlayableRace::WinchesterDescendant,
            CharacterClass::EisenKrieger,
            220.0,
            true,
        );
        tracker.record_damage_dealt(
            dd_id,
            PlayableRace::VampireAlpha,
            CharacterClass::MachetenBerserker,
            450.0,
            true,
        );
        assert_eq!(tracker.top_target(), Some(tank_id));

        // Weicht der Tank nur noch passiv aus / läuft weg, bricht seine Aggro massiv ein!
        tracker.record_passive_dodge_or_run_away(tank_id);
        assert_eq!(tracker.top_target(), Some(dd_id));
    }

    #[test]
    fn verifies_30s_banish_sheep_exclusivity_and_break_on_damage() {
        let mut hound = EnemyCombatState::new("Höllenhund", 1200, false, 0.0, 0.0);

        // Nahkampf-DD, Tank, Heiler, Support dürfen KEIN Bann-Sheep wirken
        assert!(matches!(
            apply_banish_sheep(CharacterClass::EisenKrieger, &mut hound),
            Err(DomainError::BanishRestrictedToRangedDd(_))
        ));
        assert!(matches!(
            apply_banish_sheep(CharacterClass::MachetenBerserker, &mut hound),
            Err(DomainError::BanishRestrictedToRangedDd(_))
        ));

        // Fernkampf-DD wirkt 30s Bann-Sheep
        let spell = apply_banish_sheep(CharacterClass::RunenKleriker, &mut hound).unwrap();
        assert_eq!(spell, "Teufelsfalle / Bann-Sigille");
        assert_eq!(hound.banish_remaining_secs, BANISH_SHEEP_DURATION_SECS);
        assert!(hound.is_banished());

        // Bricht sofort bei direktem Schaden
        hound.apply_direct_damage(25).unwrap();
        assert!(!hound.is_banished());
    }

    #[test]
    fn verifies_pyromancer_wildfire_three_targets_and_5m_radius() {
        let mut enemies = vec![
            EnemyCombatState::new("Kultist 1", 500, false, 0.0, 0.0),
            EnemyCombatState::new("Kultist 2", 500, false, 2.0, 1.0),
            EnemyCombatState::new("Kultist 3", 500, false, -3.0, 2.0),
            EnemyCombatState::new("Kultist Nah (4m)", 500, false, 4.0, 0.0),
            EnemyCombatState::new("Kultist Fern (9m)", 500, false, 9.0, 0.0),
        ];

        // Nur 2 Ziele brennen -> Flächenbrand schlägt fehl!
        cast_pyromancer_fire_spell(PyromancerFireSpell::QuickMolotow, &mut enemies[0]);
        cast_pyromancer_fire_spell(PyromancerFireSpell::SaltedBenzinBlast, &mut enemies[1]);
        assert!(matches!(
            cast_pyromancer_wildfire(&mut enemies, (0.0, 0.0)),
            Err(DomainError::PyromancerWildfireNeedsThreeTargets(2))
        ));

        // 3. Ziel in Flammen setzen -> Flächenbrand breitet sich im 5m-Radius aus (trifft 0..=3, NICHT 9m!)
        cast_pyromancer_fire_spell(PyromancerFireSpell::HellfireJerrycan, &mut enemies[2]);
        let hit_count = cast_pyromancer_wildfire(&mut enemies, (0.0, 0.0)).unwrap();
        assert_eq!(hit_count, 4);
        assert!(enemies[3].flame_stacks >= 2);
        assert_eq!(enemies[4].flame_stacks, 0);
    }

    #[test]
    fn verifies_sniper_movement_reset_rune_cleric_shield_cap_and_trickster_aggro_return() {
        // 1. Winchester-Sniper Stillstand-Stacks & Bewegungs-Reset
        let mut sniper = SniperStance::default();
        let shot1 = sniper.fire_stationary_shot();
        let shot2 = sniper.fire_stationary_shot();
        assert!(shot2 > shot1);
        assert_eq!(sniper.stationary_stacks, 2);
        sniper.on_movement(1.5, 0.0);
        assert_eq!(sniper.stationary_stacks, 0);
        let shot_after_move = sniper.fire_stationary_shot();
        assert!((shot_after_move - shot1).abs() < f32::EPSILON);

        // 2. Runen-Kleriker 20% Schild-Konversion + hartes Schild-Cap
        let (dmg, shield1) =
            execute_rune_cleric_salvo(200.0, PlayableRace::ManOfLetters, 0.0, 50.0);
        assert_eq!(dmg, 180);
        assert!((shield1 - 36.0).abs() < 0.01); // 20% von 180 = 36
        let (_, shield_capped) =
            execute_rune_cleric_salvo(200.0, PlayableRace::ManOfLetters, shield1, 50.0);
        assert!((shield_capped - 50.0).abs() < 0.01); // fair am Cap gedeckelt!

        // 3. Trickster-Köder Notfall-Spott & automatische Aggro-Rückgabe an den Tank
        let mut aggro = TankAggroTracker::default();
        let tank_id = Uuid::new_v4();
        let trickster_id = Uuid::new_v4();
        aggro.record_active_block_or_hit_taken(tank_id, CharacterClass::BannWaechter, 300.0);
        aggro.record_damage_dealt(
            tank_id,
            PlayableRace::ManOfLetters,
            CharacterClass::BannWaechter,
            250.0,
            true,
        );
        let outcome = execute_trickster_emergency_taunt(
            &mut aggro,
            trickster_id,
            tank_id,
            400,
            (0.0, 0.0),
            0.0,
        );
        assert_eq!(outcome.aggro_restored_to_tank, tank_id);
        assert_eq!(aggro.top_target(), Some(tank_id));
        assert!(outcome.group_buff_power > 100.0);
    }

    #[test]
    fn verifies_specialist_class_constraints_and_raid_encounters() {
        let mut boss = EnemyCombatState::new("Malphas", 1000, true, 0.0, 0.0);
        boss.active_boss_buffs.push("Dämonenmacht".into());

        // Hoodoo nur über Voodoo-Puppe
        assert!(matches!(
            cast_hoodoo_direct_damage(false, 80, &mut boss),
            Err(DomainError::HoodooDirectDamageRestricted)
        ));
        assert!(cast_hoodoo_direct_damage(true, 80, &mut boss).is_ok());

        // Sigillen-Elementarist keine Heilung, nur Boden-Sigille
        assert!(sigil_elementalist_attempt_heal().is_err());
        assert!(place_elementalist_sigil(false, (0.0, 0.0), 50.0).is_err());
        assert!(place_elementalist_sigil(true, (1.0, 2.0), 50.0).is_ok());

        // Macheten-Berserker Crit-Blutungs-Skalierung
        let (d1, _) = execute_machete_berserker_strike(100.0, true, &mut boss).unwrap();
        let (d2, _) = execute_machete_berserker_strike(100.0, true, &mut boss).unwrap();
        assert!(d2 > d1);

        // Nacht-Pirschjäger Doppelagent erst ab <= 50% Boss-HP & kein Friendly-Fire-Kill
        let mut stalker = NightStalkerState::default();
        assert!(stalker.gain_resource(false).is_err());
        assert_eq!(stalker.gain_resource(true).unwrap(), 20);
        assert!(stalker.activate_doppelagent(&mut boss).is_err());
        boss.hp = 450; // <= 50%
        let stolen = stalker.activate_doppelagent(&mut boss).unwrap();
        assert_eq!(stolen, vec!["Dämonenmacht".to_string()]);
        let mut ally_hp = 50;
        assert_eq!(
            NightStalkerState::apply_feigned_friendly_hit(&mut ally_hp, 9999),
            1
        );

        // Raid-Bosse Malphas & Sariel + Eigenblut-Spende in Himmelsphase
        let mut malphas = MalphasEncounter::new();
        let ev = malphas.tick(60.0);
        assert_eq!(malphas.hellhound_alphas.len(), 2);
        assert!(!ev.is_empty());

        let mut sariel = SarielEncounter::new();
        sariel.enter_sky_phase();
        assert!(sariel.boss.apply_direct_damage(200).is_err());
        let mut healer_hp = 800;
        let mut wounded_hp = 200;
        assert!(cast_eigenblut_spende(&mut healer_hp, &mut wounded_hp, 1000) > 0);

        // Datengetriebene Kataloge & IRC-Slash-Parser
        let zones = ZoneCatalog::load_embedded().unwrap();
        assert!(zones.find_zone("zone_roadhouse").is_some());
        let quests = QuestCatalog::load_embedded().unwrap();
        assert_eq!(quests.quests.len(), 3);
        assert_eq!(
            IrcSlashCommand::parse_chat_line("/salzen an der Tür"),
            Some(IrcSlashCommand::Salzen)
        );
        assert!(OmenIndex::HellgateBreach.requires_irc_radio_alert());
    }
}
