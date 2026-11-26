// SPDX-License-Identifier: GPL-3.0-or-later

pub mod transport;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use ash_core::character::{CharacterClass, CharacterSheet, PlayableRace, STARTING_ZONE_ROADHOUSE};
use ash_core::equipment::{
    EquipmentItem, EquipmentLoadout, EquipmentSlot, EssenceType, EssenceWallet, ItemQuality,
    MaterialTag, PouchConsumable, WeaponArchetype,
};
use ash_protocol::messages::{
    PlayerHandoverState, QuestStateDto, ZoneRouteInfo, ZoneRoutingTicket,
};
use redis::AsyncCommands;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tokio::sync::RwLock;
use uuid::Uuid;

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

#[derive(Debug, Clone)]
pub struct AccountRecord {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
}

#[derive(Default)]
pub struct InMemoryData {
    pub accounts_by_user: HashMap<String, AccountRecord>,
    pub sessions: HashMap<String, Uuid>,
    pub characters: HashMap<Uuid, CharacterSheet>,
    pub quests: HashMap<(Uuid, String), QuestStateDto>,
    pub tickets: HashMap<String, ZoneRoutingTicket>,
    pub handovers: HashMap<String, PlayerHandoverState>,
    pub routes: HashMap<String, ZoneRouteInfo>,
    pub zone_omens: HashMap<String, u8>,
}

#[derive(Clone)]
pub enum AshStore {
    PostgresRedis { pg: PgPool, redis: redis::Client },
    InMemory(Arc<RwLock<InMemoryData>>),
}

impl AshStore {
    /// Verbindet sich mit PostgreSQL 16 + Redis 7 und führt automatisch alle SQLx-Migrationen aus.
    pub async fn connect_postgres_and_redis(database_url: &str, redis_url: &str) -> Result<Self> {
        let pg = PgPoolOptions::new()
            .max_connections(16)
            .acquire_timeout(Duration::from_secs(10))
            .connect(database_url)
            .await
            .context("connect to PostgreSQL")?;

        sqlx::migrate!("./migrations")
            .run(&pg)
            .await
            .context("run SQLx database migrations")?;

        let redis = redis::Client::open(redis_url).context("open Redis client")?;
        let mut conn = redis
            .get_multiplexed_async_connection()
            .await
            .context("verify Redis connection")?;
        let _: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .context("PING Redis")?;

        Ok(Self::PostgresRedis { pg, redis })
    }

    pub fn new_shared_in_memory() -> Self {
        Self::InMemory(Arc::new(RwLock::new(InMemoryData::default())))
    }

    // ------------------------------------------------------------------------
    // Accounts & Sessions
    // ------------------------------------------------------------------------

    pub async fn create_account(&self, username: &str, password_hash: &str) -> Result<Uuid> {
        let id = Uuid::new_v4();
        match self {
            Self::PostgresRedis { pg, .. } => {
                sqlx::query(
                    "INSERT INTO accounts (id, username, password_hash) VALUES ($1, $2, $3)",
                )
                .bind(id)
                .bind(username)
                .bind(password_hash)
                .execute(pg)
                .await
                .context("insert account")?;
                Ok(id)
            }
            Self::InMemory(mem) => {
                let mut g = mem.write().await;
                if g.accounts_by_user.contains_key(username) {
                    bail!("Username '{username}' existiert bereits");
                }
                g.accounts_by_user.insert(
                    username.to_string(),
                    AccountRecord {
                        id,
                        username: username.to_string(),
                        password_hash: password_hash.to_string(),
                    },
                );
                Ok(id)
            }
        }
    }

    pub async fn find_account_by_username(&self, username: &str) -> Result<Option<AccountRecord>> {
        match self {
            Self::PostgresRedis { pg, .. } => {
                let row = sqlx::query(
                    "SELECT id, username, password_hash FROM accounts WHERE username = $1",
                )
                .bind(username)
                .fetch_optional(pg)
                .await?;
                Ok(row.map(|r| AccountRecord {
                    id: r.get("id"),
                    username: r.get("username"),
                    password_hash: r.get("password_hash"),
                }))
            }
            Self::InMemory(mem) => {
                let g = mem.read().await;
                Ok(g.accounts_by_user.get(username).cloned())
            }
        }
    }

    pub async fn create_session(&self, account_id: Uuid) -> Result<String> {
        let token = Uuid::new_v4().to_string();
        match self {
            Self::PostgresRedis { redis, .. } => {
                let mut conn = redis.get_multiplexed_async_connection().await?;
                let key = format!("ash:session:{token}");
                conn.set_ex::<_, _, ()>(key, account_id.to_string(), 3600)
                    .await?;
            }
            Self::InMemory(mem) => {
                mem.write().await.sessions.insert(token.clone(), account_id);
            }
        }
        Ok(token)
    }

    pub async fn resolve_session(&self, token: &str) -> Result<Option<Uuid>> {
        match self {
            Self::PostgresRedis { redis, .. } => {
                let mut conn = redis.get_multiplexed_async_connection().await?;
                let key = format!("ash:session:{token}");
                let val: Option<String> = conn.get(key).await?;
                Ok(val.and_then(|s| Uuid::parse_str(&s).ok()))
            }
            Self::InMemory(mem) => Ok(mem.read().await.sessions.get(token).copied()),
        }
    }

    // ------------------------------------------------------------------------
    // Characters, 16-Slot Equipment & Occult Essences
    // ------------------------------------------------------------------------

    pub async fn save_character(&self, ch: &CharacterSheet) -> Result<()> {
        match self {
            Self::PostgresRedis { pg, .. } => {
                let mut tx = pg.begin().await?;
                let pouch_json = serde_json::to_value(&ch.equipment.pouch_contents)?;
                sqlx::query(
                    r#"
                    INSERT INTO characters (
                        id, account_id, name, race, class, current_zone,
                        pos_x, pos_y, hp, max_hp, salt_ward_active, pouch_json, updated_at
                    ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,NOW())
                    ON CONFLICT (id) DO UPDATE SET
                        current_zone = EXCLUDED.current_zone,
                        pos_x = EXCLUDED.pos_x,
                        pos_y = EXCLUDED.pos_y,
                        hp = EXCLUDED.hp,
                        max_hp = EXCLUDED.max_hp,
                        salt_ward_active = EXCLUDED.salt_ward_active,
                        pouch_json = EXCLUDED.pouch_json,
                        updated_at = NOW()
                    "#,
                )
                .bind(ch.id)
                .bind(ch.account_id)
                .bind(&ch.name)
                .bind(ch.race.as_slug())
                .bind(ch.class.as_slug())
                .bind(&ch.current_zone)
                .bind(ch.pos_x)
                .bind(ch.pos_y)
                .bind(ch.hp)
                .bind(ch.max_hp)
                .bind(ch.salt_ward_active)
                .bind(pouch_json)
                .execute(&mut *tx)
                .await?;

                for item in ch.equipment.slots.values() {
                    let mat_json = serde_json::to_string(&item.material)?;
                    let arch_json = match item.weapon_archetype {
                        Some(a) => Some(serde_json::to_string(&a)?),
                        None => None,
                    };
                    sqlx::query(
                        r#"
                        INSERT INTO character_equipment (
                            character_id, slot, item_code, display_name, quality,
                            material, weapon_archetype, power_score, capacity, access_time_ms
                        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
                        ON CONFLICT (character_id, slot) DO UPDATE SET
                            item_code = EXCLUDED.item_code,
                            display_name = EXCLUDED.display_name,
                            quality = EXCLUDED.quality,
                            material = EXCLUDED.material,
                            weapon_archetype = EXCLUDED.weapon_archetype,
                            power_score = EXCLUDED.power_score,
                            capacity = EXCLUDED.capacity,
                            access_time_ms = EXCLUDED.access_time_ms
                        "#,
                    )
                    .bind(ch.id)
                    .bind(item.slot.index() as i16)
                    .bind(&item.item_code)
                    .bind(&item.display_name)
                    .bind(item.quality as i16)
                    .bind(mat_json)
                    .bind(arch_json)
                    .bind(item.power_score as i32)
                    .bind(item.capacity as i32)
                    .bind(item.access_time_ms as i32)
                    .execute(&mut *tx)
                    .await?;
                }

                for (essence, amount) in &ch.essences.counts {
                    let ess_str = serde_json::to_string(essence)?;
                    sqlx::query(
                        r#"
                        INSERT INTO character_essences (character_id, essence_type, amount)
                        VALUES ($1, $2, $3)
                        ON CONFLICT (character_id, essence_type) DO UPDATE SET amount = EXCLUDED.amount
                        "#,
                    )
                    .bind(ch.id)
                    .bind(ess_str)
                    .bind(*amount as i32)
                    .execute(&mut *tx)
                    .await?;
                }

                tx.commit().await?;
                Ok(())
            }
            Self::InMemory(mem) => {
                mem.write().await.characters.insert(ch.id, ch.clone());
                Ok(())
            }
        }
    }

    pub async fn load_character(&self, character_id: Uuid) -> Result<Option<CharacterSheet>> {
        match self {
            Self::PostgresRedis { pg, .. } => {
                let Some(row) = sqlx::query("SELECT * FROM characters WHERE id = $1")
                    .bind(character_id)
                    .fetch_optional(pg)
                    .await?
                else {
                    return Ok(None);
                };
                Ok(Some(self.hydrate_character_from_row(pg, row).await?))
            }
            Self::InMemory(mem) => Ok(mem.read().await.characters.get(&character_id).cloned()),
        }
    }

    pub async fn load_character_by_name(&self, name: &str) -> Result<Option<CharacterSheet>> {
        match self {
            Self::PostgresRedis { pg, .. } => {
                let Some(row) =
                    sqlx::query("SELECT * FROM characters WHERE LOWER(name) = LOWER($1)")
                        .bind(name)
                        .fetch_optional(pg)
                        .await?
                else {
                    return Ok(None);
                };
                Ok(Some(self.hydrate_character_from_row(pg, row).await?))
            }
            Self::InMemory(mem) => {
                let g = mem.read().await;
                Ok(g.characters
                    .values()
                    .find(|c| c.name.eq_ignore_ascii_case(name))
                    .cloned())
            }
        }
    }

    pub async fn list_characters_for_account(
        &self,
        account_id: Uuid,
    ) -> Result<Vec<CharacterSheet>> {
        match self {
            Self::PostgresRedis { pg, .. } => {
                let rows = sqlx::query(
                    "SELECT * FROM characters WHERE account_id = $1 ORDER BY created_at ASC",
                )
                .bind(account_id)
                .fetch_all(pg)
                .await?;
                let mut out = Vec::with_capacity(rows.len());
                for r in rows {
                    out.push(self.hydrate_character_from_row(pg, r).await?);
                }
                Ok(out)
            }
            Self::InMemory(mem) => {
                let g = mem.read().await;
                Ok(g.characters
                    .values()
                    .filter(|c| c.account_id == account_id)
                    .cloned()
                    .collect())
            }
        }
    }

    async fn hydrate_character_from_row(
        &self,
        pg: &PgPool,
        row: sqlx::postgres::PgRow,
    ) -> Result<CharacterSheet> {
        let id: Uuid = row.get("id");
        let race_str: String = row.get("race");
        let class_str: String = row.get("class");
        let race = PlayableRace::parse_strict(&race_str)?;
        let class = CharacterClass::parse_strict(&class_str)?;
        let pouch_val: serde_json::Value = row.get("pouch_json");
        let pouch_contents: Vec<PouchConsumable> =
            serde_json::from_value(pouch_val).unwrap_or_default();

        let eq_rows = sqlx::query(
            "SELECT * FROM character_equipment WHERE character_id = $1 ORDER BY slot ASC",
        )
        .bind(id)
        .fetch_all(pg)
        .await?;

        let mut slots = std::collections::BTreeMap::new();
        for er in eq_rows {
            let slot_idx: i16 = er.get("slot");
            let slot = EquipmentSlot::from_index(slot_idx as u8).context("valid slot index")?;
            let q_idx: i16 = er.get("quality");
            let quality = ItemQuality::from_tier(q_idx as u8).unwrap_or(ItemQuality::Grey);
            let mat_str: String = er.get("material");
            let material: MaterialTag = serde_json::from_str(&mat_str)?;
            let arch_opt: Option<String> = er.get("weapon_archetype");
            let weapon_archetype: Option<WeaponArchetype> = match arch_opt {
                Some(s) => Some(serde_json::from_str(&s)?),
                None => None,
            };
            let power_score: i32 = er.get("power_score");
            let capacity: i32 = er.get("capacity");
            let access_time_ms: i32 = er.get("access_time_ms");
            slots.insert(
                slot,
                EquipmentItem {
                    slot,
                    item_code: er.get("item_code"),
                    display_name: er.get("display_name"),
                    quality,
                    material,
                    weapon_archetype,
                    power_score: power_score as u32,
                    capacity: capacity as u16,
                    access_time_ms: access_time_ms as u32,
                },
            );
        }

        let ess_rows = sqlx::query(
            "SELECT essence_type, amount FROM character_essences WHERE character_id = $1",
        )
        .bind(id)
        .fetch_all(pg)
        .await?;
        let mut essences = EssenceWallet::default();
        for er in ess_rows {
            let et_str: String = er.get("essence_type");
            let amt: i32 = er.get("amount");
            if let Ok(et) = serde_json::from_str::<EssenceType>(&et_str) {
                essences.counts.insert(et, amt.max(0) as u32);
            }
        }

        Ok(CharacterSheet {
            id,
            account_id: row.get("account_id"),
            name: row.get("name"),
            race,
            class,
            current_zone: row.get("current_zone"),
            pos_x: row.get("pos_x"),
            pos_y: row.get("pos_y"),
            hp: row.get("hp"),
            max_hp: row.get("max_hp"),
            equipment: EquipmentLoadout {
                slots,
                pouch_contents,
            },
            essences,
            salt_ward_active: row.get("salt_ward_active"),
        })
    }

    // ------------------------------------------------------------------------
    // Quests
    // ------------------------------------------------------------------------

    pub async fn upsert_quest(&self, character_id: Uuid, quest: &QuestStateDto) -> Result<()> {
        match self {
            Self::PostgresRedis { pg, .. } => {
                sqlx::query(
                    r#"
                    INSERT INTO character_quests (character_id, quest_id, title, status, current_step, total_steps, updated_at)
                    VALUES ($1,$2,$3,$4,$5,$6,NOW())
                    ON CONFLICT (character_id, quest_id) DO UPDATE SET
                        status = EXCLUDED.status,
                        current_step = EXCLUDED.current_step,
                        updated_at = NOW()
                    "#,
                )
                .bind(character_id)
                .bind(&quest.quest_id)
                .bind(&quest.title)
                .bind(&quest.status)
                .bind(quest.current_step as i32)
                .bind(quest.total_steps as i32)
                .execute(pg)
                .await?;
                Ok(())
            }
            Self::InMemory(mem) => {
                mem.write()
                    .await
                    .quests
                    .insert((character_id, quest.quest_id.clone()), quest.clone());
                Ok(())
            }
        }
    }

    pub async fn list_quests(&self, character_id: Uuid) -> Result<Vec<QuestStateDto>> {
        match self {
            Self::PostgresRedis { pg, .. } => {
                let rows = sqlx::query(
                    "SELECT quest_id, title, status, current_step, total_steps FROM character_quests WHERE character_id = $1 ORDER BY quest_id ASC",
                )
                .bind(character_id)
                .fetch_all(pg)
                .await?;
                Ok(rows
                    .into_iter()
                    .map(|r| QuestStateDto {
                        quest_id: r.get("quest_id"),
                        title: r.get("title"),
                        status: r.get("status"),
                        current_step: r.get::<i32, _>("current_step") as u32,
                        total_steps: r.get::<i32, _>("total_steps") as u32,
                    })
                    .collect())
            }
            Self::InMemory(mem) => {
                let g = mem.read().await;
                Ok(g.quests
                    .iter()
                    .filter(|((cid, _), _)| *cid == character_id)
                    .map(|(_, q)| q.clone())
                    .collect())
            }
        }
    }

    // ------------------------------------------------------------------------
    // Redis Login Tickets, Handover Tokens & Cluster Routing Table
    // ------------------------------------------------------------------------

    pub async fn store_zone_ticket(&self, ticket: &ZoneRoutingTicket) -> Result<()> {
        match self {
            Self::PostgresRedis { redis, .. } => {
                let mut conn = redis.get_multiplexed_async_connection().await?;
                let key = format!("ash:ticket:{}", ticket.ticket_token);
                let json = serde_json::to_string(ticket)?;
                conn.set_ex::<_, _, ()>(key, json, 60).await?;
            }
            Self::InMemory(mem) => {
                mem.write()
                    .await
                    .tickets
                    .insert(ticket.ticket_token.clone(), ticket.clone());
            }
        }
        Ok(())
    }

    pub async fn consume_zone_ticket(&self, token: &str) -> Result<Option<ZoneRoutingTicket>> {
        match self {
            Self::PostgresRedis { redis, .. } => {
                let mut conn = redis.get_multiplexed_async_connection().await?;
                let key = format!("ash:ticket:{token}");
                let val: Option<String> = redis::cmd("GETDEL")
                    .arg(&key)
                    .query_async(&mut conn)
                    .await?;
                Ok(val.and_then(|s| serde_json::from_str(&s).ok()))
            }
            Self::InMemory(mem) => Ok(mem.write().await.tickets.remove(token)),
        }
    }

    pub async fn store_handover_token(
        &self,
        token: &str,
        state: &PlayerHandoverState,
    ) -> Result<()> {
        match self {
            Self::PostgresRedis { redis, .. } => {
                let mut conn = redis.get_multiplexed_async_connection().await?;
                let key = format!("ash:handover:{token}");
                let json = serde_json::to_string(state)?;
                conn.set_ex::<_, _, ()>(key, json, 30).await?;
            }
            Self::InMemory(mem) => {
                mem.write()
                    .await
                    .handovers
                    .insert(token.to_string(), state.clone());
            }
        }
        Ok(())
    }

    pub async fn consume_handover_token(&self, token: &str) -> Result<Option<PlayerHandoverState>> {
        match self {
            Self::PostgresRedis { redis, .. } => {
                let mut conn = redis.get_multiplexed_async_connection().await?;
                let key = format!("ash:handover:{token}");
                let val: Option<String> = redis::cmd("GETDEL")
                    .arg(&key)
                    .query_async(&mut conn)
                    .await?;
                Ok(val.and_then(|s| serde_json::from_str(&s).ok()))
            }
            Self::InMemory(mem) => Ok(mem.write().await.handovers.remove(token)),
        }
    }

    pub async fn publish_zone_route(&self, route: &ZoneRouteInfo) -> Result<()> {
        match self {
            Self::PostgresRedis { pg, redis } => {
                let mut conn = redis.get_multiplexed_async_connection().await?;
                let json = serde_json::to_string(route)?;
                conn.hset::<_, _, _, ()>("ash:mesh:routes", &route.zone_id, json)
                    .await?;
                sqlx::query(
                    r#"
                    INSERT INTO zone_omen_states (zone_id, omen_index, updated_at)
                    VALUES ($1, $2, NOW())
                    ON CONFLICT (zone_id) DO UPDATE SET omen_index = EXCLUDED.omen_index, updated_at = NOW()
                    "#,
                )
                .bind(&route.zone_id)
                .bind(route.omen_level as i16)
                .execute(pg)
                .await?;
            }
            Self::InMemory(mem) => {
                let mut g = mem.write().await;
                g.zone_omens.insert(route.zone_id.clone(), route.omen_level);
                g.routes.insert(route.zone_id.clone(), route.clone());
            }
        }
        Ok(())
    }

    pub async fn get_active_zone_route(
        &self,
        zone_id: &str,
        max_stale_ms: u64,
    ) -> Result<Option<ZoneRouteInfo>> {
        let now = now_epoch_ms();
        let route_opt = match self {
            Self::PostgresRedis { redis, .. } => {
                let mut conn = redis.get_multiplexed_async_connection().await?;
                let raw: Option<String> = conn.hget("ash:mesh:routes", zone_id).await?;
                raw.and_then(|s| serde_json::from_str::<ZoneRouteInfo>(&s).ok())
            }
            Self::InMemory(mem) => mem.read().await.routes.get(zone_id).cloned(),
        };

        Ok(route_opt.filter(|r| now.saturating_sub(r.last_heartbeat_ms) <= max_stale_ms))
    }

    /// Crash-Recovery-Helfer: Setzt einen Charakter sicher nach `zone_roadhouse` zurück,
    /// falls sein bisheriger Zonen- oder Raid-Knoten ausgefallen ist.
    pub async fn fallback_character_to_roadhouse(&self, ch: &mut CharacterSheet) -> Result<()> {
        ch.current_zone = STARTING_ZONE_ROADHOUSE.to_string();
        ch.pos_x = 0.0;
        ch.pos_y = 0.0;
        self.save_character(ch).await
    }
}
