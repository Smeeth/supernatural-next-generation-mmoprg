# 05 — Die 3-Säulen-Trennwand im Monorepo (`SHARED` / `SERVER` / `CLIENT`)

Obwohl alle Rust-Crates von *Ash-Net: New Lawrence* in einem gemeinsamen Cargo-Workspace verwaltet werden, sind **Client** und **Server** architektonisch, logisch und maschinell wie zwei getrennte Repositories isoliert.

---

## Säule A — Harte Workspace- & Crate-Grenzen (Einbahnstraßen-Matrix)

Jede Workspace-Crate muss in ihrer `Cargo.toml` exakt eine Zone über `[package.metadata.ash] zone = "..."` deklarieren:

### 1. Zone `shared` (`ash-core`, `ash-protocol`)
- **Aufgabe:** Reine, deterministische Domain-Typen, Spielregeln (`ash-core`) sowie serialisierbare binäre Wire-Frames (`ash-protocol`).
- **Erlaubte Abhängigkeiten:** Untereinander (`ash-protocol` -> `ash-core`) sowie reine Daten-/Serialisierungs-Crates (`serde`, `bincode`, `uuid`, `thiserror`, `toml`, `bytes`).
- **STRIKT VERBOTEN (direkt & transitiv):**
  - Jede Crate der Zonen `server` und `client`.
  - Laufzeit-, I/O-, DB- und UI-Crates: `tokio`, `quinn`, `sqlx`, `redis`, `ratatui`, `crossterm`.

### 2. Zone `server` (`ash-db`, `ash-auth-gateway`, `ash-game-node`, `ash-irc-bridge`, künftig `ash-admin-cli`)
- **Aufgabe:** Authentifizierung, Persistenz, 25-Hz-Headless-ECS-Simulation, mTLS-Server-Mesh, `PLAYER_HANDOVER` und IRC-Service-Kopplung.
- **Erlaubte Abhängigkeiten:** `shared`-Crates, andere `server`-Crates sowie Server-Infrastruktur (`tokio`, `quinn`, `rustls`, `rcgen`, `sqlx`, `redis`, `hecs`, `argon2`).
- **STRIKT VERBOTEN (direkt & transitiv):**
  - Jede Crate der Zone `client` (`ash-test-cli`, `ash-client-tui`).
  - Jegliche TUI-/GUI- oder Terminal-Rendering-Bibliotheken: `ratatui`, `crossterm`.

### 3. Zone `client` (`ash-test-cli` in Sprint 1–4; `ash-client-tui` ab Sprint 5)
- **Aufgabe:** Senden von Spieler-Intents, Darstellen bestätigter Server-Snapshots und IRC-Chat-Teilnahme (bzw. E2E-Blackbox-Verifikation gegen laufende Server-Endpunkte).
- **Erlaubte Abhängigkeiten:** `shared` (`ash-protocol` sowie lesende Typen/UX-Validierungs-Hilfen aus `ash-core`), Client-Netzwerk (`tokio`, `quinn`, `rustls`, TCP-IRC-Client) und **erst ab Sprint 5** `ratatui` / `crossterm`.
- **STRIKT VERBOTEN (direkt & transitiv):**
  - Jede Crate der Zone `server` (`ash-db`, `ash-auth-gateway`, `ash-game-node`, `ash-irc-bridge`, `ash-admin-cli`).
  - Datenbank-, Cache- und Server-ECS-Crates: `sqlx`, `redis`, `hecs`, `bevy_ecs`.

### 4. Einzige erlaubte Brücken zwischen Client und Server
1. **`ash-protocol`:** Binäre Client↔Server-QUIC-Streams und Unreliable Datagrams.
2. **Separater `ircd`:** Textbasierter Chat und Slash-Kommandos (`/winken`, `/salzen`, `/emf` usw.).

---

## Säule B — Server-Authoritative-Prinzip (Anti-Cheat & Logik-Trennwand)

1. **Keine autoritative Spiellogik im Client:**
   - Der Client berechnet niemals verbindlichen Schaden, Crit-Würfe, Aggro-Tabellen, Cooldown-Freigaben, Loot-/Essenz-Drops, Omen-Übergänge oder `PLAYER_HANDOVER`-Tokens.
2. **Strikte Datenfluss-Richtung:**
   - **Client → Server:** Ausschließlich **Intents / Eingaben** (Bewegungsvektor, Ziel-Interaktion, Skill-Wunsch, IRC-Textzeile).
   - **Server → Client:** Ausschließlich **autoritative Snapshots, bestätigte Ergebnisse und Boss-Telegraphen**.
   - Optionale UX-Vorprüfungen im Client (z. B. Ausgrauen unzulässiger Rassen-Klassen-Paare mithilfe von `ash-core`) dienen nur der Bedienbarkeit und ersetzen niemals die harte Server-Prüfung.
3. **Modulare Protokoll-Kapselung in `ash-protocol`:**
   - Client↔Server-Nachrichten (`client`-Modul) und interne Knoten↔Knoten-Nachrichten (`mesh`-Modul: Gossip, Heartbeats, `PlayerHandoverPrepare`, `IrcSlashEvent`) sind modulseitig sauber getrennt; Client-Crates konsumieren niemals `mesh`-Nachrichten.
4. **Build-, Container- & Secret-Isolation:**
   - Produktive Server-Targets in `Dockerfile` und Standard-Services in `docker-compose.yml` bauen und starten ausschließlich `SERVER`-Binaries (`ash-test-cli` ist über `profiles: ["test"]` isoliert; `ash-client-tui` wird niemals in Server-Container eingebaut).
   - `DATABASE_URL`, `REDIS_URL`, `ca.key` sowie Server-/Mesh-mTLS-Private-Keys (`*.key`) dürfen niemals an Client-Prozesse ausgeliefert oder gemountet werden (Clients erhalten ausschließlich das öffentliche `ca.crt`).

---

## Säule C — Maschinelle Durchsetzung (`./scripts/check-invariants.sh`)

Die 3-Säulen-Trennwand verlässt sich nicht auf Disziplin allein, sondern wird maschinell erzwungen:
- `./scripts/check-invariants.sh` liest den aufgelösten Cargo-Graph über `cargo metadata --format-version 1` aus und prüft mit Python 3 alle direkten und transitiven `normal`- und `build`-Abhängigkeiten jeder Workspace-Crate.
- Fehlt `[package.metadata.ash] zone` in einer Crate oder existiert eine verbotene Kante zwischen `shared`, `server` und `client`, bricht das Skript mit konkretem Pfadnachweis (`ZONE VIOLATION: ...`) und Exit-Code `!= 0` hart ab.
