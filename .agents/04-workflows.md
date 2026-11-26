# 04 — Verbindliche Standard-Arbeitsabläufe (Workflows)

Jede Entwicklungsaufgabe folgt diesen reproduzierbaren Checklisten.

---

## 1. Workflow: Sprint- oder Task-Start
- [ ] Lies `.agents/STATUS.md` und prüfe aktuelle Phase, aktiven Zonen-Scope (`SERVER + SHARED` vs. `CLIENT`) sowie dokumentierte Altlasten.
- [ ] Prüfe gegen `00-invariants.md`, `03-scope-guardrails.md` und `05-client-server-boundary.md`, welche Crates berührt werden dürfen.
- [ ] Stelle sicher, dass keine gesperrten Zonen oder künftigen Sprint-Features (z. B. `ratatui` vor Sprint 5) eingeplant werden.

---

## 2. Workflow: Neue Spielmechanik implementieren
1. **Domain-Modell (`ash-core`, Zone `shared`):**
   - Implementiere Datenstrukturen, Validierungen und Berechnungsfunktionen rein deterministisch (ohne I/O, ohne Systemzeit, ohne ungeseedetes RNG).
   - Vergib in jeder neuen `.rs`-Datei in Zeile 1 `// SPDX-License-Identifier: GPL-3.0-or-later`.
2. **Deterministischer Unit-Test (`ash-core`):**
   - Schreibe präzise Positiv- und Negativ-Tests (inkl. Grenzwerte, Fairness-Caps und Rassen-/Klassen-Verbote).
3. **Protokoll-Erweiterung (`ash-protocol`, Zone `shared` — nur falls über das Netz sichtbar):**
   - Ergänze Client↔Server-Nachrichten im `client`-Protokollbereich bzw. Knoten↔Knoten-Nachrichten im `mesh`-Protokollbereich.
4. **Autoritative Server-Umsetzung (`ash-game-node` / `ash-auth-gateway` / `ash-db`, Zone `server`):**
   - Binde die `ash-core`-Regeln in den 25-Hz-ECS-Tick-Loop bzw. die QUIC-Request-Handler und DB-Transaktionen ein.
5. **Headless E2E-Verifikation (`ash-test-cli`, Zone `client`):**
   - Prüfe den End-to-End-Fluss ausschließlich über echte QUIC-Verbindungen (`ash-protocol`) und den separaten `ircd`.

---

## 3. Workflow: Datenbank-Migration (`ash-db`)
- [ ] Lege für jede Schema-Änderung eine **neue, fortlaufend nummerierte SQL-Datei** unter `crates/ash-db/migrations/` an (z. B. `0002_...sql`) inkl. `-- SPDX-License-Identifier: GPL-3.0-or-later`.
- [ ] **Niemals rückwirkend editieren:** Bereits eingecheckte Migrationen (`0001_initial_schema.sql` usw.) dürfen nicht nachträglich verändert werden, damit bestehende `sqlx`-Prüfsummen gültig bleiben.
- [ ] Verwende zur Kompilierzeit-Unabhängigkeit `sqlx::query(...)` + `sqlx::migrate!("./migrations")` (kein erzwungenes Live-DB-Makro beim Build).

---

## 4. Workflow: Protokoll-Änderung (`ash-protocol`)
- [ ] Halte `ash-protocol` frei von Async-Runtimes und Netzwerk-Sockets (reine Datenstrukturen + `serde`/`bincode`-Serialisierung).
- [ ] Trenne Client-Frames (`client`) strikt von Server-Mesh-Frames (`mesh`), sodass Clients niemals interne Gossip- oder Handover-Prepare-Nachrichten verwenden.
- [ ] Ergänze einen binären `bincode`-Roundtrip-Test für neue Nachrichten-Varianten.

---

## 5. Workflow: KI-Harness & Atomare Paket-Verifikation (`scripts/harness.sh`)
- Nutze für jedes Arbeitspaket aus `.agents/tasks/` konsequent den zentralen Harness:
  - `./scripts/harness.sh gate` — Führt `./scripts/check-invariants.sh` aus (SPDX, 3-Zonen-Graph, Governance, `fmt`, `clippy -D warnings`, `cargo test`).
  - `./scripts/harness.sh sim` — Führt alle für `CURRENT_SPRINT` freigeschalteten deterministischen `sim`-Szenarien (`harness/scenarios/*.toml`) inkl. `NDJSON`-Trace in `.harness-artifacts/` aus.
  - `./scripts/harness.sh e2e` — Startet den ephemeren `tmpfs`-Stack (`docker-compose.harness.yml`), prüft Container-Health, führt `00-smoke-mesh.toml` (bzw. Ziel-Szenario) aus, erzeugt bei Fehlern einen kompakten Digest in `.harness-artifacts/last-failure.md` und räumt per `trap` garantiert mit `down -v --remove-orphans` auf.
  - `./scripts/harness.sh all` — Führt `gate` → `sim` → `e2e` hintereinander aus.
- **Reservierter Sprint-5-Hook (TUI-Snapshot-Harness — NICHT vor Sprint 5 installieren!):**
  - Ab Sprint 5 (`ACTIVE_SCOPE = CLIENT`, Freigabe von `ash-client-tui`) wird `scripts/harness.sh` um ein Subcommand `tui-snap` erweitert, das Terminal-Views headless über `ratatui::backend::TestBackend` (80×24 & 120×40 Text-Snapshots) deterministisch gegen Referenz-Frames prüft. Vor Sprint 5 bleiben `ratatui` und `crossterm` im gesamten Workspace verboten!

---

## 6. Workflow: Sprint- & Task-Abschluss (Pflicht-Gate)
- [ ] Führe `./scripts/harness.sh gate && ./scripts/harness.sh sim` (sowie `./scripts/harness.sh e2e`, sofern ein Docker-Daemon verfügbar ist) vollständig mit Exit-Code `0` aus.
- [ ] Hake erledigte Arbeitspakete in `.agents/tasks/` ab und aktualisiere `.agents/STATUS.md` (umgesetzte Punkte, aktive Szenarien, aktueller Gate-Status).
