# 01 — Architektur, Workspace-Topologie & Container-Fragmentierung

## 1. Reale Cargo-Workspace-Topologie (Stand nach Sprint 1 / 1.5)

Alle Crates liegen unter `crates/` und deklarieren ihre Architektursäule über `[package.metadata.ash] zone = "..."`:

| Crate | Pfad | Zone | Typ | Verantwortlichkeit |
| :--- | :--- | :--- | :--- | :--- |
| `ash-core` | `crates/ash-core` | `shared` | Lib | I/O-freie Domain-Logik: 7 Rassen, 15 Klassen, 48er Matrix, 16-Slot-Ausrüstung, Okkulte Essenzen, Kampfregeln, Omen-Index (0–3), Boss-State-Machines & TOML-Katalog-Modelle. |
| `ash-protocol` | `crates/ash-protocol` | `shared` | Lib | Reine binäre `serde` + `bincode` Wire-Frames (`client`-, `mesh`- und `codec`-Module, vollständig frei von `tokio`/`quinn`). |
| `ash-db` | `crates/ash-db` | `server` | Lib | Persistenz-, Cache- & Server-Transportschicht: PostgreSQL 16 (`sqlx` + Migrationen), Redis 7 sowie gemeinsame Server-mTLS/QUIC-Infrastruktur (`transport.rs`). |
| `ash-auth-gateway` | `crates/ash-auth-gateway` | `server` | Lib + Bin | QUIC-Auth-Server (`:4430`): Argon2-Passwort-Hashing, Registrierung/Login, Charakter-Fabrik (Matrix-Prüfung, Engel-/Dämonen-Sperre, 16er Grau-Set in `zone_roadhouse`), Crash-Recovery-Fallback & PKI-Init-Modus. |
| `ash-game-node` | `crates/ash-game-node` | `server` | Lib + Bin | Headless ECS-Zonen- & Raid-Knoten (`hecs`, 20–30 Hz, Default 25 Hz): `NODE_ROLE=hub` oder `mirror`, mTLS-QUIC-Gossip, Unreliable Datagrams, Reliable Gameplay-Streams, `PLAYER_HANDOVER` & `tests/cluster_e2e.rs`. |
| `ash-irc-bridge` | `crates/ash-irc-bridge` | `server` | Lib + Bin | IRC-Service-Client (`AshBridge`) an `ngIRCd`: Parst `/winken`, `/grüßen`, `/prost`, `/salzen`, `/emf`, `/exorzieren`, `/tacker` -> mTLS-QUIC-ECS-Events und sendet Omen-Stufe-2/3-Funk-Alarme nach `#new_lawrence`. |
| `ash-test-cli` | `crates/ash-test-cli` | `client` | Bin | Reiner Blackbox-QUIC/IRC-Smoke-Test-Runner für Sprint 1–4 (ohne Ratatui, ohne jegliche Server-Abhängigkeiten). |

### Reservierte künftige Crates (erst ab freigegebenem Sprint anlegen!)
- `ash-admin-cli` (`zone = "server"`, Live-Ops / Cluster-Steuerung)
- `ash-client-tui` (`zone = "client"`, Ratatui-Terminal-Client — **strikt gesperrt bis Sprint 5!**)

---

## 2. Determinismus-Gesetz für `ash-core`

`ash-core` ist das mathematische Regelwerk des Spiels und muss zu 100 % deterministisch und reproduzierbar testbar bleiben:
1. **Keine Systemuhr:** Kein `std::time::SystemTime::now()`, `Instant::now()` oder `chrono::Utc::now()` in `ash-core`. Zeitspannen werden als explizite `dt_secs: f32` oder Tick-Zähler übergeben.
2. **Kein ungeseedeter Zufall:** Kein `rand::thread_rng()` oder versteckter globaler Zustand in `ash-core`. Zufallswerte (z. B. *Chaos-Sprung*-Winkel, Crit-Flags, Proc-Würfe) werden vom aufrufenden Server-System als explizite Parameter oder deterministischer Seed injiziert.
3. **Null I/O:** Kein Dateisystem-, Netzwerk-, Datenbank- oder Async-Runtime-Code in `ash-core`.

---

## 3. Docker-Compose-Topologie & Netzwerk-Isolation

`docker-compose.yml` segmentiert die Infrastruktur über **3 getrennte Docker-Netzwerke**:
- **`ash-mesh-net` (Bridge):** QUIC-Verkehr zwischen `auth-gateway`, `game-node-*` und `irc-bridge`.
- **`data-net` (`internal: true`):** Vollständig nach außen abgeschottetes Datennetz für `postgres` und `redis`.
- **`irc-net` (Bridge):** Dediziertes Chat-Netz zwischen `ircd` und `irc-bridge`.

### Container-Dienste
1. **`pki-init`:** Einmaliger Init-Container (`service_completed_successfully`), erzeugt lokale Ash-Net Root-CA (`ca.crt`) und Dienst-Zertifikate im Volume `pki-certs`.
2. **`postgres`:** PostgreSQL 16 (`postgres:16-alpine`) auf `data-net` mit Healthcheck; Migrationen laufen automatisch beim Start der Server-Dienste.
3. **`redis`:** Redis 7 (`redis:7-alpine`) auf `data-net` mit Healthcheck.
4. **`ircd`:** Eigenständiger `ngIRCd`-Container (`infra/ircd/Dockerfile` + `ngircd.conf`) auf `:6667/tcp` mit vorkonfigurierten Kanälen `#roadhouse`, `#new_lawrence`, `#zone_salzminen`, `#men_of_letters` und Oper-Zugang für `irc-bridge`.
5. **`auth-gateway`:** QUIC-Port `:4430/udp`, verbunden mit `ash-mesh-net` + `data-net`.
6. **`game-node-roadhouse`:** Primärer Seed/Hub-Knoten (`NODE_ROLE=hub`, `ZONES=zone_roadhouse`, Client `:4433/udp`, mTLS-Mesh `:5000/udp`).
7. **`game-node-frontier`:** Gespiegelter Zonen-Knoten (`NODE_ROLE=mirror`, `HUB_ADDR=game-node-roadhouse:5000`, `ZONES=zone_new_lawrence_west,zone_salzminen`, Client `:4434/udp`, mTLS-Mesh `:5001/udp`).
8. **`irc-bridge`:** Verbindet `ash-mesh-net` (mTLS-QUIC zum Hub) und `irc-net` (`ircd:6667`).
9. **`game-node-raid` (Profil `raids`):** Skalierbarer Raid-Knoten (`ZONES=zone_hoellentor_alpha,zone_relais_kathedrale,zone_fegefeuer_riss`, Client `:4435/udp`, mTLS-Mesh `:5002/udp`).
10. **`ash-test-cli` (Profil `test`):** On-Demand E2E-Test-Container (`docker compose --profile test run --rm ash-test-cli`), niemals Teil des produktiven Server-Dauerbetriebs.
