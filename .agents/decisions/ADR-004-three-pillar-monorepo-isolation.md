# ADR-004: 3-Säulen-Monorepo-Isolation (`SHARED` / `SERVER` / `CLIENT`)

- **Status:** Accepted (Sprint 1.5)
- **Kontext:** Im gemeinsamen Cargo-Workspace entwickeln unterschiedliche Sprints und KI-Agenten sowohl das autoritative Server-Mesh als auch Test- und spätere Ratatui-Clients. Ohne harte Schranken drohen schleichende Kopplungen (z. B. Client importiert `ash-db`/`sqlx` oder `ash-protocol` zieht Server-TLS/Async-Abhängigkeiten in jede Shared-Schicht).
- **Entscheidung:**
  1. **Deklarative Zonen-Pflicht (Säule A):** Jede Crate im Workspace deklariert `[package.metadata.ash] zone = "shared" | "server" | "client"`.
  2. **Server-Authority & Modul-Kapselung (Säule B):** Clients senden nur Intents und rendern bestätigte Server-Snapshots; einzige Brücken sind `ash-protocol` und `ircd`.
  3. **Maschinelles Gate (Säule C):** `./scripts/check-invariants.sh` analysiert `cargo metadata --format-version 1` über alle direkten und transitiven `normal`/`build`-Abhängigkeiten und bricht bei unklassifizierten Crates oder verbotenen Zonen-Kanten hart ab.
- **Konsequenzen:** Bereits in Sprint 1 entstandene Kopplungen (`ash-protocol -> tokio/quinn` sowie `ash-test-cli -> Server-Crates` für `--self-contained`) werden sofort transparent sichtbar gemacht und vor Sprint 2 gezielt aufgelöst, ohne Monorepo-Komfort einzubüßen.
