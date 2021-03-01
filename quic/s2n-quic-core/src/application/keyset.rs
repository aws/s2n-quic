use crate::{
    connection::ProcessingError,
    crypto::{CryptoError, Key, LimitedUseCrypto, OneRTTCrypto},
    packet::{
        number::PacketNumber,
        short::{CleartextShort, EncryptedShort, ProtectedShort},
        KeyPhase,
    },
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

    /// Set the timer to derive a new key after timestamp
    pub fn set_derivation_timer(&mut self, timestamp: Timestamp) {
        self.key_derivation_timer.set(timestamp)
    }

    /// Returns whether there is a key update in progress.
    pub fn key_update_in_progress(&self) -> bool {
        self.key_derivation_timer.is_armed()
    }

    pub fn remove_header_protection<'a>(
        &self,
        protected: ProtectedShort<'a>,
        largest_acknowledged_packet_number: PacketNumber,
    ) -> Result<EncryptedShort<'a>, CryptoError> {
        protected.unprotect(
            self.key_for_phase(KeyPhase::Zero).key(),
            largest_acknowledged_packet_number,
        )
    }

    /// Passes the key for the the requested phase to a callback function. Integrity limits are
    /// enforced.
    pub fn decrypt_packet<'a>(
        &mut self,
        packet: EncryptedShort<'a>,
        largest_acknowledged_packet_number: PacketNumber,
        pto: Timestamp,
    ) -> Result<CleartextShort<'a>, ProcessingError> {
        let mut phase_to_use = self.key_phase() as u8;
        let packet_phase = packet.key_phase();
        let phase_switch = phase_to_use != (packet_phase as u8);
        phase_to_use ^= phase_switch as u8;

        if self.key_update_in_progress() && phase_switch {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.5
            //# An endpoint MAY allow a period of approximately the Probe Timeout
            //# (PTO; see [QUIC-RECOVERY]) after receiving a packet that uses the new
            //# key generation before it creates the next set of packet protection
            //# keys.

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.4
            //= type=TODO
            //= tracking-issue=479
            //= feature=Key update
            //# Packets with higher packet numbers MUST be protected with either the
            //# same or newer packet protection keys than packets with lower packet
            //# numbers.
            // During this PTO we can still process delayed packets, reducing retransmits
            // required from the peer. We know the packets are delayed because they have a
            // lower packet number than expected and the old key phase.
            if packet.packet_number < largest_acknowledged_packet_number {
                phase_to_use = packet.key_phase() as u8;
            }
        }

        match packet.decrypt(self.key_for_phase(phase_to_use.into()).key()) {
            Ok(packet) => {
                if packet_phase != self.key_phase() {
                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.2
                    //# Sending keys MUST be updated before sending an
                    //# acknowledgement for the packet that was received with updated keys.

                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.2
                    //# The endpoint MUST update its
                    //# send keys to the corresponding key phase in response, as described in
                    //# Section 6.1.
                    self.rotate_phase();

                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.3
                    //# Endpoints responding to an apparent key update MUST NOT generate a
                    //# timing side-channel signal that might indicate that the Key Phase bit
                    //# was invalid (see Section 9.4).

                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.5
                    //# An endpoint MAY allow a period of approximately the Probe Timeout
                    //# (PTO; see [QUIC-RECOVERY]) after receiving a packet that uses the new
                    //# key generation before it creates the next set of packet protection
                    //# keys.
                    self.set_derivation_timer(pto);
                }

                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.4
                //= type=TODO
                //= tracking-issue=479
                //= feature=Key update
                //# An endpoint that successfully removes protection with old
                //# keys when newer keys were used for packets with lower packet numbers
                //# MUST treat this as a connection error of type KEY_UPDATE_ERROR.
                Ok(packet)
            }
            Err(e) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
                //# In addition to counting packets sent, endpoints MUST count the number
                //# of received packets that fail authentication during the lifetime of a
                //# connection.
                self.packet_decryption_failures += 1;

                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
                //# If the total number of received packets that fail
                //# authentication within the connection, across all keys, exceeds the
                //# integrity limit for the selected AEAD, the endpoint MUST immediately
                //# close the connection with a connection error of type
                //# AEAD_LIMIT_REACHED and not process any more packets.
                if self.decryption_error_count() > self.aead_integrity_limit {
                    return Err(ProcessingError::TransportError(
                        TransportError::AEAD_LIMIT_REACHED,
                    ));
                }
                Err(ProcessingError::CryptoError(e))
            }
        }
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.key_derivation_timer.iter()
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
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

    pub fn key_phase(&self) -> KeyPhase {
        self.key_phase
    }

    pub fn active_key(&mut self) -> &LimitedUseCrypto<K> {
        self.key_for_phase(self.key_phase)
    }

    pub fn active_key_mut(&mut self) -> &mut LimitedUseCrypto<K> {
        self.key_for_phase_mut(self.key_phase)
    }

    /// NOTE: Only public so I can test from transport, which I do so I can access a clock
    pub fn key_for_phase(&self, key_phase: KeyPhase) -> &LimitedUseCrypto<K> {
        &self.crypto[(key_phase as u8) as usize]
    }

    fn key_for_phase_mut(&mut self, key_phase: KeyPhase) -> &mut LimitedUseCrypto<K> {
        &mut self.crypto[(key_phase as u8) as usize]
    }

    fn decryption_error_count(&self) -> u64 {
        self.packet_decryption_failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        connection::id::ConnectionInfo,
        crypto::{
            testing::{FailingKey, Key as TestKey},
            ProtectedPayload,
        },
        inet::SocketAddress,
        packet::{encoding::PacketEncodingError, number::PacketNumberSpace, KeyPhase},
        time::{Clock, NoopClock},
        varint::VarInt,
    };
    use s2n_codec::{DecoderBufferMut, EncoderBuffer};

    #[test]
    fn test_key_set() {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.3
        //= type=test
        //# For this reason, endpoints MUST be able to retain two sets of packet
        //# protection keys for receiving packets: the current and the next.

        let keyset = KeySet::new(TestKey::default());

        assert_eq!(keyset.key_for_phase(KeyPhase::Zero).key().value, 0);
        assert_eq!(keyset.key_for_phase(KeyPhase::One).key().value, 1);
    }

    #[test]
    fn test_phase_rotation() {
        let mut keyset = KeySet::new(TestKey::default());

        assert_eq!(keyset.active_key().key().value, 0);
        keyset.rotate_phase();
        assert_eq!(keyset.active_key().key().value, 1);
    }

    #[test]
    fn test_key_derivation() {
        let mut keyset = KeySet::new(TestKey::default());

        keyset.rotate_phase();
        keyset.derive_and_store_next_key();
        keyset.rotate_phase();
        assert_eq!(keyset.active_key().key().value, 2);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //= type=test
    //# In addition to counting packets sent, endpoints MUST count the number
    //# of received packets that fail authentication during the lifetime of a
    //# connection.
    #[test]
    fn test_decryption_failure_counter() {
        let clock = NoopClock {};
        let key = FailingKey::new(0, 1, 1);
        let mut keyset = KeySet::new(key);
        let mut data = [0; 128];
        let remote_address = SocketAddress::default();
        let connection_info = ConnectionInfo::new(&remote_address);
        let decoder_buffer = DecoderBufferMut::new(&mut data);

        let (encoded_packet, _remaining) =
            ProtectedShort::decode(0, decoder_buffer, &connection_info, &20).unwrap();

        let encrypted_packet = encoded_packet
            .unprotect(
                keyset.active_key().key(),
                PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0)),
            )
            .unwrap();

        assert_eq!(keyset.decryption_error_count(), 0);
        assert!(keyset
            .decrypt_packet(
                encrypted_packet,
                PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0)),
                clock.get_time(),
            )
            .is_err());
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
        let clock = NoopClock {};
        let key = FailingKey::new(0, 0, 0);
        let mut keyset = KeySet::new(key);
        let mut data = [0; 128];
        let remote_address = SocketAddress::default();
        let connection_info = ConnectionInfo::new(&remote_address);
        let decoder_buffer = DecoderBufferMut::new(&mut data);

        let (encoded_packet, _remaining) =
            ProtectedShort::decode(0, decoder_buffer, &connection_info, &20).unwrap();

        let encrypted_packet = encoded_packet
            .unprotect(
                keyset.active_key().key(),
                PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0)),
            )
            .unwrap();

        assert_eq!(keyset.decryption_error_count(), 0);
        assert!(matches!(
            keyset.decrypt_packet(
                encrypted_packet,
                PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0)),
                clock.get_time()
            ),
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
        let key = TestKey::default();
        let mut crypto = LimitedUseCrypto::new(key);
        let mut encoder_bytes = [0; 512];
        let buffer = EncoderBuffer::new(&mut encoder_bytes);
        let mut decoder_bytes = [0; 512];

        assert_eq!(crypto.encrypted_packets(), 0);
        assert!(crypto
            .encode_packet(buffer, |buffer, _key| {
                let payload = ProtectedPayload::new(0, &mut decoder_bytes);

                Ok((payload, buffer))
            })
            .is_ok());

        assert_eq!(crypto.encrypted_packets(), 1);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //= type=test
    //# If the total number of encrypted packets with the same key
    //# exceeds the confidentiality limit for the selected AEAD, the endpoint
    //# MUST stop using those keys.
    #[test]
    fn test_encrypted_packet_count_enforced_aead_limit() {
        let key = FailingKey::new(0, 1, 1);
        let mut crypto = LimitedUseCrypto::new(key);
        let mut encoder_bytes = [0; 512];
        let buffer = EncoderBuffer::new(&mut encoder_bytes);
        let mut decoder_bytes = [0; 512];

        assert_eq!(crypto.encrypted_packets(), 0);
        assert!(crypto
            .encode_packet(buffer, |buffer, _key| {
                let payload = ProtectedPayload::new(0, &mut decoder_bytes);

                Ok((payload, buffer))
            })
            .is_ok());
        assert_eq!(crypto.encrypted_packets(), 1);

        let mut encoder_bytes = [0; 512];
        let buffer = EncoderBuffer::new(&mut encoder_bytes);
        assert!(matches!(
            crypto.encode_packet(buffer, |buffer, _key| {
                let payload = ProtectedPayload::new(0, &mut decoder_bytes);

                Ok((payload, buffer))
            }),
            Err(PacketEncodingError::AeadLimitReached(_))
        ));
    }
}
