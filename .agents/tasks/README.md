# Atomare Task-Orchestrierung (`.agents/tasks/`)

Um Kontext-Überlauf, unentdeckte Seiteneffekte und halbfertige Groß-Commits in komplexen Sprints zu verhindern, gilt für alle KI-Modelle folgende **Pflichtregel zur Arbeitspaket-Zerlegung**:

## 1. Ein Paket = Ein abgeschlossener Verifikations-Zyklus
Große Sprints (insbesondere **Sprint 2** mit 7 Rassen-Passives und 15 Klassen über 4 Rollen) dürfen **niemals** in einem einzigen monolithischen Edit-Durchlauf ohne Zwischenprüfung geschrieben werden. Stattdessen wird jedes Arbeitspaket einzeln abgearbeitet und unmittelbar verifiziert:

```bash
./scripts/harness.sh gate && ./scripts/harness.sh sim
```

Erst wenn **beide** Kommandos mit Exit-Code `0` grün sind, wird das Arbeitspaket in der jeweiligen Breakdown-Datei abgehakt (`[x]`) und das nächste Paket begonnen.

## 2. Umgang mit Fehlern & Kontext-Budget
- Schlägt ein Szenario fehl, lies **zuerst** den kompakten Fehler-Digest unter `.harness-artifacts/last-failure.md` (bzw. den `NDJSON`-Trace `.harness-artifacts/<szenario>.ndjson`), statt tausende Roh-Logzeilen in das Kontextfenster zu laden.
- Bestehende Tests und aktive Szenarien (`00-smoke-mesh.toml`, `01-sim-invariants.toml`) dürfen niemals gelöscht oder abgeschwächt werden.

## 3. Aktive Sprint-Breakdowns
- [`sprint-2-breakdown.md`](sprint-2-breakdown.md) — Verbindliche 7-Paket-Zerlegung (`2A` bis `2G`) für den Ausbau des 4-Rollen-Kampfsystems in Sprint 2.
