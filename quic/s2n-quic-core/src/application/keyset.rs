use crate::{
    connection::ProcessingError,
    crypto::{CryptoError, Key, LimitedUseCrypto, OneRTTCrypto},
    packet::KeyPhase,
    time::{Timer, Timestamp},
    transport::error::TransportError,
};

pub struct KeySet<Key> {
    /// The current [`KeyPhase`]
    key_phase: KeyPhase,

    key_derivation_timer: Timer,

    packet_decryption_failures: u64,
    aead_integrity_limit: u64,

    /// Set of keys for the current and next phase
    crypto: [LimitedUseCrypto<Key>; 2],
}

impl<K: Key> KeySet<K>
where
    K: OneRTTCrypto,
{
    pub fn new(crypto: K) -> Self {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6
        //# The Key Phase bit is initially set to 0 for the
        //# first set of 1-RTT packets and toggled to signal each subsequent key
        //# update.

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.3
        //# Endpoints responding to an apparent key update MUST NOT generate a
        //# timing side-channel signal that might indicate that the Key Phase bit
        //# was invalid (see Section 9.4).

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.4
        //# The same header protection key is used for the duration of the
        //# connection, with the value not changing after a key update (see
        //# Section 6).  This allows header protection to be used to protect the
        //# key phase.
        // By pre-generating the next key, we can respond to a KeyUpdate without exposing a timing
        // side channel.
        let aead_integrity_limit = crypto.aead_integrity_limit();
        let next_key = LimitedUseCrypto::new(crypto.derive_next_key());
        let active_key = LimitedUseCrypto::new(crypto);
        Self {
            key_phase: KeyPhase::Zero,
            key_derivation_timer: Default::default(),
            packet_decryption_failures: 0,
            aead_integrity_limit,
            crypto: [active_key, next_key],
        }
    }

    /// Rotating the phase will switch the active key
    pub fn rotate_phase(&mut self) {
        self.key_phase = KeyPhase::next_phase(self.key_phase)
    }

    /// Derive a new key based on the active key, and store it in the non-active slot
    pub fn derive_and_store_next_key(&mut self) {
        let next_key = self.active_key().derive_next_key();
        let next_phase = KeyPhase::next_phase(self.key_phase);
        self.crypto[next_phase as usize] = LimitedUseCrypto::new(next_key);
    }

    fn aead_integrity_limit(&self) -> u64 {
        self.aead_integrity_limit
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.key_derivation_timer.iter()
    }

    pub fn set_timer(&mut self, timestamp: Timestamp) {
        self.key_derivation_timer.set(timestamp)
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        // key_derivation_timer
        if self
            .key_derivation_timer
            .poll_expiration(timestamp)
            .is_ready()
        {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.5
            //# An endpoint SHOULD retain old read keys for no more than three times
            //# the PTO after having received a packet protected using the new keys.
            self.derive_and_store_next_key();
        }
    }

    pub fn key_update_in_progress(&self) -> bool {
        self.key_derivation_timer.is_armed()
    }

    pub fn key_phase(&self) -> KeyPhase {
        self.key_phase
    }

    pub fn active_key(&mut self) -> &LimitedUseCrypto<K> {
        self.key_for_phase(self.key_phase)
    }

    pub fn active_key_mut(&mut self) -> &mut LimitedUseCrypto<K> {
        self.key_for_phase_mut(self.key_phase)
    }

    pub fn key_for_phase(&self, key_phase: KeyPhase) -> &LimitedUseCrypto<K> {
        &self.crypto[(key_phase as u8) as usize]
    }

    pub fn key_for_phase_mut(&mut self, key_phase: KeyPhase) -> &mut LimitedUseCrypto<K> {
        &mut self.crypto[(key_phase as u8) as usize]
    }

    fn on_decryption_error(&mut self) {
        self.packet_decryption_failures += 1
    }

    pub fn decryption_error_count(&self) -> u64 {
        self.packet_decryption_failures
    }

    pub fn decrypt_packet<F, R>(&mut self, phase: KeyPhase, f: F) -> Result<R, ProcessingError>
    where
        K: OneRTTCrypto,
        F: FnOnce(&K) -> Result<R, CryptoError>,
    {
        match self
            .key_for_phase(phase)
            .unprotect_packet(|key| -> Result<R, CryptoError> { f(key) })
        {
            Ok(r) => Ok(r),
            Err(e) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
                //# In addition to counting packets sent, endpoints MUST count the number
                //# of received packets that fail authentication during the lifetime of a
                //# connection.
                self.on_decryption_error();

                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
                //# If the total number of received packets that fail
                //# authentication within the connection, across all keys, exceeds the
                //# integrity limit for the selected AEAD, the endpoint MUST immediately
                //# close the connection with a connection error of type
                //# AEAD_LIMIT_REACHED and not process any more packets.
                if self.decryption_error_count() > self.aead_integrity_limit() {
                    return Err(ProcessingError::TransportError(
                        TransportError::AEAD_LIMIT_REACHED,
                    ));
                }

                Err(ProcessingError::CryptoError(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crypto::{
            header_crypto::{HeaderCrypto, HeaderProtectionMask},
            CryptoError, Key, OneRTTCrypto, ProtectedPayload,
        },
        packet::{encoding::PacketEncodingError, KeyPhase},
    };
    use s2n_codec::EncoderBuffer;

    #[derive(Default)]
    struct NullKey {
        pub value: u64,
        pub integrity_limit: u64,
    }

    impl NullKey {
        fn new(value: u64, integrity_limit: u64) -> Self {
            Self {
                value,
                integrity_limit,
            }
        }
    }

    impl Key for NullKey {
        fn decrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), CryptoError> {
            Ok(())
        }

        fn encrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), CryptoError> {
            Ok(())
        }

        fn tag_len(&self) -> usize {
            0
        }

        fn aead_confidentiality_limit(&self) -> u64 {
            0
        }

        fn aead_integrity_limit(&self) -> u64 {
            self.integrity_limit
        }
    }

    impl HeaderCrypto for NullKey {
        fn opening_header_protection_mask(&self, _sample: &[u8]) -> HeaderProtectionMask {
            [0; 5]
        }

        fn opening_sample_len(&self) -> usize {
            0
        }

        fn sealing_header_protection_mask(&self, _sample: &[u8]) -> HeaderProtectionMask {
            [0; 5]
        }

        fn sealing_sample_len(&self) -> usize {
            0
        }
    }

    impl OneRTTCrypto for NullKey {
        fn derive_next_key(&self) -> Self {
            Self {
                value: self.value + 1,
                integrity_limit: self.integrity_limit,
            }
        }
    }

    #[test]
    fn test_key_set() {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.3
        //= type=test
        //# For this reason, endpoints MUST be able to retain two sets of packet
        //# protection keys for receiving packets: the current and the next.

        let mut keyset = KeySet::new(NullKey::default());
        let mut phase_zero_value = 0;
        let mut phase_one_value = 0;

        assert!(keyset
            .decrypt_packet(KeyPhase::Zero, |key| -> Result<(), CryptoError> {
                phase_zero_value = key.value;
                Ok(())
            })
            .is_ok());
        assert!(keyset
            .decrypt_packet(KeyPhase::One, |key| -> Result<(), CryptoError> {
                phase_one_value = key.value;
                Ok(())
            })
            .is_ok());

        assert_eq!(phase_zero_value, 0);
        assert_eq!(phase_one_value, 1);
    }

    #[test]
    fn test_phase_rotation() {
        let mut keyset = KeySet::new(NullKey::default());
        let mut phase_zero_value = 0;
        let mut phase_one_value = 0;

        keyset.rotate_phase();
        assert!(keyset
            .decrypt_packet(keyset.key_phase(), |key| -> Result<(), CryptoError> {
                phase_one_value = key.value;
                Ok(())
            })
            .is_ok());

        keyset.rotate_phase();
        assert!(keyset
            .decrypt_packet(keyset.key_phase(), |key| -> Result<(), CryptoError> {
                phase_zero_value = key.value;
                Ok(())
            })
            .is_ok());

        assert_eq!(phase_zero_value, 0);
        assert_eq!(phase_one_value, 1);
    }

    #[test]
    fn test_key_derivation() {
        let mut keyset = KeySet::new(NullKey::default());

        keyset.rotate_phase();
        keyset.derive_and_store_next_key();
        keyset.rotate_phase();

        let mut key_value = 0;
        assert!(keyset
            .decrypt_packet(keyset.key_phase(), |key| -> Result<(), CryptoError> {
                key_value = key.value;
                Ok(())
            })
            .is_ok());
        assert_eq!(key_value, 2);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //= type=test
    //# In addition to counting packets sent, endpoints MUST count the number
    //# of received packets that fail authentication during the lifetime of a
    //# connection.
    #[test]
    fn test_decryption_failure_counter() {
        let key = NullKey::new(0, 1);
        let mut keyset = KeySet::new(key);

        assert_eq!(keyset.decryption_error_count(), 0);
        assert!(matches!(
            keyset.decrypt_packet(keyset.key_phase(), |_key| -> Result<(), CryptoError> {
                Err(CryptoError::DECRYPT_ERROR)
            }),
            Err(ProcessingError::CryptoError(_))
        ));
        assert_eq!(keyset.decryption_error_count(), 1);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //= type=test
    //# If the total number of received packets that fail
    //# authentication within the connection, across all keys, exceeds the
    //# integrity limit for the selected AEAD, the endpoint MUST immediately
    //# close the connection with a connection error of type
    //# AEAD_LIMIT_REACHED and not process any more packets.
    #[test]
    fn test_decryption_failure_enforced_aead_limit() {
        let key = NullKey::new(0, 0);
        let mut keyset = KeySet::new(key);

        assert_eq!(keyset.decryption_error_count(), 0);
        assert!(matches!(
            keyset.decrypt_packet(keyset.key_phase(), |_key| -> Result<(), CryptoError> {
                Err(CryptoError::DECRYPT_ERROR)
            }),
            Err(ProcessingError::TransportError(
                TransportError::AEAD_LIMIT_REACHED
            ))
        ));
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //= type=test
    //# Endpoints MUST count the number of encrypted packets for each set of
    //# keys.
    #[test]
    fn test_encrypted_packet_count_increased() {
        let key = NullKey::new(0, 0);
        let mut crypto = LimitedUseCrypto::new(key);
        let mut encoder_bytes = [0; 512];
        let buffer = EncoderBuffer::new(&mut encoder_bytes);
        let mut decoder_bytes = [0; 512];

        assert_eq!(crypto.encrypted_packets, 0);
        assert!(crypto
            .encode_packet(buffer, |buffer, _key| {
                let payload = ProtectedPayload::new(0, &mut decoder_bytes);

                Ok((payload, buffer))
            })
            .is_ok());

        assert_eq!(crypto.encrypted_packets, 1);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //= type=test
    //# If the total number of encrypted packets with the same key
    //# exceeds the confidentiality limit for the selected AEAD, the endpoint
    //# MUST stop using those keys.
    #[test]
    fn test_encrypted_packet_count_enforced_aead_limit() {
        let key = NullKey::default();
        let mut crypto = LimitedUseCrypto::new(key);
        let mut encoder_bytes = [0; 512];
        let buffer = EncoderBuffer::new(&mut encoder_bytes);
        let mut decoder_bytes = [0; 512];

        crypto.encrypted_packets = 1;
        assert!(matches!(
            crypto.encode_packet(buffer, |buffer, _key| {
                let payload = ProtectedPayload::new(0, &mut decoder_bytes);

                Ok((payload, buffer))
            }),
            Err(PacketEncodingError::AeadLimitReached(_))
        ));
        assert_eq!(crypto.encrypted_packets, 1);
    }
}
