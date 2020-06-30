use s2n_quic_core::crypto::HandshakeCrypto;

negotiated_crypto!(RingHandshakeCrypto);

impl HandshakeCrypto for RingHandshakeCrypto {}
