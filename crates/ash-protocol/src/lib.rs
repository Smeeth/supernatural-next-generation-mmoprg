// SPDX-License-Identifier: GPL-3.0-or-later

pub mod client;
pub mod codec;
pub mod mesh;

/// Re-Exports für Server-Module, während Client-Crates gezielt `ash_protocol::client` nutzen.
pub mod messages {
    pub use crate::client::*;
    pub use crate::mesh::*;
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::client::{AuthRequest, ClientDatagram};
    use crate::codec::{
        decode_datagram, decode_frame_payload, encode_datagram, encode_frame_payload,
    };

    #[test]
    fn roundtrips_reliable_frame_and_unreliable_datagram() {
        let req = AuthRequest::Login {
            username: "dean".into(),
            password: "secret".into(),
        };
        let encoded = encode_frame_payload(&req).unwrap();
        let decoded: AuthRequest = decode_frame_payload(&encoded).unwrap();
        assert!(matches!(decoded, AuthRequest::Login { username, .. } if username == "dean"));

        let cid = Uuid::new_v4();
        let dgram = ClientDatagram::MoveInput {
            character_id: cid,
            seq: 42,
            pos_x: 12.5,
            pos_y: -4.0,
        };
        let raw = encode_datagram(&dgram).unwrap();
        let parsed: ClientDatagram = decode_datagram(&raw).unwrap();
        assert!(
            matches!(parsed, ClientDatagram::MoveInput { character_id, seq: 42, .. } if character_id == cid)
        );
    }
}
