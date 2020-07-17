//= https://tools.ietf.org/id/draft-ietf-quic-tls-22.txt#5.1
//# QUIC derives packet protection keys in the same way that TLS derives
//# record protection keys.
//#
//# Each encryption level has separate secret values for protection of
//# packets sent in each direction.  These traffic secrets are derived by
//# TLS (see Section 7.1 of [TLS13]) and are used by QUIC for all
//# encryption levels except the Initial encryption level.  The secrets
//# for the Initial encryption level are computed based on the client's
//# initial Destination Connection ID, as described in Section 5.2.
//#
//# The keys used for packet protection are computed from the TLS secrets
//# using the KDF provided by TLS.  In TLS 1.3, the HKDF-Expand-Label
//# function described in Section 7.1 of [TLS13] is used, using the hash
//# function from the negotiated cipher suite.  Other versions of TLS
//# MUST provide a similar function in order to be used with QUIC.
//#
//# The current encryption level secret and the label "quic key" are
//# input to the KDF to produce the AEAD key; the label "quic iv" is used
//# to derive the IV; see Section 5.3.  The header protection key uses
//# the "quic hp" label; see Section 5.4.  Using these labels provides
//# key separation between QUIC and TLS; see Section 9.4.

pub const QUIC_KEY_LABEL: [u8; 8] = *b"quic key";
pub const QUIC_IV_LABEL: [u8; 7] = *b"quic iv";
pub const QUIC_HP_LABEL: [u8; 7] = *b"quic hp";

//# The KDF used for initial secrets is always the HKDF-Expand-Label
//# function from TLS 1.3 (see Section 5.2).
