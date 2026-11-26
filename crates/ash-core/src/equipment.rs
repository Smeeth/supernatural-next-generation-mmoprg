// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::character::{CharacterClass, PlayableRace};
use crate::error::DomainError;

/// Exakt 16 feste Ausrüstungs-Slots für jeden Spieler- und NPC-Charakter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EquipmentSlot {
    Headgear = 1,       // 1. Kopfbedeckung
    FaceGuard = 2,      // 2. Gesichtsschutz
    Amulet = 3,         // 3. Amulett
    Cloak = 4,          // 4. Umhang/Mantel
    ChestArmor = 5,     // 5. Brustrüstung
    BracersGloves = 6,  // 6. Armschienen/Handschuhe
    MainHand = 7,       // 7. Hauptwaffe
    OffHand = 8,        // 8. Nebenwaffe/Schild
    Ring1 = 9,          // 9. Ring 1
    Ring2 = 10,         // 10. Ring 2
    Belt = 11,          // 11. Gürtel
    Pouch = 12,         // 12. Bauchtasche (Kampf-Schnellzugriff)
    Backpack = 13,      // 13. Rucksack ("Mobiler Impala-Kofferraum")
    LegGuards = 14,     // 14. Beinschutz
    Boots = 15,         // 15. Schuhe/Stiefel
    RelicFocus = 16,    // 16. Relikt/Fokus
}

impl EquipmentSlot {
    pub const ALL: [EquipmentSlot; 16] = [
        Self::Headgear,
        Self::FaceGuard,
        Self::Amulet,
        Self::Cloak,
        Self::ChestArmor,
        Self::BracersGloves,
        Self::MainHand,
        Self::OffHand,
        Self::Ring1,
        Self::Ring2,
        Self::Belt,
        Self::Pouch,
        Self::Backpack,
        Self::LegGuards,
        Self::Boots,
        Self::RelicFocus,
    ];

    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn from_index(idx: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|s| *s as u8 == idx)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Headgear => "Kopfbedeckung",
            Self::FaceGuard => "Gesichtsschutz",
            Self::Amulet => "Amulett",
            Self::Cloak => "Umhang/Mantel",
            Self::ChestArmor => "Brustrüstung",
            Self::BracersGloves => "Armschienen/Handschuhe",
            Self::MainHand => "Hauptwaffe",
            Self::OffHand => "Nebenwaffe/Schild",
            Self::Ring1 => "Ring 1",
            Self::Ring2 => "Ring 2",
            Self::Belt => "Gürtel",
            Self::Pouch => "Bauchtasche",
            Self::Backpack => "Rucksack",
            Self::LegGuards => "Beinschutz",
            Self::Boots => "Schuhe/Stiefel",
            Self::RelicFocus => "Relikt/Fokus",
        }
    }
}

/// 5 Qualitätsstufen ("Kofferraum-Prinzip" — Aufwertung statt Wegwerf-Loot).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ItemQuality {
    Grey = 0,   // Gewöhnlich (Startset)
    Green = 1,  // Geweiht (Steinsalz-Asche)
    Blue = 2,   // Silber-Geschmiedet (Geweihtes Silber & Monster-Blut)
    Purple = 3, // Bunker-Relikt (Henochische Bunker-Runen)
    Gold = 4,   // Legenden-Stufe / Colt-Klasse (Fegefeuer-Glut & Erzengel-Staub)
}

impl ItemQuality {
    pub fn from_tier(tier: u8) -> Option<Self> {
        match tier {
            0 => Some(Self::Grey),
            1 => Some(Self::Green),
            2 => Some(Self::Blue),
            3 => Some(Self::Purple),
            4 => Some(Self::Gold),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Grey => "Grau (Gewöhnlich)",
            Self::Green => "Grün (Geweiht)",
            Self::Blue => "Blau (Silber-Geschmiedet)",
            Self::Purple => "Violett (Bunker-Relikt)",
            Self::Gold => "Gold (Legenden-Stufe / Colt-Klasse)",
        }
    }

    pub fn next_tier(self) -> Option<(ItemQuality, EssenceType)> {
        match self {
            Self::Grey => Some((Self::Green, EssenceType::RockSaltAsh)),
            Self::Green => Some((Self::Blue, EssenceType::ConsecratedSilverAndMonsterBlood)),
            Self::Blue => Some((Self::Purple, EssenceType::EnochianBunkerRunes)),
            Self::Purple => Some((Self::Gold, EssenceType::PurgatoryEmberAndArchangelDust)),
            Self::Gold => None,
        }
    }

    pub fn base_upgrade_cost(self) -> u32 {
        match self {
            Self::Grey => 5,
            Self::Green => 8,
            Self::Blue => 12,
            Self::Purple => 15,
            Self::Gold => 0,
        }
    }

    /// Kapazität der Bauchtasche (Slot 12) skaliert mit Qualitätsstufe.
    pub fn pouch_capacity(self) -> u16 {
        match self {
            Self::Grey => 4,
            Self::Green => 5,
            Self::Blue => 6,
            Self::Purple => 7,
            Self::Gold => 8,
        }
    }

    /// Zugriffszeit der Bauchtasche in Millisekunden (kürzer bei höherer Qualität).
    pub fn pouch_access_time_ms(self) -> u32 {
        match self {
            Self::Grey => 2500,
            Self::Green => 2000,
            Self::Blue => 1600,
            Self::Purple => 1200,
            Self::Gold => 800,
        }
    }

    /// Kapazität des Rucksacks ("Mobiler Impala-Kofferraum", Slot 13).
    pub fn backpack_capacity(self) -> u16 {
        match self {
            Self::Grey => 20,
            Self::Green => 28,
            Self::Blue => 36,
            Self::Purple => 48,
            Self::Gold => 64,
        }
    }

    pub fn base_power_score(self) -> u32 {
        match self {
            Self::Grey => 10,
            Self::Green => 22,
            Self::Blue => 40,
            Self::Purple => 68,
            Self::Gold => 110,
        }
    }
}

/// Die 4 Okkulten Essenzen zur Aufwertung von Grau -> Gold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EssenceType {
    /// Grau -> Grün
    RockSaltAsh,
    /// Grün -> Blau
    ConsecratedSilverAndMonsterBlood,
    /// Blau -> Violett
    EnochianBunkerRunes,
    /// Violett -> Gold
    PurgatoryEmberAndArchangelDust,
}

impl EssenceType {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::RockSaltAsh => "Steinsalz-Asche",
            Self::ConsecratedSilverAndMonsterBlood => "Geweihtes Silber & Monster-Blut",
            Self::EnochianBunkerRunes => "Henochische Bunker-Runen",
            Self::PurgatoryEmberAndArchangelDust => "Fegefeuer-Glut & Erzengel-Staub",
        }
    }

    pub fn parse_slug(raw: &str) -> Option<Self> {
        match raw.trim() {
            "RockSaltAsh" | "rock_salt_ash" | "steinsalz_asche" => Some(Self::RockSaltAsh),
            "ConsecratedSilverAndMonsterBlood" | "consecrated_silver" | "silber_blut" => {
                Some(Self::ConsecratedSilverAndMonsterBlood)
            }
            "EnochianBunkerRunes" | "enochian_bunker_runes" | "bunker_runen" => {
                Some(Self::EnochianBunkerRunes)
            }
            "PurgatoryEmberAndArchangelDust" | "purgatory_ember" | "fegefeuer_glut" => {
                Some(Self::PurgatoryEmberAndArchangelDust)
            }
            _ => None,
        }
    }
}

/// Die 5 Essenz-Quellen im Loot- & Welt-System.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EssenceSource {
    QuestReward,
    WorldGathering,
    MonsterAndHellgateLoot,
    OptionalPvpBounty,
    HiddenMenOfLettersBunker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialTag {
    Cloth,
    Leather,
    AshWood,
    BrassRune,
    ConsecratedSilver,
    PureIron,
    HeavySteel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponArchetype {
    HeavyMeleeSteel,
    IronBladeOrRod,
    SilverMacheteOrClaws,
    ConsecratedStaffOrClub,
    RitualDaggerOrVoodoo,
    ColtRifle,
    Crossbow,
    HuntingBow,
    RuneTomeOrPyroRig,
    ShieldOrFocus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentItem {
    pub slot: EquipmentSlot,
    pub item_code: String,
    pub display_name: String,
    pub quality: ItemQuality,
    pub material: MaterialTag,
    pub weapon_archetype: Option<WeaponArchetype>,
    pub power_score: u32,
    pub capacity: u16,
    pub access_time_ms: u32,
}

impl EquipmentItem {
    pub fn validate_for_race(&self, race: PlayableRace) -> Result<(), DomainError> {
        if race.has_iron_taboo() && self.material == MaterialTag::PureIron {
            return Err(DomainError::FaeIronTaboo {
                item: self.display_name.clone(),
            });
        }
        if race.forbids_heavy_melee_steel()
            && (self.weapon_archetype == Some(WeaponArchetype::HeavyMeleeSteel)
                || (matches!(self.slot, EquipmentSlot::MainHand | EquipmentSlot::OffHand)
                    && self.material == MaterialTag::HeavySteel))
        {
            return Err(DomainError::WitchHeavySteelTaboo {
                item: self.display_name.clone(),
            });
        }
        Ok(())
    }
}

/// Schnellzugriffs-Verbrauchsgüter für die Bauchtasche (Slot 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PouchConsumable {
    RockSaltCartridge,
    HolyWaterFlask,
    RoadhouseBeer,
    FakeFbiBadge,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EssenceWallet {
    pub counts: BTreeMap<EssenceType, u32>,
}

impl EssenceWallet {
    pub fn amount(&self, essence: EssenceType) -> u32 {
        self.counts.get(&essence).copied().unwrap_or(0)
    }

    pub fn grant(&mut self, essence: EssenceType, base_amount: u32, race: PlayableRace, _source: EssenceSource) -> u32 {
        // Hexe/Hexer Passiv „Buch der Verdammten“: +20% höhere magische Essenz-Ausbeute (aufgerundet)
        let final_amount = if race == PlayableRace::Witch {
            ((base_amount as f32) * 1.20).ceil() as u32
        } else {
            base_amount
        };
        *self.counts.entry(essence).or_insert(0) += final_amount;
        final_amount
    }
}

/// 16-Slot-Ausrüstungs-Container für Spieler und NPCs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentLoadout {
    pub slots: BTreeMap<EquipmentSlot, EquipmentItem>,
    pub pouch_contents: Vec<PouchConsumable>,
}

impl EquipmentLoadout {
    /// Erzeugt das verbindliche 16-teilige Start-Equipment in Grau (Gewöhnlich),
    /// abgestimmt auf Rassen-Tabus (kein Eisen für Fae, kein schwerer Nahkampf-Stahl für Hexen).
    pub fn generate_starter_set(
        race: PlayableRace,
        class: CharacterClass,
    ) -> Result<Self, DomainError> {
        let armor_material = if race == PlayableRace::Fae {
            MaterialTag::Leather
        } else {
            MaterialTag::Cloth
        };

        let (main_name, main_mat, main_arch, off_name, off_mat, off_arch) =
            starter_weapons_for(race, class);

        let mut slots = BTreeMap::new();
        for slot in EquipmentSlot::ALL {
            let (code, name, mat, arch) = match slot {
                EquipmentSlot::Headgear => ("starter_head", "Abgewetzter Staubhut", armor_material, None),
                EquipmentSlot::FaceGuard => ("starter_face", "Salzgetränktes Halstuch", MaterialTag::Cloth, None),
                EquipmentSlot::Amulet => ("starter_amulet", "Schutz-Talisman an Lederschnur", MaterialTag::BrassRune, None),
                EquipmentSlot::Cloak => ("starter_cloak", "Verwitterter Highway-Mantel", armor_material, None),
                EquipmentSlot::ChestArmor => ("starter_chest", "Geflickte Jäger-Weste", MaterialTag::Leather, None),
                EquipmentSlot::BracersGloves => ("starter_gloves", "Griffige Lederhandschuhe", MaterialTag::Leather, None),
                EquipmentSlot::MainHand => ("starter_main", main_name, main_mat, Some(main_arch)),
                EquipmentSlot::OffHand => ("starter_off", off_name, off_mat, Some(off_arch)),
                EquipmentSlot::Ring1 => ("starter_ring1", "Angelaufener Messingring", MaterialTag::BrassRune, None),
                EquipmentSlot::Ring2 => ("starter_ring2", "Schlichter Siegelring", MaterialTag::ConsecratedSilver, None),
                EquipmentSlot::Belt => ("starter_belt", "Breiter Patronengurt", MaterialTag::Leather, None),
                EquipmentSlot::Pouch => ("starter_pouch", "Ausgebeulte Jäger-Bauchtasche", MaterialTag::Leather, None),
                EquipmentSlot::Backpack => ("starter_backpack", "Mobiler Kofferraum-Seesack", MaterialTag::Leather, None),
                EquipmentSlot::LegGuards => ("starter_legs", "Verstärkte Staub-Jeans", MaterialTag::Leather, None),
                EquipmentSlot::Boots => ("starter_boots", "Schwere Roadhouse-Stiefel", MaterialTag::Leather, None),
                EquipmentSlot::RelicFocus => ("starter_relic", starter_relic_name(race), MaterialTag::BrassRune, None),
            };

            let item = EquipmentItem {
                slot,
                item_code: code.to_string(),
                display_name: name.to_string(),
                quality: ItemQuality::Grey,
                material: mat,
                weapon_archetype: arch,
                power_score: ItemQuality::Grey.base_power_score(),
                capacity: match slot {
                    EquipmentSlot::Pouch => ItemQuality::Grey.pouch_capacity(),
                    EquipmentSlot::Backpack => ItemQuality::Grey.backpack_capacity(),
                    _ => 0,
                },
                access_time_ms: if slot == EquipmentSlot::Pouch {
                    ItemQuality::Grey.pouch_access_time_ms()
                } else {
                    0
                },
            };
            item.validate_for_race(race)?;
            slots.insert(slot, item);
        }

        let pouch_contents = if race == PlayableRace::Fae {
            vec![
                PouchConsumable::HolyWaterFlask,
                PouchConsumable::RoadhouseBeer,
                PouchConsumable::FakeFbiBadge,
            ]
        } else {
            vec![
                PouchConsumable::RockSaltCartridge,
                PouchConsumable::HolyWaterFlask,
                PouchConsumable::RoadhouseBeer,
                PouchConsumable::FakeFbiBadge,
            ]
        };

        Ok(Self {
            slots,
            pouch_contents,
        })
    }

    pub fn get(&self, slot: EquipmentSlot) -> Option<&EquipmentItem> {
        self.slots.get(&slot)
    }

    /// Rüstet ein neues Basis-Item aus (strikte Prüfung von Slot, Fae-Eisen-Tabu und Hexen-Stahl-Verbot).
    pub fn equip_item(
        &mut self,
        race: PlayableRace,
        item: EquipmentItem,
    ) -> Result<EquipmentItem, DomainError> {
        item.validate_for_race(race)?;
        let prev = self
            .slots
            .insert(item.slot, item)
            .expect("16 feste Ausrüstungs-Slots sind immer initialisiert");
        Ok(prev)
    }

    /// Wertet ein bestehendes Ausrüstungsteil per Okkulter Essenz auf die nächste Stufe auf.
    /// Berücksichtigt das Passiv „Archivwissen“ (Mann der Schriften: 20% Essenz-Rabatt).
    pub fn upgrade_slot(
        &mut self,
        race: PlayableRace,
        slot: EquipmentSlot,
        essence_provided: EssenceType,
        wallet: &mut EssenceWallet,
    ) -> Result<ItemQuality, DomainError> {
        let item = self
            .slots
            .get_mut(&slot)
            .expect("16 feste Ausrüstungs-Slots sind immer initialisiert");

        let (next_quality, required_essence) = item
            .quality
            .next_tier()
            .ok_or_else(|| DomainError::MaxQualityReached(slot.display_name().to_string()))?;

        if essence_provided != required_essence {
            return Err(DomainError::InvalidEssenceForTier {
                expected: required_essence.display_name().to_string(),
                provided: essence_provided.display_name().to_string(),
            });
        }

        let base_cost = item.quality.base_upgrade_cost();
        let effective_cost = upgrade_cost_for_race(race, base_cost);
        let available = wallet.amount(required_essence);
        if available < effective_cost {
            return Err(DomainError::InsufficientEssences {
                essence: required_essence.display_name().to_string(),
                required: effective_cost,
                available,
            });
        }

        if let Some(balance) = wallet.counts.get_mut(&required_essence) {
            *balance -= effective_cost;
        }

        item.quality = next_quality;
        item.power_score = next_quality.base_power_score();
        if slot == EquipmentSlot::Pouch {
            item.capacity = next_quality.pouch_capacity();
            item.access_time_ms = next_quality.pouch_access_time_ms();
        } else if slot == EquipmentSlot::Backpack {
            item.capacity = next_quality.backpack_capacity();
        }

        Ok(next_quality)
    }
}

/// Berechnet die Essenz-Kosten unter Berücksichtigung von „Archivwissen“ (Mann der Schriften).
pub fn upgrade_cost_for_race(race: PlayableRace, base_cost: u32) -> u32 {
    if race == PlayableRace::ManOfLetters {
        ((base_cost as f32 * 0.80).floor() as u32).max(1)
    } else {
        base_cost
    }
}

fn starter_relic_name(race: PlayableRace) -> &'static str {
    match race {
        PlayableRace::Witch => "Vernähter Hexenbeutel (Grau)",
        PlayableRace::ManOfLetters => "Henochisches Runenmodul Mk.I",
        PlayableRace::Fae => "Avalon-Glamour-Prisma",
        _ => "Analoger EMF-Detektor",
    }
}

fn starter_weapons_for(
    race: PlayableRace,
    class: CharacterClass,
) -> (
    &'static str,
    MaterialTag,
    WeaponArchetype,
    &'static str,
    MaterialTag,
    WeaponArchetype,
) {
    match class {
        CharacterClass::EisenKrieger => (
            "Geschmiedete Eisen-Machete",
            MaterialTag::PureIron,
            WeaponArchetype::IronBladeOrRod,
            "Verbeulter Schrottplatz-Schild",
            MaterialTag::HeavySteel,
            WeaponArchetype::ShieldOrFocus,
        ),
        CharacterClass::BlutdurstTank => (
            "Schartige Kurz-Machete",
            MaterialTag::ConsecratedSilver,
            WeaponArchetype::SilverMacheteOrClaws,
            "Zweite Jagd-Klaue",
            MaterialTag::ConsecratedSilver,
            WeaponArchetype::SilverMacheteOrClaws,
        ),
        CharacterClass::BannWaechter => (
            "Schwere 2H-Eisenstange",
            MaterialTag::PureIron,
            WeaponArchetype::IronBladeOrRod,
            "Henochisches Schutz-Tattoo-Set",
            MaterialTag::BrassRune,
            WeaponArchetype::ShieldOrFocus,
        ),
        CharacterClass::BlutSiphon => (
            "Geätztes Aderlass-Messer",
            MaterialTag::ConsecratedSilver,
            WeaponArchetype::RitualDaggerOrVoodoo,
            "Blut-Phiole",
            MaterialTag::BrassRune,
            WeaponArchetype::ShieldOrFocus,
        ),
        CharacterClass::BannMoench => (
            "Geweihter Eschenstab",
            MaterialTag::AshWood,
            WeaponArchetype::ConsecratedStaffOrClub,
            "Abgegriffenes Exorzismus-Brevier",
            MaterialTag::Cloth,
            WeaponArchetype::ShieldOrFocus,
        ),
        CharacterClass::HoodooBrauer => (
            "Vernähte Voodoo-Puppe",
            MaterialTag::Cloth,
            WeaponArchetype::RitualDaggerOrVoodoo,
            "Kräuter-Mojo-Flasche",
            MaterialTag::AshWood,
            WeaponArchetype::ShieldOrFocus,
        ),
        CharacterClass::BunkerTueftler => {
            let mat = if race.has_iron_taboo() {
                MaterialTag::BrassRune
            } else {
                MaterialTag::PureIron
            };
            (
                "Bunker-Schraubenschlüssel",
                mat,
                WeaponArchetype::ConsecratedStaffOrClub,
                "Klappbares Salz-Geschütz-Gestell",
                MaterialTag::BrassRune,
                WeaponArchetype::ShieldOrFocus,
            )
        }
        CharacterClass::SigillenElementarist => (
            "Kreide- & Blut-Ritzgriffel",
            MaterialTag::BrassRune,
            WeaponArchetype::RuneTomeOrPyroRig,
            "Sigillen-Kompendium",
            MaterialTag::Cloth,
            WeaponArchetype::ShieldOrFocus,
        ),
        CharacterClass::TricksterKoeder => (
            "Angesengter Holz-Spazierstock",
            MaterialTag::AshWood,
            WeaponArchetype::ConsecratedStaffOrClub,
            "Gezinktes Würfel-Set",
            MaterialTag::BrassRune,
            WeaponArchetype::ShieldOrFocus,
        ),
        CharacterClass::MachetenBerserker => (
            "Rechte Sägeblatt-Machete",
            MaterialTag::HeavySteel,
            WeaponArchetype::HeavyMeleeSteel,
            "Linke Sägeblatt-Machete",
            MaterialTag::HeavySteel,
            WeaponArchetype::HeavyMeleeSteel,
        ),
        CharacterClass::KneipenPruegler => (
            "Schweres Roadhouse-Billardqueue",
            MaterialTag::AshWood,
            WeaponArchetype::ConsecratedStaffOrClub,
            "Billiger Mais-Fusel-Krug",
            MaterialTag::Cloth,
            WeaponArchetype::ShieldOrFocus,
        ),
        CharacterClass::NachtPirschjaeger => (
            "Lautlose Jagdklinge",
            MaterialTag::ConsecratedSilver,
            WeaponArchetype::SilverMacheteOrClaws,
            "Wurf-Dorn",
            MaterialTag::ConsecratedSilver,
            WeaponArchetype::SilverMacheteOrClaws,
        ),
        CharacterClass::WinchesterSniper => (
            "Altes Vorderlader-Jagdgewehr",
            MaterialTag::BrassRune,
            WeaponArchetype::ColtRifle,
            "Kompakt-Armbrust (Blend-Bolzen)",
            MaterialTag::AshWood,
            WeaponArchetype::Crossbow,
        ),
        CharacterClass::RunenKleriker => (
            "Runen-beschlagenes Salven-Zepter",
            MaterialTag::BrassRune,
            WeaponArchetype::RuneTomeOrPyroRig,
            "Teufelsfallen-Schablone",
            MaterialTag::BrassRune,
            WeaponArchetype::ShieldOrFocus,
        ),
        CharacterClass::SalzUndBenzinPyromant => (
            "Umgebauter Benzin-Zünder",
            MaterialTag::BrassRune,
            WeaponArchetype::RuneTomeOrPyroRig,
            "Kiste Roadhouse-Molotows & Heiliges Öl",
            MaterialTag::Cloth,
            WeaponArchetype::ShieldOrFocus,
        ),
    }
}
