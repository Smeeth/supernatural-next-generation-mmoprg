# AGENTS.md — Operative KI-Governance (Single Source of Truth Entry)

## 1. Projekt-Steckbrief
- **Projekt:** *Ash-Net: New Lawrence* — Server-First Terminal-MMORPG (*Die Neue Front*, Andromeda-System, 30+ Jahre nach *Supernatural*).
- **Tech-Stack:** Rust Cargo-Workspace, QUIC-Server-Mesh (`quinn`, TLS 1.3 / Server-mTLS), Headless ECS (`hecs`, 20–30 Hz), PostgreSQL 16 (`sqlx`), Redis 7, separater IRCd (`ngIRCd`) + Docker Compose.
- **Lizenz (zwingend):** `GPL-3.0-or-later` (`SPDX-License-Identifier: GPL-3.0-or-later` in jeder `.rs`-Datei). Nur GPLv3-kompatible Crates.

## 2. Das 3-Zonen-Monorepo-Prinzip (Strikte Systemtrennung)
Obwohl `SHARED`, `SERVER` und `CLIENT` im selben Cargo-Workspace liegen, werden sie als **strikt getrennte Systeme** behandelt. Jede Crate deklariert ihre Zone verbindlich via `[package.metadata.ash] zone = "shared" | "server" | "client"`:

| Zone | Aktuelle & Geplante Crates | Erlaubte Kanten | Strikt verboten |
| :--- | :--- | :--- | :--- |
| **`shared`** | `ash-core`, `ash-protocol` | Reine Domain-/Serialisierungs-Crates (`serde`, `bincode`, `uuid`, `thiserror`) | `SERVER`-/`CLIENT`-Crates, `tokio`, `quinn`, `sqlx`, `redis`, `ratatui`, `crossterm` |
| **`server`** | `ash-db`, `ash-auth-gateway`, `ash-game-node`, `ash-irc-bridge` *(+ künftig `ash-admin-cli`)* | `shared` + Server-Infra (`tokio`, `quinn`, `sqlx`, `redis`, `hecs`) | `CLIENT`-Crates, `ratatui`, `crossterm`, jegliche TUI-/GUI-Crates |
| **`client`** | `ash-test-cli` *(Sprint 1–4)*, `ash-client-tui` *(reserviert ab Sprint 5)* | `shared` (`ash-protocol`, Lese-/Validierungs-Typen aus `ash-core`), `tokio`, `quinn`, IRC-Client, ab Sprint 5 `ratatui`/`crossterm` | `SERVER`-Crates, `sqlx`, `redis`, `hecs`, `bevy_ecs`, autoritative Spiellogik |

**Einzige erlaubte Brücken zwischen Client und Server:**
1. **`ash-protocol`** (typisierte binäre Wire-Frames über QUIC)
2. **Separater `ircd`** (soziale Text-Kommunikation & In-Game-Slash-Emotes)

## 3. Pflicht-Lesereihenfolge VOR jeder Code-Änderung
Jeder Agent (unabhängig von Modell oder IDE) MUSS vor der ersten Dateiänderung in dieser Reihenfolge lesen:
1. [`.agents/STATUS.md`](.agents/STATUS.md) — Aktueller Sprint-Stand, aktiver Zonen-Scope (`SERVER` vs. `CLIENT`), reale Crates & offene Befunde.
2. [`.agents/00-invariants.md`](.agents/00-invariants.md) — Unverhandelbare Lizenz-, Protokoll-, Architektur- und Balancing-Gesetze.
3. [`.agents/03-scope-guardrails.md`](.agents/03-scope-guardrails.md) — Sprint-Lock, Anti-Scope-Creep, Surgical-Diff- & Eskalationsregeln.
4. [`.agents/05-client-server-boundary.md`](.agents/05-client-server-boundary.md) — 3-Säulen-Trennwand (Abhängigkeitsgraph, Server-Authority, maschinelle Prüfung).
5. Fachdokumente nach Aufgabenbedarf:
   - [`.agents/01-architecture.md`](.agents/01-architecture.md) (Workspace, Determinismus-Regeln, Docker-Topologie)
   - [`.agents/02-lore-canon.md`](.agents/02-lore-canon.md) (Kanon-Tabellen & Negativ-Liste gegen Halluzinationen)
   - [`.agents/04-workflows.md`](.agents/04-workflows.md) (Checklisten für Features, Migrationen, Protokoll & Abschluss)
   - [`.agents/decisions/`](.agents/decisions/) (Verbindliche Architecture Decision Records `ADR-001` bis `ADR-004`)

## 4. Operative Grundregeln
- **Scope-Disziplin:** Arbeite ausschließlich in den Zonen, die für den jeweiligen Sprint freigegeben sind (Sprint 2–4: `SERVER + SHARED`; `CLIENT`-TUI erst ab Sprint 5).
- **Kein Regel-Kopieren:** Adapter-Dateien (`CLAUDE.md`, `GEMINI.md`, `.agent/rules/`, `.cursor/rules/`, `.github/copilot-instructions.md`, `.windsurfrules`) bleiben reine Verweise auf `AGENTS.md` und `.agents/`.
- **Stop-on-Conflict:** Widerspricht eine Anforderung `00-invariants.md` oder `05-client-server-boundary.md`, stoppe sofort und frage aktiv beim User rück, statt still Grenzen aufzuweichen.

## 5. Pflicht-Abschluss-Gate (Definition of Done)
Kein Sprint, Bugfix oder Task gilt als abgeschlossen, bevor **beide** Schritte erfüllt sind:
1. `./scripts/check-invariants.sh` läuft vollständig und **grün (Exit-Code 0)** durch (SPDX, 3-Zonen-`cargo metadata`-Graph, Governance-Dateien, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`).
2. [`.agents/STATUS.md`](.agents/STATUS.md) wurde wahrheitsgemäß auf den neuen Ist-Stand aktualisiert.
