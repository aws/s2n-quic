use ring::aead;
use s2n_quic_core::crypto::{
    retry::{IntegrityTag, NONCE_BYTES, SECRET_KEY_BYTES},
    CryptoError, RetryCrypto,
};

negotiated_crypto!(RingRetryCrypto);

lazy_static::lazy_static! {
/// Compute the Initial salt once, as the seed is constant
static ref SECRET_KEY: aead::LessSafeKey = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_128_GCM, &SECRET_KEY_BYTES).unwrap(),
    );
}

impl RetryCrypto for RingRetryCrypto {
    fn generate_tag(pseudo_packet: &[u8]) -> IntegrityTag {
        // TODO use the static nonce, no need to create this every time.
        let nonce: aead::Nonce = aead::Nonce::assume_unique_for_key(NONCE_BYTES);
        let tag = SECRET_KEY
            .seal_in_place_separate_tag(nonce, aead::Aad::from(pseudo_packet), &mut [])
            .unwrap();

        let mut integrity_tag = [0; 16];
        integrity_tag.copy_from_slice(tag.as_ref());
        integrity_tag
    }

    fn validate(pseudo_packet: &[u8], tag: IntegrityTag) -> Result<(), CryptoError> {
        let t = RingRetryCrypto::generate_tag(pseudo_packet);

        if ring::constant_time::verify_slices_are_equal(&t, &tag).is_ok() {
            return Ok(());
        }

        Err(CryptoError::DECRYPT_ERROR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn test_valid_tag() {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.4
        //# This shows a Retry packet that might be sent in response to the
        //# Initial packet in Appendix A.2.  The integrity check includes the
        //# client-chosen connection ID value of 0x8394c8f03e515708, but that
        //# value is not included in the final Retry packet:

        // Pseudo packet manually created based the retry packet and initial packet from Appendix A.2
        // TODO use the Retry / PseudoRetry encoder and decoder for this.
        let pseudo_packet = hex!("088394c8f03e515708 ffff000020 00 08f067a5502a4262b5 746f6b656e");

        let expected_tag = hex!("59756519dd6cc85bd90e33a934d2ff85");
        let invalid_tag = hex!("00112233445566778899aabbccddeeff");

        assert!(RingRetryCrypto::validate(&pseudo_packet, expected_tag).is_ok());
        assert!(RingRetryCrypto::validate(&pseudo_packet, invalid_tag).is_err());
    }
}
