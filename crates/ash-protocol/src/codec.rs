// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use serde::de::DeserializeOwned;
use serde::Serialize;

pub const ASH_ALPN: &[u8] = b"ash-quic-v1";
pub const MAX_FRAME_SIZE: usize = 4 * 1024 * 1024; // 4 MiB Schutzlimit

/// Serialisiert eine Nachricht in einen `bincode`-Payload und prüft das Größenlimit.
pub fn encode_frame_payload<T: Serialize>(msg: &T) -> Result<Vec<u8>> {
    let payload = bincode::serialize(msg).context("bincode serialize frame")?;
    if payload.len() > MAX_FRAME_SIZE {
        bail!("QUIC frame exceeds max size ({} bytes)", payload.len());
    }
    Ok(payload)
}

/// Dekodiert einen längenvalidierten `bincode`-Payload eines Reliable QUIC-Streams.
pub fn decode_frame_payload<T: DeserializeOwned>(buf: &[u8]) -> Result<T> {
    if buf.len() > MAX_FRAME_SIZE {
        bail!("incoming QUIC frame exceeds max size ({} bytes)", buf.len());
    }
    bincode::deserialize(buf).context("bincode deserialize frame")
}

/// Kodiert ein kompaktes Unreliable QUIC-Datagramm (für schnelle Bewegungs- und Positions-Updates).
pub fn encode_datagram<T: Serialize>(msg: &T) -> Result<Bytes> {
    let vec = bincode::serialize(msg).context("serialize QUIC datagram")?;
    Ok(Bytes::from(vec))
}

/// Dekodiert ein Unreliable QUIC-Datagramm.
pub fn decode_datagram<T: DeserializeOwned>(raw: &[u8]) -> Result<T> {
    bincode::deserialize(raw).context("deserialize QUIC datagram")
}
