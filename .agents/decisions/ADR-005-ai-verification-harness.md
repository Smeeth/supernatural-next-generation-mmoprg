# ADR-005: Fest installierter KI-Selbstverifizierungs-Harness (`scripts/harness.sh`)

- **Status:** Accepted (Sprint 1.7)
- **Kontext:** Über Sprint 2–5 entwickeln verschiedene KI-Modelle komplexe Kampf-, Quest-, Raid- und TUI-Systeme. Unstrukturierte 10.000-Zeilen-Container-Logs sprengen das Kontextfenster von LLMs, während verschmutzte Docker-Volumes zu nicht-reproduzierbaren E2E-Fehlern führen.
- **Entscheidung:**
  1. **Deklarative TOML-Szenarien (`harness/scenarios/*.toml`):** Ein einheitliches Schema in `ash-core::scenario` bildet sowohl schnelle, I/O-freie Domain-Simulationen (`mode = "sim"`, deterministischer SplitMix64-Seed) als auch Blackbox-Cluster-Läufe (`mode = "e2e"` in `ash-test-cli`) inkl. `min_sprint`-Gating und `NDJSON`-Tick-Traces ab.
  2. **Ephemerer E2E-Stack (`docker-compose.harness.yml`):** Nutzt den isolierten Projektnamen `ash-harness`, `tmpfs`-Mounts für PostgreSQL und Redis (null Restzustand zwischen Läufen) und striktes `trap`-Cleanup (`down -v --remove-orphans`).
  3. **Kontext-schonende Fehlerdiagnose:** Bei Fehlschlägen erzeugt `ash-test-cli` strukturierte Fehlerblöcke (`FAILED_STEP`, `EXPECTED`, `ACTUAL`, `STATE_SNAPSHOT`), und `scripts/harness.sh` bündelt gefilterte Fehlersignale plus maximal 30 Tail-Zeilen je Kern-Container in `.harness-artifacts/last-failure.md` (per Ignore-Dateien vor blindem Kontext-Scan geschützt).
- **Konsequenzen:** Jedes Arbeitspaket in Sprint 2–5 lässt sich einheitlich über `./scripts/harness.sh gate | sim | e2e | all` ohne Crate-Wildwuchs verifizieren.
