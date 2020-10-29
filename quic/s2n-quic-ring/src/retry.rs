use crate::ciphersuite::TLS_AES_128_GCM_SHA256 as Ciphersuite;
use s2n_quic_core::crypto::{CryptoError, HeaderCrypto, HeaderProtectionMask, Key, RetryCrypto};

#[derive(Debug)]
pub struct RingRetryCrypto {}

impl RingRetryCrypto {}

impl RetryCrypto for RingRetryCrypto {}
    fn calculate_tag() -> [u8; 16] {
        let packet_len = self.original_destination_connection_id.len()
            + self.destination_connection_id.len()
            + self.source_connection_id.len();

        let mut pseudo_packet_buffer = Vec::with_capacity(packet_len);
        let mut pseudo_packet = EncoderBuffer::new(&mut pseudo_packet_buffer);

        pseudo_packet.encode(self);

        let nonce = aead::Nonce::assume_unique_for_key(NONCE);
        let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_128_GCM, &SECRET_KEY).unwrap(),);
        let tag = key.seal_in_place_separate_tag(nonce, aead::Aad::from(pseudo_packet_buffer), &mut []).unwrap();

        let mut integrity_tag = [0; 16];
        integrity_tag.copy_from_slice(tag.as_ref());
        integrity_tag
    }

    fn is_valid() -> bool { false }
}
