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

## 5. Workflow: Sprint- & Task-Abschluss (Pflicht-Gate)
- [ ] Führe `./scripts/check-invariants.sh` vollständig aus:
  - Check 1: SPDX-Header in allen `.rs`-Dateien.
  - Check 2: 3-Zonen-Abhängigkeitsgraph (`cargo metadata`).
  - Check 3: Vollständigkeit aller `.agents/`- und Adapter-Dateien.
  - Check 4: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (+ `cargo deny check licenses`, falls installiert).
- [ ] Aktualisiere `.agents/STATUS.md` (umgesetzte Punkte, neue Migrationen/Tests, aktueller Gate-Status).
