# Ash-Net: New Lawrence — Server-First Terminal-MMORPG

**Lizenz:** `GPL-3.0-or-later` ([LICENSE](LICENSE))  
**Setting:** *Die Neue Front* — New Lawrence im Andromeda-System, über 30 Jahre nach den Ereignissen von *Supernatural*.

Dieses Repository implementiert das **Server-First-Backend** von *Ash-Net: New Lawrence* in **Rust**:
- **QUIC-Server-Mesh (`quinn`, TLS 1.3 & mTLS):** Strikte Trennung von Unreliable Datagrams (25-Hz-Positions-Snapshots) und Reliable Streams (Auth, Quests, 16-Slot-Ausrüstung, Kampf & nahtloser `PLAYER_HANDOVER`).
- **Headless ECS (`hecs`):** Deterministische Zonen-Simulation (20–30 Hz, Standard 25 Hz) ohne Grafik-Abhängigkeiten.
- **Separater IRC-Funk (`ngIRCd` + `ash-irc-bridge`):** Soziale Kommunikation, RP-/Gameplay-Slash-Kommandos (`/winken`, `/grüßen`, `/prost`, `/salzen`, `/emf`, `/exorzieren`, `/tacker`) und automatische Omen-Funk-Alarme in `#new_lawrence`.
- **PostgreSQL 16 + Redis 7 (`sqlx` + `redis`):** Automatische Datenbank-Migrationen beim Start, kurzlebige Handover-Tokens und automatischer Crash-Fallback nach *Harvelle's Roadhouse* (`zone_roadhouse`).

---

## Cargo-Workspace-Übersicht

| Crate | Beschreibung |
| :--- | :--- |
| `crates/ash-core` | Rassen-Klassen-Matrix (7 Rassen, 15 Klassen in 4 Rollen, Verbot von Engeln/Dämonen), 16 Ausrüstungs-Slots, Okkulte Essenzen, Kampfregeln & Raid-Bosse (*Malphas*, *Sariel*) |
| `crates/ash-protocol` | Kompaktes binäres `bincode`-Protokoll über QUIC-Streams & Datagramme sowie `rcgen`/`rustls` mTLS-Infrastruktur |
| `crates/ash-db` | PostgreSQL-16-Repositories inkl. SQLx-Migrationen & Redis-7-Session/Handover/Routing-Layer |
| `crates/ash-auth-gateway` | QUIC-Login, Charaktererstellung mit 16 grauen Start-Items in `zone_roadhouse` & Crash-Recovery-Routing |
| `crates/ash-game-node` | Eigenständiger Zonen-/Raid-Knoten (`NODE_ROLE=hub` oder `mirror`) mit mTLS-Gossip & `PLAYER_HANDOVER` |
| `crates/ash-irc-bridge` | Kopplung an `ngIRCd` für In-Game-Slash-Kommandos und Omen-Stufe-2/3-Alarme |
| `crates/ash-test-cli` | Schlanker, headless End-to-End-Integrations- & Smoke-Test-Runner |

---

## Schnellstart per Docker Compose

### 1. Basis-Cluster starten (PKI, Postgres, Redis, IRCd, Auth, Hub-Roadhouse, Frontier-Mirror, IRC-Bridge)

```bash
docker compose up --build -d
```

### 2. Optional mit skalierbarem Raid-Knoten starten (*Malphas* & *Sariel*)

```bash
docker compose --profile raids up --build -d
```

### 3. End-to-End-Smoke-Test gegen den laufenden Docker-Cluster ausführen

```bash
docker compose --profile test run --rm ash-test-cli
```

Der Runner prüft vollautomatisch:
1. Account-Registrierung & strikte Ablehnung von `Engel` / `Dämon` am `auth-gateway`.
2. Charaktererstellung mit 16 grauen Start-Items in `zone_roadhouse` (*Harvelle's Roadhouse*).
3. Unreliable QUIC-Datagramme & 25-Hz-ECS-Snapshots.
4. Interaktion mit **Jo Harvelles Echo**, **Ellen Harvelles Echo** und **Ashs Terminal** (Live-Mesh-Diagnose).
5. `/salzen` in `#roadhouse` über `ircd` → `irc-bridge` → `game-node-roadhouse`.
6. Überschreiten der Zonengrenze mit Live-`PLAYER_HANDOVER` auf `game-node-frontier` (`zone_new_lawrence_west`).
7. `/emf` in `#new_lawrence` → Abschluss von Quest 1 (*„Salz auf die Schwelle“*) → Aufwertung der Hauptwaffe mit *Steinsalz-Asche* auf **Grün (Geweiht)** sowie 30-Sek.-Bann-Sheep.
8. Auslösen eines Omen-Durchbruchs (Stufe 2) inkl. automatischem Funk-Alarm in `#new_lawrence`.

---

## Lokale Kompilierung & Unit-/E2E-Tests (ohne Docker)

```bash
# Alle Domain-, Matrix-, Kampf- und End-to-End-Mesh-Tests ausführen
cargo test --workspace

# Selbstständigen In-Process-Cluster-Smoke-Test direkt starten
cargo run -p ash-test-cli -- --self-contained
```

---

## Horizontales Hinzuspiegeln eines neuen `game-node` (IRCd-Spiegelungsprinzip)

Um weitere Zonen im laufenden Cluster bereitzustellen, genügt es, in `docker-compose.yml` einen weiteren `game-node`-Block zu ergänzen. Das universelle `node.crt`/`node.key`-Zertifikat aus `pki-init` authentifiziert den Knoten per **mTLS über QUIC** automatisch am Hub (`game-node-roadhouse:5000`), woraufhin seine Zonen per Gossip im gesamten Mesh bekannt gemacht werden:

```yaml
  game-node-badlands:
    build:
      context: .
      target: game-node
    restart: unless-stopped
    depends_on:
      pki-init:
        condition: service_completed_successfully
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
      game-node-roadhouse:
        condition: service_started
    environment:
      NODE_ID: "game-node-badlands"
      NODE_ROLE: "mirror"
      HUB_ADDR: "game-node-roadhouse:5000"
      ZONES: "zone_andromeda_canyon,zone_alter_bunker_09"
      CLIENT_BIND_ADDR: "0.0.0.0:4436"
      CLIENT_PUBLIC_ADDR: "game-node-badlands:4436"
      MESH_BIND_ADDR: "0.0.0.0:5003"
      MESH_PUBLIC_ADDR: "game-node-badlands:5003"
      TICK_RATE_HZ: "25"
      TLS_CA_PATH: "/certs/ca.crt"
      TLS_CERT_PATH: "/certs/node.crt"
      TLS_KEY_PATH: "/certs/node.key"
      DATABASE_URL: "postgres://ash:ash_secret@postgres:5432/ashnet"
      REDIS_URL: "redis://redis:6379"
    volumes:
      - pki-certs:/certs:ro
    ports:
      - "4436:4436/udp"
    networks:
      - ash-mesh-net
      - data-net
```

Starten des neuen Spiegel-Knotens ohne Neustart der bestehenden Zonen:

```bash
docker compose up -d --no-deps --build game-node-badlands
```
