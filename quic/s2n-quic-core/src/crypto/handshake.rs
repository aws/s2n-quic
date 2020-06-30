use crate::crypto::{HeaderCrypto, Key};

/// Types for which are able to perform handshake cryptography.
///
/// This marker trait ensures only Handshake-level keys
/// are used with Handshake packets. Any key misuses are
/// caught by the type system.
pub trait HandshakeCrypto: Key + HeaderCrypto {}
