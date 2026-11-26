# ADR-001: GPL-3.0-or-later Lizenzierung & Prozess-Isolation des IRCd

- **Status:** Accepted (Sprint 1 / verankert in Sprint 1.5)
- **Kontext:** *Ash-Net: New Lawrence* steht vollständig unter `GPL-3.0-or-later`, nutzt ein modernes Rust-Ökosystem (überwiegend MIT/Apache-2.0) und benötigt einen vollwertigen, kampferprobten IRC-Server (`ngIRCd` / `InspIRCd`, GPLv2+).
- **Entscheidung:**
  1. Jede eigene Quelldatei trägt zwingend `SPDX-License-Identifier: GPL-3.0-or-later`.
  2. Alle Rust-Crate-Abhängigkeiten werden über `deny.toml` auf GPL-3.0-or-later-kompatible Lizenzen beschränkt.
  3. Der `ircd` (`ngIRCd`) läuft **ausschließlich** als eigenständiger Betriebssystem-Prozess in einem eigenen Docker-Container (`ircd` im Netzwerk `irc-net`) und wird über reine RFC-1459-TCP-Sockets durch `ash-irc-bridge` angesprochen — ohne jegliche Code- oder Bibliotheks-Verlinkung.
- **Konsequenzen:** Vollständige lizenzrechtliche Sauberkeit, klare Fehlertoleranz-Grenze (ein Neustart eines Game-Nodes unterbricht niemals den laufenden Kolonie-Funk im IRC) und einfache Austauschbarkeit der IRCd-Implementierung.
