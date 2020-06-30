use s2n_quic_core::crypto::OneRTTCrypto;

negotiated_crypto!(RingOneRTTCrypto);

impl OneRTTCrypto for RingOneRTTCrypto {}
