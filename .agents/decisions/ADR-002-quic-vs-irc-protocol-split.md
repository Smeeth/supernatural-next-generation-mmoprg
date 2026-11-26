# ADR-002: Strikte Protokoll-Trennung zwischen Binär-QUIC und Text-IRC

- **Status:** Accepted (Sprint 1 / verankert in Sprint 1.5)
- **Kontext:** Ein Terminal-MMORPG benötigt gleichzeitig latenzarme Echtzeit-Bewegung ohne Head-of-Line-Blocking, verlustfreie Transaktionsströme (Kampf, Inventar, Zonen-Handover) und ein authentisches Terminal-Funknetz für soziale Interaktion.
- **Entscheidung:**
  1. **Game-State & Server-Mesh:** 100 % binär (`serde` + `bincode`) über **QUIC (`quinn`, TLS 1.3; mTLS im Server-Mesh)**:
     - *Unreliable Datagrams* für hochfrequente Bewegungs-Inputs und 25-Hz-Zonen-Snapshots.
     - *Reliable Bi-Streams* für Login, Quests, 16-Slot-Ausrüstung, Kampf-Aktionen und `PLAYER_HANDOVER`.
  2. **Soziale Kommunikation & RP-Kommandos:** 100 % nativ über **IRC**. `ash-irc-bridge` übersetzt In-Game-Slash-Befehle (`/salzen`, `/emf`, `/exorzieren`, `/tacker`, `/winken`, `/grüßen`, `/prost`) per mTLS-QUIC in ECS-Events und spiegelt Omen-Alarme ab Stufe 2 automatisch nach `#new_lawrence`.
- **Konsequenzen:** Kein HTTP/REST/WebSocket-Mischmasch; Lore („Henochisches Ash-Net-Terminal & Kanal-4-Funkrelais“) und Netzwerkarchitektur decken sich exakt.
