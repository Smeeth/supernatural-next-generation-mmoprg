# 03 — Scope-Guardrails, Change-Budget & Anti-Scope-Creep

Diese Leitplanken verhindern unkontrolliertes Ausufern (Scope Creep), Vorgriffe auf spätere Phasen und unbeabsichtigte Seiteneffekte über Sprint-Grenzen hinweg.

---

## 1. Sprint- & Zonen-Lock
- Jeder Sprint deklariert explizit seinen freigegebenen **Zonen-Scope**:
  - **Sprint 2–4:** Ausschließlich `SERVER + SHARED` (plus gezielte headless E2E-Prüfungen in `ash-test-cli`, sofern beauftragt).
  - **Ab Sprint 5:** Freigabe der produktiven `CLIENT`-Zone (`ash-client-tui`).
- Dateiänderungen in gesperrten Zonen sind ohne ausdrückliche Freigabe des Users verboten.

---

## 2. Verbot des Vorziehens künftiger Sprints
1. **Kein TUI-Client vor Sprint 5:** Weder `ratatui` noch `crossterm` noch eine Crate `ash-client-tui` dürfen vor Sprint 5 angelegt oder in `Cargo.toml` eingetragen werden.
2. **Keine vorzeitige Raid-Expansion vor Sprint 4:** Bestehende Raid-Grundstrukturen aus Sprint 1 bleiben unangetastet, bis Sprint 4 den vollen Raid-Ausbau beauftragt.
3. **Kein Infrastruktur-Wildwuchs:** Kein Web-Dashboard, kein HTTP-/REST-/GraphQL-Gateway, kein gRPC-Sidecar, kein Prometheus/Grafana-Stack und keine zusätzlichen Message-Broker ohne expliziten Projektauftrag.

---

## 3. No-Unplanned-Expansion (Crates, Container & Abhängigkeiten)
1. **Neue Crates:** Dürfen nur bei expliziter Beauftragung oder nach dokumentiertem Kurz-ADR in `.agents/decisions/` angelegt werden. Jede neue Crate **muss vom ersten Commit an** `[package.metadata.ash] zone = "shared" | "server" | "client"` setzen.
2. **Neue Docker-Services:** `docker-compose.yml` darf nicht eigenmächtig um neue Container erweitert werden.
3. **Abhängigkeits-Disziplin:** Bevorzuge die bereits im Workspace deklarierten `[workspace.dependencies]`. Jede neue externe Abhängigkeit muss GPL-3.0-or-later-kompatibel sein (`deny.toml`) und zur jeweiligen Zonen-Reinheit passen.

---

## 4. Surgical-Diff-Regel & Test-Integrität
1. **Chirurgische Änderungen:** Keine großflächigen Formatierungs-, Umbenennungs- oder Stil-Rewrites in unbeteiligten Modulen früherer Sprints.
2. **Unantastbare Test-Suite:** Bestehende Unit- und Integrationstests dürfen **niemals gelöscht, abgeschwächt oder mit `#[ignore]` stummgeschaltet** werden, um einen Build grün zu färben.

---

## 5. Datengetrieben vor Hardcoding
- Zonen-Parameter, Einstiegs- und Story-Quests, Loot-/Essenz-Tabellen sowie NPC-Dialoge werden nicht tief in prozeduralem Rust-Code vergraben, sondern ab Sprint 3 strukturiert in validierten TOML-Dateien unter `/data/world/*.toml` gepflegt (aktueller Sprint-1-Pfad: `data/zones.toml` & `data/quests.toml`).

---

## 6. Verbindliche Eskalationsregel (Stop-on-Conflict)
Erkennt ein Agent einen Widerspruch zwischen einem User-Prompt (oder einem geplanten Refactoring) und:
- [`.agents/00-invariants.md`](00-invariants.md) (z. B. Lizenz, Engel/Dämonen-Verbot, 16-Slot-Prinzip, QUIC/IRC-Trennung) **oder**
- [`.agents/05-client-server-boundary.md`](05-client-server-boundary.md) (3-Säulen-Trennwand `SHARED` / `SERVER` / `CLIENT`),

so gilt **sofortiger Baustopp für den betroffenen Punkt**: Der Konflikt wird klar benannt und aktiv rückgefragt, statt stillschweigend Projektgesetze zu verletzen.
