use s2n_quic_core::crypto::OneRTTCrypto;

negotiated_crypto!(RingOneRTTCrypto);

impl OneRTTCrypto for RingOneRTTCrypto {
    fn derive_next_key(&self) -> Self {
        Self(self.0.update())
    }
}
