# STATUS.md — Realer Projekt- & Architektur-Stand

- **Aktuelle Phase:** **Sprint 1.7 abgeschlossen – KI-Harness aktiv, bereit für Sprint 2 (`ACTIVE_SCOPE = SERVER + SHARED`)**
- **Harness- & Gate-Status:**
  - `./scripts/harness.sh gate`: **GRÜN (Exit-Code 0)** — 21 `.rs`-Dateien SPDX-konform, 3-Zonen-Graph sauber, 29 Governance-/Harness-Dateien geprüft, `fmt`/`clippy -D warnings` sauber, 15/15 Workspace-Tests grün.
  - `./scripts/harness.sh sim`: **GRÜN (Exit-Code 0)** — `01-sim-invariants.toml` (18 Steps + `NDJSON`-Trace) bestanden; `02-combat-4role.template.toml` (`min_sprint = 2`) bei `CURRENT_SPRINT=1` sauber übersprungen.
  - `./scripts/harness.sh e2e`: Konfiguriert über `docker-compose.harness.yml` (`tmpfs` für Postgres/Redis, Projekt `ash-harness`) + `00-smoke-mesh.toml` (Schema per Unit-Test validiert; in der lokalen WSL-Umgebung ist keine `docker`-CLI / kein Docker-Daemon installiert, In-Process-QUIC/IRC-E2E-Äquivalent läuft über `crates/ash-game-node/tests/cluster_e2e.rs` grün).
- **Nächster freigegebener Zonen-Scope:** `SERVER + SHARED` gemäß [`.agents/tasks/sprint-2-breakdown.md`](tasks/sprint-2-breakdown.md) *(`ash-client-tui` bleibt bis Sprint 5 strikt gesperrt!)*

---

## 1. Reales Cargo-Workspace-Inventar (7 Crates — kein Crate-Wildwuchs)

| Crate | Pfad | `[package.metadata.ash] zone` | Status & Tests | 3-Zonen-Graph-Status |
| :--- | :--- | :---: | :--- | :--- |
| `ash-core` | `crates/ash-core` | `shared` | Domain, Kampf & deterministischer `scenario`-Simulator (9/9 Unit-Tests grün) | **GRÜN** (null I/O, deterministischer Seed) |
| `ash-protocol` | `crates/ash-protocol` | `shared` | Reine Wire-Frames (`client`, `mesh`, `codec`; 1/1 Roundtrip-Test grün) | **GRÜN** |
| `ash-db` | `crates/ash-db` | `server` | `sqlx` + `redis` + Server-mTLS/QUIC `transport` | **GRÜN** |
| `ash-auth-gateway` | `crates/ash-auth-gateway` | `server` | Auth, Matrix, PKI-Init, Crash-Recovery-Fallback | **GRÜN** |
| `ash-game-node` | `crates/ash-game-node` | `server` | 25-Hz-`hecs`, mTLS-Gossip, Handover (2/2 Cluster-E2E-Tests grün) | **GRÜN** |
| `ash-irc-bridge` | `crates/ash-irc-bridge` | `server` | Slash-Parser & `#new_lawrence`-Funk-Alarme | **GRÜN** |
| `ash-test-cli` | `crates/ash-test-cli` | `client` | Blackbox-QUIC/IRC-Client + `run-scenario --trace --report` (3/3 Unit-Tests grün) | **GRÜN** |

---

## 2. Verankerte KI-Harness-Komponenten (Sprint 1.7)

- **Zentraler Runner:** [`scripts/harness.sh`](../scripts/harness.sh) (`gate | sim | e2e | all`) mit `trap`-Teardown und kompaktem Fehler-Digest in `.harness-artifacts/last-failure.md`.
- **Ephemerer Live-Stack:** [`docker-compose.harness.yml`](../docker-compose.harness.yml) (`name: ash-harness`, `tmpfs` für PostgreSQL 16 & Redis 7, schnelle Healthchecks).
- **Deklarative Szenarien (`harness/scenarios/`):**
  - [`00-smoke-mesh.toml`](../harness/scenarios/00-smoke-mesh.toml) (`mode = "e2e"`, `min_sprint = 1` — aktiv)
  - [`01-sim-invariants.toml`](../harness/scenarios/01-sim-invariants.toml) (`mode = "sim"`, `min_sprint = 1` — aktiv, 18 Steps)
  - [`02-combat-4role.template.toml`](../harness/scenarios/02-combat-4role.template.toml) (`min_sprint = 2`)
  - [`03-roadhouse-quests.template.toml`](../harness/scenarios/03-roadhouse-quests.template.toml) (`min_sprint = 3`)
  - [`04-raid-failover.template.toml`](../harness/scenarios/04-raid-failover.template.toml) (`min_sprint = 4`)
- **Atomare Sprint-Orchestrierung:** [`.agents/tasks/README.md`](tasks/README.md) & [`.agents/tasks/sprint-2-breakdown.md`](tasks/sprint-2-breakdown.md) (Pakete `2A` bis `2G`) sowie [`ADR-005`](decisions/ADR-005-ai-verification-harness.md).

---

## 3. Pflicht-Update-Checkliste für kommende Sprints

- [x] **Sprint 1.5 – 1.7 (Governance, 3-Zonen-Gate & KI-Harness):** Vollständig aktiv & grün.
- [ ] **Ende Sprint 2 (`SERVER + SHARED`):** Pakete `2A`–`2G` in `sprint-2-breakdown.md` abarbeiten, `02-combat-4role.toml` aktivieren, `CURRENT_SPRINT=2 ./scripts/harness.sh gate && CURRENT_SPRINT=2 ./scripts/harness.sh sim` grün.
- [ ] **Ende Sprint 3 (`SERVER + SHARED`):** Weltdaten `/data/world/*.toml`, Quest-/Omen-/Gathering-Ausbau & `03-roadhouse-quests.toml` grün.
- [ ] **Ende Sprint 4 (`SERVER + SHARED`):** Raid-Encounter (*Malphas*, *Sariel*), Failover-Szenario `04-raid-failover.toml` & ggf. `ash-admin-cli` (`zone = "server"`) grün.
- [ ] **Ende Sprint 5 (`CLIENT`):** `ash-client-tui` (`zone = "client"`, `ratatui`/`crossterm`) + `tui-snap`-Harness (`TestBackend`) grün.
