// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::crypto;

header_key!(OneRttHeaderKey);
negotiated_crypto!(OneRttKey, OneRttHeaderKey);

impl crypto::OneRttKey for OneRttKey {
    #[inline]
    #[must_use]
    fn derive_next_key(&self) -> Self {
        Self(self.0.update())
    }

    #[inline]
    fn update_sealer_pmtu(&mut self, pmtu: u16) {
        self.0.sealer.update_pmtu(pmtu)
    }

    #[inline]
    fn update_opener_pmtu(&mut self, pmtu: u16) {
        self.0.opener.update_pmtu(pmtu)
    }
}

impl crypto::OneRttHeaderKey for OneRttHeaderKey {}

#[cfg(test)]
mod tests {
    use crate::cipher_suite::TLS_CHACHA20_POLY1305_SHA256;
    use hex_literal::hex;
    use ring::hkdf;
    use s2n_quic_core::crypto::Key;

    //= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.5
    //# In this example, TLS produces an application write secret from which
    //# a server uses HKDF-Expand-Label to produce four values: a key, an IV,
    //# a header protection key, and the secret that will be used after keys
    //# are updated (this last value is not used further in this example).
    //#
    //# secret
    //#     = 9ac312a7f877468ebe69422748ad00a1
    //#       5443f18203a07d6060f688f30f21632b
    //#
    //# key = HKDF-Expand-Label(secret, "quic key", "", 32)
    //#     = c6d98ff3441c3fe1b2182094f69caa2e
    //#       d4b716b65488960a7a984979fb23e1c8
    //#
    //# iv  = HKDF-Expand-Label(secret, "quic iv", "", 12)
    //#     = e0459b3474bdd0e44a41c144
    //#
    //# hp  = HKDF-Expand-Label(secret, "quic hp", "", 32)
    //#     = 25a282b9e82f06f21f488917a4fc8f1b
    //#       73573685608597d0efcb076b0ab7a7a4
    //#
    //# ku  = HKDF-Expand-Label(secret, "quic ku", "", 32)
    //#     = 1223504755036d556342ee9361d25342
    //#       1a826c9ecdf3c7148684b36b714881f9
    const SECRET: [u8; 32] =
        hex!("9ac312a7f877468ebe69422748ad00a15443f18203a07d6060f688f30f21632b");
    const KU_SECRET: [u8; 32] =
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
        let next_cipher = cipher.0.update();

        // Create a cipher based on the expected post-update secret
        let next_key = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, next_secret);
        let expected_next_cipher = TLS_CHACHA20_POLY1305_SHA256::new(next_key);

        (next_cipher, expected_next_cipher.0)
    }

    #[test]
    fn test_key_update() {
        let (next_cipher, expected_next_cipher) = generate_ciphers(&SECRET, &KU_SECRET);

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
        let (next_cipher, expected_next_cipher) = generate_ciphers(&INVALID_SECRET, &KU_SECRET);

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
