use crate::crypto::{HeaderCrypto, Key};

/// Types for which are able to perform 0-RTT cryptography.
///
/// This marker trait ensures only 0-RTT-level keys
/// are used with ZeroRTT packets. Any key misuses are
/// caught by the type system.
pub trait ZeroRTTCrypto: Key + HeaderCrypto {}

/// ZeroRTT Secret tokens are always 32 bytes
pub type ZeroRTTSecret = [u8; 32];
