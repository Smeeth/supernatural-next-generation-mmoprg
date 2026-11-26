# 00 — Unverhandelbare Projekt-Gesetze (Invariants)

Diese Gesetze gelten über alle Sprints hinweg ausnahmslos. Kein Agent darf sie ohne explizite Architektur-Änderung durch den Projektleiter abschwächen oder umgehen.

---

## 1. Lizenz- & Prozess-Invarianten
1. **Eigener Code:** Sämtlicher eigener Quellcode, Skripte, SQL-Migrationen und Dockerfiles stehen unter **`GPL-3.0-or-later`**.
2. **SPDX-Pflicht:** Jede `.rs`-Datei im Workspace beginnt in der ersten Zeile mit:
   ```rust
   // SPDX-License-Identifier: GPL-3.0-or-later
   ```
3. **Abhängigkeits-Kompatibilität:** Es dürfen ausschließlich GPLv3-kompatible Rust-Crates eingebunden werden (`GPL-3.0-or-later`, `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `Unicode-DFS-2016`, `CC0-1.0`, `MPL-2.0`), überwacht durch `deny.toml`.
4. **IRCd-Prozessgrenze:** Der IRC-Server (`ngIRCd` / `InspIRCd`, GPLv2+) läuft **immer** als eigenständiger Prozess/Container und kommuniziert ausschließlich über TCP-Sockets mit `ash-irc-bridge` (keinerlei statische oder dynamische C-/Rust-Verlinkung).

---

## 2. Protokoll- & Netzwerk-Invarianten
1. **Strikte Protokoll-Trennung:**
   - **Game-State & Cluster-Mesh:** Ausschließlich binär über **QUIC** (`quinn`, TLS 1.3; mTLS zwischen Server-Knoten). Kein HTTP/REST/JSON/GraphQL für Gameplay-Traffic.
   - **Soziale Kommunikation & Slash-Emotes:** Ausschließlich nativ über den separaten **IRCd** (`#roadhouse`, `#new_lawrence`, `#zone_salzminen`, `#men_of_letters`) gekoppelt über `ash-irc-bridge`.
2. **QUIC-Kanal-Aufteilung:**
   - **Unreliable Datagrams:** Hochfrequente Bewegungs-Inputs und Zonen-Positions-Snapshots.
   - **Reliable Streams:** Auth, Charaktererstellung, Kampf-Aktionen/Procs, Inventar, Essenz-Aufwertungen, Quests und `PLAYER_HANDOVER`.
3. **Nahtloser Handover & Crash-Fallback:**
   - Zonenwechsel erfolgt über `PLAYER_HANDOVER` (mTLS-QUIC-Stream + kurzlebiges Redis-Handover-Token).
   - Ist der Ziel- oder letzte Zonen-/Raid-Knoten eines Spielers ausgefallen, leitet das System den Charakter beim Login/Reconnect automatisch und verlustfrei nach `zone_roadhouse` (*Harvelle’s Roadhouse*) zurück.

---

## 3. Headless- & Server-Authoritative-Invarianten
1. **100 % Server-Authoritative:** Alle Entscheidungen über Position, Schaden, Crits, Aggro, Cooldowns, Loot, Essenz-Verbrauch, Quest-Fortschritt, Omen-Stufen und Zonen-Handover fallen ausschließlich auf dem Server.
2. **Strikt Headless Server:** Server-Crates (`ash-db`, `ash-auth-gateway`, `ash-game-node`, `ash-irc-bridge`, `ash-admin-cli`) laufen mit fester Simulations-Tick-Rate (**20–30 Ticks/Sekunde**, Standard 25 Hz) und besitzen **null** TUI-, GUI- oder Terminal-Rendering-Abhängigkeiten (`ratatui`, `crossterm` etc. sind im Server verboten).

---

## 4. Harte Spielwelt- & Balancing-Invarianten
1. **Strikt unspielbare Übermächte:** **Engel und Dämonen** dürfen niemals als Spieler-Rasse erstellt oder gewählt werden (harte Sperre in `ash-core`, `ash-auth-gateway` und PostgreSQL-`CHECK`-Constraint). Sie existieren ausschließlich als Raid-Bosse, Fraktionsgegner und Omen-Welt-Events.
2. **Verbindlicher Startpunkt:** Jeder neu erstellte Charakter spawnt ausnahmslos in `zone_roadhouse` (*Harvelle’s Roadhouse*).
3. **Exakt 16 feste Ausrüstungs-Slots („Kofferraum-Prinzip“):**
   - Gilt für Spieler **und** ausgerüstete NPCs (Slots 1–16, darunter **Slot 12 `Bauchtasche`** für Kampf-Schnellzugriff und **Slot 13 `Rucksack`** als mobiler Impala-Kofferraum).
   - Bei Charaktererstellung starten alle 16 Slots zwingend in **Grau (Gewöhnlich)**.
   - Kein Wegwerf-Loot-Grind: Progression erfolgt über **5 Qualitätsstufen** (`Grau` → `Grün` → `Blau` → `Violett` → `Gold`) mittels **Okkulter Essenzen**.
4. **Exakte Rassen- & Klassen-Struktur (48 gültige Paare):**
   - Exakt **7 spielbare Rassen** und **15 Klassen** über **4 Grundrollen** (je 3 Tanks, Heiler, Support, Nahkampf-DDs, Fernkampf-DDs).
   - **Tank-Verbot:** `Gestaltwandler`, `Hexe / Hexer` und `Fee / Fae` dürfen niemals eine Tank-Klasse spielen.
   - **Material-Tabus:** Absolutes **Eisen-Tabu** für `Fee / Fae`; Verbot schwerer Nahkampf-Stahlwaffen für `Hexe / Hexer`.
5. **Unverhandelbare Kampfregeln:**
   - **Tank-Aggro-Balance:** Ein Tank hält Aggro **nur**, wenn er ausgewogen Schaden austeilt **und** einsteckt (aktives Blocken/Parieren im Nahkontakt zählt als Risiko; rein passives Ausweichen oder Weglaufen führt zu massivem Aggro-Verlust).
   - **Exklusives 30-Sek.-„Bann-Sheep“:** Ausschließlich die **3 Fernkampf-DD-Klassen** (`Winchester-Sniper`, `Runen-Kleriker`, `Salz-&-Benzin-Pyromant`) können einen feindlichen NPC für 30 Sekunden bannen; der Bann bricht sofort bei direktem Schaden.
   - **Trickster-Trolling-Schutz:** Nach dem Notfall-Spott und eingesteckten Treffer des `Trickster-Köders` springt die Aggro automatisch sauber zurück zum Tank.
