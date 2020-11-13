use core::convert::TryInto;
use ring::aead;
use s2n_quic_core::crypto::{
    retry::{IntegrityTag, NONCE_BYTES, SECRET_KEY_BYTES},
    CryptoError, RetryCrypto,
};

lazy_static::lazy_static! {
    /// Compute the Initial salt once, as the seed is constant
    static ref SECRET_KEY: aead::LessSafeKey = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_128_GCM, &SECRET_KEY_BYTES).unwrap(),
    );
}

#[derive(Debug)]
pub struct RingRetryCrypto;

impl RetryCrypto for RingRetryCrypto {
    fn generate_tag(pseudo_packet: &[u8]) -> IntegrityTag {
        let nonce = aead::Nonce::assume_unique_for_key(NONCE_BYTES);
        let tag = SECRET_KEY
            .seal_in_place_separate_tag(nonce, aead::Aad::from(pseudo_packet), &mut [])
            .expect("in_out len is 0 and should always be less than the nonce max bytes");

        tag.as_ref()
            .try_into()
            .expect("AES_128_GCM tag len should always be 128 bits")
    }

    fn validate(pseudo_packet: &[u8], tag: IntegrityTag) -> Result<(), CryptoError> {
        let expected = RingRetryCrypto::generate_tag(pseudo_packet);

        ring::constant_time::verify_slices_are_equal(&expected, &tag)
            .map_err(|_| CryptoError::DECRYPT_ERROR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;
    use s2n_quic_core::crypto::retry::{EXAMPLE_EXPECTED_TAG, EXAMPLE_PSEUDO_RETRY_PACKET};

    #[test]
    fn test_valid_tag() {
        let invalid_tag: [u8; 16] = hex!("00112233445566778899aabbccddeeff");

        assert!(
            RingRetryCrypto::validate(&EXAMPLE_PSEUDO_RETRY_PACKET, EXAMPLE_EXPECTED_TAG).is_ok()
        );
        assert!(RingRetryCrypto::validate(&EXAMPLE_PSEUDO_RETRY_PACKET, invalid_tag).is_err());
    }
}
