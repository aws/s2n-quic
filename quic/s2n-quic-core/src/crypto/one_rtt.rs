use crate::crypto::{HeaderCrypto, Key};

/// Types for which are able to perform 1-RTT cryptography.
///
/// This trait ensures only 1-RTT-level keys
/// are used with Short packets. Any key misuses are
/// caught by the type system.
pub trait OneRTTCrypto: Key + HeaderCrypto {
    fn derive_next_key(&self) -> Self;
}
