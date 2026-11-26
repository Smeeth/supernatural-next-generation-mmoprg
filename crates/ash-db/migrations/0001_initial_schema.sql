-- SPDX-License-Identifier: GPL-3.0-or-later
-- Ash-Net: New Lawrence — Initial PostgreSQL 16 Schema

CREATE TABLE IF NOT EXISTS accounts (
    id UUID PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS characters (
    id UUID PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT NOT NULL UNIQUE,
    race TEXT NOT NULL CHECK (race NOT IN ('angel', 'engel', 'erzengel', 'demon', 'daemon', 'dämon')),
    class TEXT NOT NULL,
    current_zone TEXT NOT NULL DEFAULT 'zone_roadhouse',
    pos_x REAL NOT NULL DEFAULT 0.0,
    pos_y REAL NOT NULL DEFAULT 0.0,
    hp INTEGER NOT NULL,
    max_hp INTEGER NOT NULL,
    salt_ward_active BOOLEAN NOT NULL DEFAULT FALSE,
    pouch_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_characters_account_id ON characters(account_id);
CREATE INDEX IF NOT EXISTS idx_characters_name_lower ON characters(LOWER(name));

CREATE TABLE IF NOT EXISTS character_equipment (
    character_id UUID NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    slot SMALLINT NOT NULL CHECK (slot BETWEEN 1 AND 16),
    item_code TEXT NOT NULL,
    display_name TEXT NOT NULL,
    quality SMALLINT NOT NULL CHECK (quality BETWEEN 0 AND 4),
    material TEXT NOT NULL,
    weapon_archetype TEXT,
    power_score INTEGER NOT NULL,
    capacity INTEGER NOT NULL DEFAULT 0,
    access_time_ms INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (character_id, slot)
);

CREATE TABLE IF NOT EXISTS character_essences (
    character_id UUID NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    essence_type TEXT NOT NULL,
    amount INTEGER NOT NULL CHECK (amount >= 0),
    PRIMARY KEY (character_id, essence_type)
);

CREATE TABLE IF NOT EXISTS character_quests (
    character_id UUID NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    quest_id TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    current_step INTEGER NOT NULL DEFAULT 0,
    total_steps INTEGER NOT NULL DEFAULT 3,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (character_id, quest_id)
);

CREATE TABLE IF NOT EXISTS zone_omen_states (
    zone_id TEXT PRIMARY KEY,
    omen_index SMALLINT NOT NULL CHECK (omen_index BETWEEN 0 AND 3),
    bounty_title TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
