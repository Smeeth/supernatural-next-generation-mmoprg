# Sprint 2 — Verbindliche Arbeits-Checkliste (`ACTIVE_SCOPE = SERVER + SHARED`)

**Voraussetzung vor Start:** `CURRENT_SPRINT=2` setzen, `.agents/STATUS.md` prüfen und jedes Paket einzeln mit `./scripts/harness.sh gate && ./scripts/harness.sh sim` abschließen. Kein `ratatui`/`crossterm`!

---

## Paket 2A — Kampf-, Aggro- & Status-Kern (`ash-core` + `ash-game-node`)
- [ ] Deterministische Tick-basierte Buff-/Debuff-, Schild-, Cooldown- und Cast-Timer-Verwaltung in `ash-core` ausbauen (0 Systemzeit, explizites `dt_secs` + `DeterministicRng`).
- [ ] Verbindliche Tank-Aggro-Balance (Schaden austeilen + aktives Blocken/Einstecken vs. passives Ausweichen/Weglaufen) sowie exklusives 30-Sek.-Bann-Sheep (sofortiger Bruch bei direktem Schaden) im 25-Hz-ECS-Kampfloop verdrahten.
- [ ] Zwischen-Gate grün: `./scripts/harness.sh gate && ./scripts/harness.sh sim`.

## Paket 2B — Die 7 Rassen-Passives
- [ ] *Family Business* (`Winchester-Abkömmling`): +Schaden/+Aggro vs. übernatürliche Bosse/Dämonen, reduzierte klassische Fremdheilung.
- [ ] *Archivwissen* (`Mann der Schriften`): schnellere Bann-/Runen-Vorbereitung & 20 % Essenz-Rabatt.
- [ ] *Blutdurst* (`Vampir – Alpha-Blutlinie`): Lebensraub-Skalierung & Silber-/Feuer-/Totmannsblut-Anfälligkeit.
- [ ] *Rudelzähigkeit* (`Werwolf – Rudel-Überlebende`): +Grundzähigkeit & Regeneration außerhalb von Silbertreffern.
- [ ] *Häutung* (`Gestaltwandler`), *Buch der Verdammten* (`Hexe / Hexer`), *Avalon-Glamour* (`Fee / Fae` inkl. Eisen-Tabu & Steinsalz-Malus).
- [ ] Zwischen-Gate grün: `./scripts/harness.sh gate && ./scripts/harness.sh sim`.

## Paket 2C — Die 3 Tank-Klassen
- [ ] `Eisen-Krieger`: Wut/Adrenalin, aktives Blocken/Parieren + Treffer schlucken.
- [ ] `Blutdurst-Tank`: Dual-Wield, minimale Vermeidung, Selbstheilung aus ausgeteiltem Schaden.
- [ ] `Bann-Wächter`: Schlag-Absorption in Henochische Runen -> AoE-Schockwelle für Gruppen-Aggro.
- [ ] Zwischen-Gate grün: `./scripts/harness.sh gate && ./scripts/harness.sh sim`.

## Paket 2D — Die 3 Heiler-Klassen
- [ ] `Blut-Siphon`: Lebensraub-Gruppenheilung + Notfall-Skill *Eigenblut-Spende* bei unangreifbaren Boss-Phasen.
- [ ] `Bann-Mönch`: Exorzismus-Gebete & geweihter Stab (Nahkampf + direkte Berührungs-Heilung/-Schildung).
- [ ] `Hoodoo-Brauer`: Mojos/Kräuter/Salz/Bier (Heilung, Schilde, Aggro-Senkung) + direkter Schaden **ausschließlich** über Voodoo-Puppen.
- [ ] Zwischen-Gate grün: `./scripts/harness.sh gate && ./scripts/harness.sh sim`.

## Paket 2E — Die 3 Support-Klassen
- [ ] `Bunker-Tüftler`: Physische Salz-Selbstschussgeschütze, Eisen-/Salz-Barrikaden (AoE-Deckung) & temporäre Ausrüstungs-Buffs.
- [ ] `Sigillen-Elementarist`: Schilde, Schadens- & Tempo-Buffs **ausschließlich als Boden-Sigille**, null Heilung, Energie aus Schaden + Notfall-*Eigenblut-Sigille*.
- [ ] `Trickster-Köder`: Notfall-Spott vom Tank -> 1 Treffer -> *Chaos-Sprung* + AoE-Gruppenbuff -> **automatische Aggro-Rückgabe an den Tank (Trolling-Schutz)**.
- [ ] Zwischen-Gate grün: `./scripts/harness.sh gate && ./scripts/harness.sh sim`.

## Paket 2F — Die 3 Nahkampf-DD-Klassen
- [ ] `Macheten-Berserker`: Crits lösen *Jagd-Raserei* + stapelbare Blutungen mit Schadens-Skalierung aus.
- [ ] `Kneipen-Prügler`: Langsame wuchtige 2H-Hiebe + *Bier ins Gesicht* (Zufalls-Debuff, Blindheit oder stapelbare *Besoffenheit*).
- [ ] `Nacht-Pirschjäger`: Fokus ausschließlich über Auto-Attacks; Boss-Ulti *Doppelagent* erst ab **≤ 50 % Boss-HP** (8 Sek., Boss-Buff-Diebstahl, Verbündeten-HP niemals < 1!).
- [ ] Zwischen-Gate grün: `./scripts/harness.sh gate && ./scripts/harness.sh sim`.

## Paket 2G — Die 3 Fernkampf-DD-Klassen & Aktivierung `02-combat-4role.toml`
- [ ] `Winchester-Sniper`: 3 Waffenmodi (Vorderlader/Armbrust/Bogen), Stillstand-Schadens-Stacks, **sofortiger Bewegungs-Reset auf 0**, Bann-Sheep *Blend-Augenbinde*.
- [ ] `Runen-Kleriker`: 20-%-Schild-Konversion (1/5) mit hartem **Schild-Cap pro Ziel** & reduziertem Basisschaden, Bann-Sheep *Teufelsfalle / Bann-Sigille*.
- [ ] `Salz-&-Benzin-Pyromant`: 3 Feuer-Casts stapeln *Flamme*, Instant *Flächenbrand* ab **≥ 3 Zielen** im **5-m-Radius**, Bann-Sheep *Heiliger-Öl-Feuerring*.
- [ ] `harness/scenarios/02-combat-4role.template.toml` zu vollständigem `02-combat-4role.toml` ausbauen und mit `CURRENT_SPRINT=2 ./scripts/harness.sh gate && CURRENT_SPRINT=2 ./scripts/harness.sh sim` (sowie `e2e`, falls Docker aktiv) abnehmen.
