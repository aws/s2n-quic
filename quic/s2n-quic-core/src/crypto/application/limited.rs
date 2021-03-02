use crate::crypto::{Key as KeyTrait, OneRTTCrypto};

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
//# Endpoints MUST count the number of encrypted packets for each set of
//# keys.
pub struct Key<K> {
    key: K,

    // Keeping encrypted_packets out of the key allow keys to be immutable, which allows optimizations
    // later on.
    encrypted_packets: u64,
    confidentiality_limit: u64,
}

// TODO: This should be configured through the limit provider.
const KEY_UPDATE_WINDOW: u64 = 10_000;

impl<K: KeyTrait> Key<K>
where
    K: OneRTTCrypto,
{
    pub fn new(key: K) -> Self {
        Key {
            // TODO: This should be configured through the limit provider, the default being the
            // key's AEAD limit.
            confidentiality_limit: key.aead_confidentiality_limit(),
            key,
            encrypted_packets: 0,
        }
    }

    /// If the key has been used passed the confidentiality_limit, it is expired and may not be
    /// used. We check >= because we don't want to encrypt an additional packet if the key has
    /// already been used up to the limit.
    #[inline]
    pub fn expired(&self) -> bool {
        self.encrypted_packets >= self.confidentiality_limit
    }

    /// If the key is within the update window, an update should be initiated.
    #[inline]
    pub fn needs_update(&self) -> bool {
        self.encrypted_packets > (self.confidentiality_limit - KEY_UPDATE_WINDOW)
    }

    pub fn derive_next_key(&self) -> K {
        self.key.derive_next_key()
    }

    #[inline]
    pub fn encrypted_packets(&self) -> u64 {
        self.encrypted_packets
    }

    #[inline]
    pub fn inc_packet_count(&mut self) {
        self.encrypted_packets += 1
    }

    #[inline]
    pub fn key(&self) -> &K {
        &self.key
    }
}
