use crate::{
    crypto::{Key as KeyTrait, OneRTTCrypto, ProtectedPayload},
    packet::encoding::PacketEncodingError,
};
use s2n_codec::EncoderBuffer;

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
//# Endpoints MUST count the number of encrypted packets for each set of
//# keys.
pub struct Key<K> {
    key: K,
    // Keeping encrypted_packets out of the key allow keys to be immutable, which allows optimizations
    // later on.
    encrypted_packets: u64,
}

impl<K: KeyTrait> Key<K>
where
    K: OneRTTCrypto,
{
    pub fn new(key: K) -> Self {
        Key {
            key,
            encrypted_packets: 0,
        }
    }

    pub fn derive_next_key(&self) -> K {
        self.key.derive_next_key()
    }

    pub fn encrypted_packets(&self) -> u64 {
        self.encrypted_packets
    }

    pub fn key(&self) -> &K {
        &self.key
    }

    pub fn encode_packet<'a, F>(
        &mut self,
        buffer: EncoderBuffer<'a>,
        f: F,
    ) -> Result<(ProtectedPayload<'a>, EncoderBuffer<'a>), PacketEncodingError<'a>>
    where
        F: FnOnce(
            EncoderBuffer<'a>,
            &K,
        )
            -> Result<(ProtectedPayload<'a>, EncoderBuffer<'a>), PacketEncodingError<'a>>,
    {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
        //# If the total number of encrypted packets with the same key
        //# exceeds the confidentiality limit for the selected AEAD, the endpoint
        //# MUST stop using those keys.
        if self.encrypted_packets > self.key.aead_confidentiality_limit() {
            return Err(PacketEncodingError::AeadLimitReached(buffer));
        }

        let r = f(buffer, &self.key)?;

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
        //# Endpoints MUST count the number of encrypted packets for each set of
        //# keys.
        self.encrypted_packets += 1;
        Ok(r)
    }
}
