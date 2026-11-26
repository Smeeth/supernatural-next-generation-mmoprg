// SPDX-License-Identifier: GPL-3.0-or-later

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("Übermächtige Wesen ({0}) sind als Spieler-Rasse strikt verboten — nur als Raid-Boss oder Fraktionsgegner erlaubt")]
    ForbiddenSupernaturalOrigin(String),

    #[error("Unbekannte Rasse: {0}")]
    UnknownRace(String),

    #[error("Unbekannte Klasse: {0}")]
    UnknownClass(String),

    #[error("Rasse {race} darf niemals eine Tank-Klasse ({class}) spielen")]
    TankRoleForbidden { race: String, class: String },

    #[error("Ungültige Rassen-Klassen-Kombination: {race} + {class}")]
    InvalidRaceClassCombination { race: String, class: String },

    #[error("Absolutes Eisen-Tabu: Fee/Fae erleidet Verbrennungen durch reines Eisen ({item})")]
    FaeIronTaboo { item: String },

    #[error("Hexen-Restriktion: Schwere Nahkampf-Stahlwaffen ({item}) sind für Hexen/Hexer verboten")]
    WitchHeavySteelTaboo { item: String },

    #[error("Ausrüstungsteil gehört in Slot {expected:?}, nicht {actual:?}")]
    SlotMismatch { expected: String, actual: String },

    #[error("Ausrüstungsteil in Slot {0} hat bereits Gold-Stufe (Colt-Klasse) erreicht")]
    MaxQualityReached(String),

    #[error("Falsche Essenz für Aufwertung: benötigt {expected}, erhalten {provided}")]
    InvalidEssenceForTier { expected: String, provided: String },

    #[error("Zu wenig Essenzen ({essence}): benötigt {required}, vorhanden {available}")]
    InsufficientEssences {
        essence: String,
        required: u32,
        available: u32,
    },

    #[error("Exklusives 30-Sek.-Bann-Sheep ist ausschließlich den 3 Fernkampf-DD-Klassen vorbehalten (versucht von {0})")]
    BanishRestrictedToRangedDd(String),

    #[error("Hoodoo-Brauer darf direkten Schaden ausschließlich über Voodoo-Puppen verursachen")]
    HoodooDirectDamageRestricted,

    #[error("Sigillen-Elementarist besitzt keinerlei direkte Heilzauber und wirkt Buffs/Schilde nur als Boden-Sigillen")]
    SigilElementalistCannotHeal,

    #[error("Sigillen-Elementarist kann Buffs und Schilde ausschließlich als Boden-Sigille platzieren")]
    SigilMustBeGroundPlaced,

    #[error("Flächenbrand erfordert mindestens 3 Ziele mit aktivem Flamme-Effekt (aktuell: {0})")]
    PyromancerWildfireNeedsThreeTargets(usize),

    #[error("Nacht-Pirschjäger generiert Ressourcen ausschließlich über Auto-Attacks")]
    StalkerResourceRequiresAutoAttack,

    #[error("Boss-Ulti 'Doppelagent' proct ausschließlich bei Bossen mit <= 50% HP")]
    BossUltiRequiresBossBelowHalfHp,

    #[error("Ziel ist in einer unangreifbaren Phase (z. B. Sariels Himmelsphase) — nutze Notfall-Eigenblut-Skills")]
    BossCurrentlyInvulnerable,

    #[error("Quest nicht gefunden: {0}")]
    QuestNotFound(String),

    #[error("Daten-Konfigurationsfehler: {0}")]
    DataConfigError(String),
}
