use s2n_quic_core::crypto::OneRTTCrypto;

negotiated_crypto!(RingOneRTTCrypto);

impl OneRTTCrypto for RingOneRTTCrypto {
    fn derive_next_key(&self) -> Self {
        Self(self.0.update())
    }
}

#[cfg(test)]
mod tests {
    use crate::ciphersuite::TLS_CHACHA20_POLY1305_SHA256;
    use hex_literal::hex;
    use ring::hkdf;
    use s2n_quic_core::crypto::Key;

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.5
    //# In this example, TLS produces an application write secret from which
    //# a server uses HKDF-Expand-Label to produce four values: a key, an IV,
    //# a header protection key, and the secret that will be used after keys
    //# are updated (this last value is not used further in this example).
    const SECRET: [u8; 32] =
        hex!("9ac312a7f877468ebe69422748ad00a15443f18203a07d6060f688f30f21632b");
    const UPDATED_SECRET: [u8; 32] =
        hex!("1223504755036d556342ee9361d253421a826c9ecdf3c7148684b36b714881f9");

    // Prevent trivial success
    const INVALID_SECRET: [u8; 32] =
        hex!("0000000000000000000000000000000000000000000000000000000000000000");

    /// Return ChaCha20 ciphers because these are the ciphers given in the RFC. The other cipher
    /// implementations don't have RFC values we can test.
    /// This is not exhaustive, but it does show that we are using the KDF and label correctly.
    fn generate_ciphers(
        secret: &[u8],
        next_secret: &[u8],
    ) -> (TLS_CHACHA20_POLY1305_SHA256, TLS_CHACHA20_POLY1305_SHA256) {
        // Create a cipher based on the initial secret
        let key = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, secret);
        let cipher = TLS_CHACHA20_POLY1305_SHA256::new(key);

        // Create the cipher after a Key Update has occurred
        let next_cipher = cipher.update();

        // Create a cipher based on the expected post-update secret
        let next_key = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, next_secret);
        let expected_next_cipher = TLS_CHACHA20_POLY1305_SHA256::new(next_key);

        (next_cipher, expected_next_cipher)
    }

    #[test]
    fn test_key_update() {
        let (next_cipher, expected_next_cipher) = generate_ciphers(&SECRET, &UPDATED_SECRET);

        // Encrypt two empty blocks to verify the ciphers are the same
        let mut next_cipher_output = [0; 32];
        let mut expected_cipher_output = [0; 32];
        next_cipher
            .encrypt(0, &[], &mut next_cipher_output[..])
            .unwrap();
        expected_next_cipher
            .encrypt(0, &[], &mut expected_cipher_output[..])
            .unwrap();

        assert_eq!(next_cipher_output, expected_cipher_output);
    }

    #[test]
    fn test_key_update_failure() {
        let (next_cipher, expected_next_cipher) =
            generate_ciphers(&INVALID_SECRET, &UPDATED_SECRET);

        // Encrypt two empty blocks to verify the ciphers are the same
        let mut next_cipher_output = [0; 32];
        let mut expected_cipher_output = [0; 32];
        next_cipher
            .encrypt(0, &[], &mut next_cipher_output[..])
            .unwrap();
        expected_next_cipher
            .encrypt(0, &[], &mut expected_cipher_output[..])
            .unwrap();

        assert!(next_cipher_output != expected_cipher_output);
    }
}
