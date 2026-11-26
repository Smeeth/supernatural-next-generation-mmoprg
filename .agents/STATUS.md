# STATUS.md — Realer Projekt- & Architektur-Stand

- **Aktuelle Phase:** Sprint 1, Sprint 1.5 & **Sprint 1.6 (Gate- & 3-Zonen-Cleanup) abgeschlossen**
- **Gate-Status (`./scripts/check-invariants.sh`):** **GRÜN (Exit-Code 0)** — Bereit für Sprint 2 (`ACTIVE_SCOPE = SERVER + SHARED`)
- **Nächster freigegebener Zonen-Scope:** `SERVER + SHARED` *(`ash-client-tui` bleibt bis Sprint 5 strikt gesperrt!)*

---

## 1. Reales Cargo-Workspace-Inventar (7 Crates — 100 % 3-Zonen-konform)

| Crate | Pfad | `[package.metadata.ash] zone` | Status & Tests | 3-Zonen-Graph-Status |
| :--- | :--- | :---: | :--- | :--- |
| `ash-core` | `crates/ash-core` | `shared` | Produktiv (9/9 Unit-Tests grün) | **GRÜN** (null I/O, deterministisch) |
| `ash-protocol` | `crates/ash-protocol` | `shared` | Produktiv (`client`, `mesh`, `codec`; 1/1 Roundtrip-Test grün) | **GRÜN** (frei von `tokio`/`quinn`) |
| `ash-db` | `crates/ash-db` | `server` | Produktiv (`sqlx`, `redis` & Server-mTLS/QUIC `transport`) | **GRÜN** |
| `ash-auth-gateway` | `crates/ash-auth-gateway` | `server` | Produktiv (Auth, Matrix, PKI-Init, Crash-Fallback) | **GRÜN** |
| `ash-game-node` | `crates/ash-game-node` | `server` | Produktiv (25-Hz-`hecs`, mTLS-Gossip, Handover; 2/2 Cluster-E2E-Tests grün) | **GRÜN** |
| `ash-irc-bridge` | `crates/ash-irc-bridge` | `server` | Produktiv (Slash-Parser & `#new_lawrence`-Funk-Alarme) | **GRÜN** |
| `ash-test-cli` | `crates/ash-test-cli` | `client` | Reiner Blackbox-QUIC/IRC-Smoke-Client (1/1 Unit-Test grün) | **GRÜN** (null Server-/DB-/ECS-Kanten) |

---

## 2. Reale Infrastruktur & Datenbank-Tabellen

- **Docker-Compose-Services (`docker-compose.yml`):**
  - Standard: `pki-init`, `postgres` (PostgreSQL 16), `redis` (Redis 7), `ircd` (`ngIRCd` mit `#roadhouse`, `#new_lawrence`, `#zone_salzminen`, `#men_of_letters`), `auth-gateway`, `game-node-roadhouse` (`hub`), `game-node-frontier` (`mirror`), `irc-bridge`.
  - Profile: `game-node-raid` (`profiles: ["raids"]`), `ash-test-cli` (`profiles: ["test"]`).
  - Netzwerke: `ash-mesh-net`, `data-net` (`internal: true`), `irc-net`.
- **PostgreSQL-Schema (`crates/ash-db/migrations/0001_initial_schema.sql`):**
  - `accounts`, `characters` (inkl. DB-`CHECK` gegen Engel/Dämonen), `character_equipment` (Slots 1–16), `character_essences`, `character_quests`, `zone_omen_states`.
- **Datengetriebene Content-Dateien:**
  - `data/zones.toml` (6 Zonen inkl. Roadhouse-NPCs Ellen, Jo, Ashs Terminal)
  - `data/quests.toml` (3 Einstiegs-Quests)

---

## 3. Behobene Schulden aus Sprint 1.6 (Abgeschlossen)

- [x] **3-Zonen-Entkopplung `ash-protocol` (`shared`):** `tokio` und `quinn` vollständig entfernt; Nachrichten sauber in `client`-, `mesh`- und synchrones `codec`-Modul getrennt; Server-mTLS/QUIC-Transport nach `ash-db::transport` (`zone = "server"`) verschoben.
- [x] **3-Zonen-Isolation `ash-test-cli` (`client`):** Sämtliche `server`-, `sqlx`-, `redis`- und `hecs`-Abhängigkeiten entfernt; den vollständigen 8-Phasen-In-Process-Cluster-E2E-Test + Crash-Recovery-Test nach `crates/ash-game-node/tests/cluster_e2e.rs` (`zone = "server"`) überführt.
- [x] **Formatierung & Linter:** `cargo fmt --all -- --check` und `cargo clippy --workspace --all-targets -- -D warnings` (inkl. Fix in `ash-core/src/world.rs`) laufen mit **0 Warnungen / 0 Diffs** durch.
- **Offene Schulden:** *Keine.*

---

## 4. Pflicht-Update-Checkliste für kommende Sprints

- [x] **Vor Sprint 2 (Sprint 1.6 Gate-Cleanup):** `./scripts/check-invariants.sh` vollständig auf Grün (`Exit 0`) gebracht.
- [ ] **Ende Sprint 2 (`SERVER + SHARED`):** Neue Server-/Mesh-/Kampf-Erweiterungen + additive DB-Migrationen dokumentieren, `./scripts/check-invariants.sh` grün.
- [ ] **Ende Sprint 3 (`SERVER + SHARED`):** Migration/Ausbau der Weltdaten nach `/data/world/*.toml`, Quest-/Omen-/Gathering-Stand eintragen, `./scripts/check-invariants.sh` grün.
- [ ] **Ende Sprint 4 (`SERVER + SHARED`):** Vollständige Raid-Encounter (*Malphas*, *Sariel*) & ggf. `ash-admin-cli` (`zone = "server"`) dokumentieren, `./scripts/check-invariants.sh` grün.
- [ ] **Ende Sprint 5 (`CLIENT`):** Freigabe und Dokumentation von `ash-client-tui` (`zone = "client"`, `ratatui`/`crossterm`) strikt über `ash-protocol::client` + `ircd`, `./scripts/check-invariants.sh` grün.
