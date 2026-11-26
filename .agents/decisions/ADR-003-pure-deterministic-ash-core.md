# ADR-003: Reines, deterministisch testbares `ash-core`

- **Status:** Accepted (Sprint 1 / verankert in Sprint 1.5)
- **Kontext:** Komplexe MMORPG-Regeln (48er Rassen-Klassen-Matrix, 16-Slot-Kofferraum-Prinzip, Tank-Aggro-Balance, 30s-Bann-Sheep, Pyromanten-Flächenbrand, Trickster-Aggro-Rückgabe, Boss-Phasen) sind fehleranfällig, wenn sie mit Netzwerk-, Async- oder Datenbank-Code vermischt werden.
- **Entscheidung:**
  - `ash-core` (`zone = "shared"`) bleibt vollständig frei von Async-Runtimes (`tokio`), Netzwerk (`quinn`), Datenbanken (`sqlx`, `redis`), Systemzeit (`SystemTime::now()`) und ungeseedetem Zufall (`thread_rng()`).
  - Zeitfortschritt (`dt_secs`) und Zufallsparameter werden explizit von außen übergeben.
- **Konsequenzen:** Die gesamte Domain- und Kampf-Logik kompiliert in Sekundenbruchteilen und wird in Millisekunden deterministisch per Unit-Test verifiziert; zugleich kann der spätere Client lesende Typen und UX-Vorprüfungen sicher aus `ash-core` nutzen, ohne Server-Infrastruktur hineinzuziehen.
