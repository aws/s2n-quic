//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.1
//# The current encryption level secret and the label "quic key" are
//# input to the KDF to produce the AEAD key;
pub const QUIC_KEY_LABEL: [u8; 8] = *b"quic key";

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.1
//# the label "quic iv" is used
//# to derive the Initialization Vector (IV); see Section 5.3.
pub const QUIC_IV_LABEL: [u8; 7] = *b"quic iv";

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.1
//# The header protection key uses the "quic hp" label; see Section 5.4.
pub const QUIC_HP_LABEL: [u8; 7] = *b"quic hp";
