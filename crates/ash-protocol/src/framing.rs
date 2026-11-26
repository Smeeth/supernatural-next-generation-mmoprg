// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use quinn::{Connection, RecvStream, SendStream};
use serde::de::DeserializeOwned;
use serde::Serialize;

pub const MAX_FRAME_SIZE: usize = 4 * 1024 * 1024; // 4 MiB Schutzlimit

/// Schreibt einen längenpräfixierten binären Frame (`u32` BE + `bincode`) auf einen Reliable QUIC-Stream.
pub async fn write_frame<T: Serialize>(send: &mut SendStream, msg: &T) -> Result<()> {
    let payload = bincode::serialize(msg).context("bincode serialize frame")?;
    if payload.len() > MAX_FRAME_SIZE {
        bail!("QUIC frame exceeds max size ({} bytes)", payload.len());
    }
    let len_bytes = (payload.len() as u32).to_be_bytes();
    send.write_all(&len_bytes).await.context("write frame length")?;
    send.write_all(&payload).await.context("write frame payload")?;
    Ok(())
}

/// Liest einen längenpräfixierten binären Frame von einem Reliable QUIC-Stream.
pub async fn read_frame<T: DeserializeOwned>(recv: &mut RecvStream) -> Result<T> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .context("read frame length")?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_SIZE {
        bail!("incoming QUIC frame exceeds max size ({len} bytes)");
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .context("read frame payload")?;
    bincode::deserialize(&buf).context("bincode deserialize frame")
}

/// Führt einen atomaren Request/Response-Austausch über einen eigenen bidirektionalen QUIC-Stream aus
/// (verhindert Head-of-Line-Blocking zwischen unabhängigen Gameplay- und Mesh-Operationen).
pub async fn rpc_call<Req: Serialize, Resp: DeserializeOwned>(
    conn: &Connection,
    req: &Req,
) -> Result<Resp> {
    let (mut send, mut recv) = conn.open_bi().await.context("open QUIC bi-stream")?;
    write_frame(&mut send, req).await?;
    send.finish().context("finish QUIC request stream")?;
    read_frame(&mut recv).await
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
