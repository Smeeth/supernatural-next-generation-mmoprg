// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::equipment::{EquipmentLoadout, EssenceWallet};
use crate::error::DomainError;

/// Die 7 spielbaren Rassen auf New Lawrence.
/// Engel und Dämonen sind auf Domain-Ebene explizit ausgeschlossen!
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayableRace {
    /// 1. Winchester-Abkömmling (8 Klassen) — Passiv: „Family Business“
    WinchesterDescendant,
    /// 2. Mann der Schriften (7 Klassen) — Passiv: „Archivwissen“
    ManOfLetters,
    /// 3. Vampir – Alpha-Blutlinie (7 Klassen) — Passiv: „Blutdurst“
    VampireAlpha,
    /// 4. Werwolf – Rudel-Überlebende (7 Klassen) — Passiv: „Rudelzähigkeit“
    WerewolfPack,
    /// 5. Gestaltwandler (7 Klassen, niemals Tank) — Passiv: „Häutung“
    Shapeshifter,
    /// 6. Hexe / Hexer (6 Klassen, kein Tank, keine schweren Stahl-Nahkampfwaffen) — Passiv: „Buch der Verdammten“
    Witch,
    /// 7. Fee / Fae (6 Klassen, kein Tank, absolutes Eisen-Tabu) — Passiv: „Avalon-Glamour“
    Fae,
}

impl PlayableRace {
    pub const ALL: [PlayableRace; 7] = [
        PlayableRace::WinchesterDescendant,
        PlayableRace::ManOfLetters,
        PlayableRace::VampireAlpha,
        PlayableRace::WerewolfPack,
        PlayableRace::Shapeshifter,
        PlayableRace::Witch,
        PlayableRace::Fae,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::WinchesterDescendant => "Winchester-Abkömmling",
            Self::ManOfLetters => "Mann der Schriften",
            Self::VampireAlpha => "Vampir – Alpha-Blutlinie",
            Self::WerewolfPack => "Werwolf – Rudel-Überlebende",
            Self::Shapeshifter => "Gestaltwandler",
            Self::Witch => "Hexe / Hexer",
            Self::Fae => "Fee / Fae",
        }
    }

    pub fn passive_name(self) -> &'static str {
        match self {
            Self::WinchesterDescendant => "Family Business",
            Self::ManOfLetters => "Archivwissen",
            Self::VampireAlpha => "Blutdurst",
            Self::WerewolfPack => "Rudelzähigkeit",
            Self::Shapeshifter => "Häutung",
            Self::Witch => "Buch der Verdammten",
            Self::Fae => "Avalon-Glamour",
        }
    }

    pub fn can_be_tank(self) -> bool {
        !matches!(self, Self::Shapeshifter | Self::Witch | Self::Fae)
    }

    pub fn has_iron_taboo(self) -> bool {
        matches!(self, Self::Fae)
    }

    pub fn forbids_heavy_melee_steel(self) -> bool {
        matches!(self, Self::Witch)
    }

    pub fn is_non_human_monster_or_fae(self) -> bool {
        matches!(
            self,
            Self::VampireAlpha | Self::WerewolfPack | Self::Shapeshifter | Self::Fae
        )
    }

    /// Strikter Parser inkl. harter Zurückweisung von Engeln & Dämonen.
    pub fn parse_strict(raw: &str) -> Result<Self, DomainError> {
        let norm = raw.trim().to_lowercase().replace(['-', ' '], "_");
        match norm.as_str() {
            "angel" | "engel" | "erzengel" | "archangel" | "seraph" | "demon" | "daemon"
            | "dämon" | "knight_of_hell" | "ritter_der_hoelle" | "ritter_der_hölle" => Err(
                DomainError::ForbiddenSupernaturalOrigin(raw.trim().to_string()),
            ),
            "winchester"
            | "winchester_descendant"
            | "winchester_abkoemmling"
            | "winchester_abkömmling" => Ok(Self::WinchesterDescendant),
            "man_of_letters" | "mann_der_schriften" | "men_of_letters" => Ok(Self::ManOfLetters),
            "vampire" | "vampir" | "vampire_alpha" | "vampir_alpha_blutlinie" => {
                Ok(Self::VampireAlpha)
            }
            "werewolf" | "werwolf" | "werewolf_pack" | "werwolf_rudel" => Ok(Self::WerewolfPack),
            "shapeshifter" | "gestaltwandler" | "wandler" => Ok(Self::Shapeshifter),
            "witch" | "hexe" | "hexer" | "hexe_hexer" => Ok(Self::Witch),
            "fae" | "fee" | "fee_fae" => Ok(Self::Fae),
            _ => Err(DomainError::UnknownRace(raw.trim().to_string())),
        }
    }

    pub fn as_slug(self) -> &'static str {
        match self {
            Self::WinchesterDescendant => "winchester_descendant",
            Self::ManOfLetters => "man_of_letters",
            Self::VampireAlpha => "vampire_alpha",
            Self::WerewolfPack => "werewolf_pack",
            Self::Shapeshifter => "shapeshifter",
            Self::Witch => "witch",
            Self::Fae => "fae",
        }
    }
}

/// Übermächtige Fraktionen existieren ausschließlich für NPC-Bosse und Welt-Invasionen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NpcOverpowerFaction {
    HellLegion,
    HeavenGarrison,
    PurgatoryApex,
    Cultists,
}

/// Die 4 Grundrollen (Bruch mit der Holy Trinity) sowie Unterteilung Nah-/Fernkampf-DD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BaseRole {
    Tank,
    Healer,
    Support,
    DamageDealer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClassCategory {
    Tank,
    Healer,
    Support,
    MeleeDd,
    RangedDd,
}

impl ClassCategory {
    pub fn base_role(self) -> BaseRole {
        match self {
            Self::Tank => BaseRole::Tank,
            Self::Healer => BaseRole::Healer,
            Self::Support => BaseRole::Support,
            Self::MeleeDd | Self::RangedDd => BaseRole::DamageDealer,
        }
    }
}

/// Die 15 Klassen (exakt 3 pro Kategorie).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CharacterClass {
    // Tanks (3)
    EisenKrieger,
    BlutdurstTank,
    BannWaechter,
    // Heiler (3)
    BlutSiphon,
    BannMoench,
    HoodooBrauer,
    // Support (3)
    BunkerTueftler,
    SigillenElementarist,
    TricksterKoeder,
    // Nahkampf-DDs (3)
    MachetenBerserker,
    KneipenPruegler,
    NachtPirschjaeger,
    // Fernkampf-DDs (3 — exklusives 30-Sek.-Bann-Sheep)
    WinchesterSniper,
    RunenKleriker,
    SalzUndBenzinPyromant,
}

impl CharacterClass {
    pub const ALL: [CharacterClass; 15] = [
        Self::EisenKrieger,
        Self::BlutdurstTank,
        Self::BannWaechter,
        Self::BlutSiphon,
        Self::BannMoench,
        Self::HoodooBrauer,
        Self::BunkerTueftler,
        Self::SigillenElementarist,
        Self::TricksterKoeder,
        Self::MachetenBerserker,
        Self::KneipenPruegler,
        Self::NachtPirschjaeger,
        Self::WinchesterSniper,
        Self::RunenKleriker,
        Self::SalzUndBenzinPyromant,
    ];

    pub fn category(self) -> ClassCategory {
        match self {
            Self::EisenKrieger | Self::BlutdurstTank | Self::BannWaechter => ClassCategory::Tank,
            Self::BlutSiphon | Self::BannMoench | Self::HoodooBrauer => ClassCategory::Healer,
            Self::BunkerTueftler | Self::SigillenElementarist | Self::TricksterKoeder => {
                ClassCategory::Support
            }
            Self::MachetenBerserker | Self::KneipenPruegler | Self::NachtPirschjaeger => {
                ClassCategory::MeleeDd
            }
            Self::WinchesterSniper | Self::RunenKleriker | Self::SalzUndBenzinPyromant => {
                ClassCategory::RangedDd
            }
        }
    }

    pub fn base_role(self) -> BaseRole {
        self.category().base_role()
    }

    pub fn can_cast_banish_sheep(self) -> bool {
        matches!(self.category(), ClassCategory::RangedDd)
    }

    pub fn banish_spell_name(self) -> Option<&'static str> {
        match self {
            Self::WinchesterSniper => Some("Blend-Augenbinde"),
            Self::RunenKleriker => Some("Teufelsfalle / Bann-Sigille"),
            Self::SalzUndBenzinPyromant => Some("Heiliger-Öl-Feuerring"),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::EisenKrieger => "Eisen-Krieger",
            Self::BlutdurstTank => "Blutdurst-Tank",
            Self::BannWaechter => "Bann-Wächter",
            Self::BlutSiphon => "Blut-Siphon",
            Self::BannMoench => "Bann-Mönch",
            Self::HoodooBrauer => "Hoodoo-Brauer",
            Self::BunkerTueftler => "Bunker-Tüftler",
            Self::SigillenElementarist => "Sigillen-Elementarist",
            Self::TricksterKoeder => "Trickster-Köder",
            Self::MachetenBerserker => "Macheten-Berserker",
            Self::KneipenPruegler => "Kneipen-Prügler",
            Self::NachtPirschjaeger => "Nacht-Pirschjäger",
            Self::WinchesterSniper => "Winchester-Sniper",
            Self::RunenKleriker => "Runen-Kleriker",
            Self::SalzUndBenzinPyromant => "Salz-&-Benzin-Pyromant",
        }
    }

    pub fn as_slug(self) -> &'static str {
        match self {
            Self::EisenKrieger => "eisen_krieger",
            Self::BlutdurstTank => "blutdurst_tank",
            Self::BannWaechter => "bann_waechter",
            Self::BlutSiphon => "blut_siphon",
            Self::BannMoench => "bann_moench",
            Self::HoodooBrauer => "hoodoo_brauer",
            Self::BunkerTueftler => "bunker_tueftler",
            Self::SigillenElementarist => "sigillen_elementarist",
            Self::TricksterKoeder => "trickster_koeder",
            Self::MachetenBerserker => "macheten_berserker",
            Self::KneipenPruegler => "kneipen_pruegler",
            Self::NachtPirschjaeger => "nacht_pirschjaeger",
            Self::WinchesterSniper => "winchester_sniper",
            Self::RunenKleriker => "runen_kleriker",
            Self::SalzUndBenzinPyromant => "salz_und_benzin_pyromant",
        }
    }

    pub fn parse_strict(raw: &str) -> Result<Self, DomainError> {
        let norm = raw
            .trim()
            .to_lowercase()
            .replace('&', "und")
            .replace(['-', ' '], "_");
        match norm.as_str() {
            "eisen_krieger" => Ok(Self::EisenKrieger),
            "blutdurst_tank" => Ok(Self::BlutdurstTank),
            "bann_waechter" | "bann_wächter" => Ok(Self::BannWaechter),
            "blut_siphon" => Ok(Self::BlutSiphon),
            "bann_moench" | "bann_mönch" => Ok(Self::BannMoench),
            "hoodoo_brauer" => Ok(Self::HoodooBrauer),
            "bunker_tueftler" | "bunker_tüftler" => Ok(Self::BunkerTueftler),
            "sigillen_elementarist" => Ok(Self::SigillenElementarist),
            "trickster_koeder" | "trickster_köder" | "der_verrueckte" | "der_verrückte" => {
                Ok(Self::TricksterKoeder)
            }
            "macheten_berserker" => Ok(Self::MachetenBerserker),
            "kneipen_pruegler" | "kneipen_prügler" => Ok(Self::KneipenPruegler),
            "nacht_pirschjaeger" | "nacht_pirschjäger" => Ok(Self::NachtPirschjaeger),
            "winchester_sniper" => Ok(Self::WinchesterSniper),
            "runen_kleriker" => Ok(Self::RunenKleriker),
            "salz_und_benzin_pyromant" | "salz__und__benzin_pyromant" | "pyromant" => {
                Ok(Self::SalzUndBenzinPyromant)
            }
            _ => Err(DomainError::UnknownClass(raw.trim().to_string())),
        }
    }

    /// Exakte Rassen-Freigabe pro Klasse gemäß Design-Spezifikation (Summe = 48 Kombinationen).
    pub fn allowed_races(self) -> &'static [PlayableRace] {
        use PlayableRace::*;
        match self {
            // Tanks (3)
            Self::EisenKrieger => &[WinchesterDescendant, VampireAlpha, WerewolfPack],
            Self::BlutdurstTank => &[VampireAlpha, WerewolfPack],
            Self::BannWaechter => &[WinchesterDescendant, ManOfLetters],
            // Heiler (3)
            Self::BlutSiphon => &[VampireAlpha, Shapeshifter, Witch],
            Self::BannMoench => &[WinchesterDescendant, ManOfLetters, Fae],
            Self::HoodooBrauer => &[WerewolfPack, Shapeshifter, Witch, Fae],
            // Support (3)
            Self::BunkerTueftler => &[WinchesterDescendant, ManOfLetters, Shapeshifter],
            Self::SigillenElementarist => &[ManOfLetters, VampireAlpha, Witch, Fae],
            Self::TricksterKoeder => &[WerewolfPack, Shapeshifter, Witch, Fae],
            // Nahkampf-DDs (3)
            Self::MachetenBerserker => &[WinchesterDescendant, VampireAlpha, WerewolfPack],
            Self::KneipenPruegler => &[WinchesterDescendant, WerewolfPack, Shapeshifter, Fae],
            Self::NachtPirschjaeger => &[VampireAlpha, WerewolfPack, Shapeshifter],
            // Fernkampf-DDs (3)
            Self::WinchesterSniper => &[
                WinchesterDescendant,
                ManOfLetters,
                VampireAlpha,
                Shapeshifter,
            ],
            Self::RunenKleriker => &[ManOfLetters, Witch, Fae],
            Self::SalzUndBenzinPyromant => &[WinchesterDescendant, ManOfLetters, Witch],
        }
    }
}

/// Prüft die Rassen-Klassen-Matrix inklusive Tank-Verbot für Gestaltwandler, Hexe und Fee.
pub fn validate_race_class(race: PlayableRace, class: CharacterClass) -> Result<(), DomainError> {
    if class.category() == ClassCategory::Tank && !race.can_be_tank() {
        return Err(DomainError::TankRoleForbidden {
            race: race.display_name().to_string(),
            class: class.display_name().to_string(),
        });
    }
    if !class.allowed_races().contains(&race) {
        return Err(DomainError::InvalidRaceClassCombination {
            race: race.display_name().to_string(),
            class: class.display_name().to_string(),
        });
    }
    Ok(())
}

/// Liefert alle erlaubten Klassen für eine Rasse (Winchester=8, MoL=7, Vampir=7, Werwolf=7, Wandler=7, Hexe=6, Fee=6).
pub fn allowed_classes_for_race(race: PlayableRace) -> Vec<CharacterClass> {
    CharacterClass::ALL
        .into_iter()
        .filter(|c| validate_race_class(race, *c).is_ok())
        .collect()
}

pub const STARTING_ZONE_ROADHOUSE: &str = "zone_roadhouse";

/// Vollständiges Charakter-Datenmodell (Spieler & ausgerüstete NPCs) inkl. 16-Slot-Ausrüstung.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterSheet {
    pub id: Uuid,
    pub account_id: Uuid,
    pub name: String,
    pub race: PlayableRace,
    pub class: CharacterClass,
    pub current_zone: String,
    pub pos_x: f32,
    pub pos_y: f32,
    pub hp: i32,
    pub max_hp: i32,
    pub equipment: EquipmentLoadout,
    pub essences: EssenceWallet,
    pub salt_ward_active: bool,
}

impl CharacterSheet {
    /// Erstellt einen neuen Spielercharakter zwingend in `zone_roadhouse` mit 16 grauen Ausrüstungsteilen.
    pub fn create_new_hunter(
        account_id: Uuid,
        name: impl Into<String>,
        race: PlayableRace,
        class: CharacterClass,
    ) -> Result<Self, DomainError> {
        validate_race_class(race, class)?;
        let base_hp = match class.category() {
            ClassCategory::Tank => 1500,
            ClassCategory::MeleeDd => 1150,
            ClassCategory::Support => 1050,
            ClassCategory::Healer => 1000,
            ClassCategory::RangedDd => 950,
        };
        let max_hp = if race == PlayableRace::WerewolfPack {
            (base_hp as f32 * 1.20) as i32
        } else {
            base_hp
        };
        let equipment = EquipmentLoadout::generate_starter_set(race, class)?;
        Ok(Self {
            id: Uuid::new_v4(),
            account_id,
            name: name.into(),
            race,
            class,
            current_zone: STARTING_ZONE_ROADHOUSE.to_string(),
            pos_x: 0.0,
            pos_y: 0.0,
            hp: max_hp,
            max_hp,
            equipment,
            essences: EssenceWallet::default(),
            salt_ward_active: false,
        })
    }
}
