// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::ProcessingError,
    crypto::{application::limited, OneRttKey, ProtectedPayload},
    packet::{
        encoding::PacketEncodingError,
        number::PacketNumber,
        short::{CleartextShort, EncryptedShort},
        KeyPhase,
    },
    time::{timer, Timer, Timestamp},
    transport,
};
use core::ops;
use s2n_codec::EncoderBuffer;

pub struct KeySet<K> {
    /// The current [`KeyPhase`]
    key_phase: KeyPhase,

    key_derivation_timer: Timer,

    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
    //# In addition to counting packets sent, endpoints MUST count the number
    //# of received packets that fail authentication during the lifetime of a
    //# connection.
    packet_decryption_failures: u64,
    aead_integrity_limit: u64,
    /// The number of times the key has been rotated
    generation: u16,

    /// Set of keys for the current and next phase
    crypto: KeyArray<K>,

    limits: limited::Limits,
}

impl<K: OneRttKey> KeySet<K> {
    pub fn new(crypto: K, limits: limited::Limits) -> Self {
        //= https://www.rfc-editor.org/rfc/rfc9001#section-6
        //# The Key Phase bit is initially set to 0 for the
        //# first set of 1-RTT packets and toggled to signal each subsequent key
        //# update.

        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.3
        //# Endpoints responding to an apparent key update MUST NOT generate a
        //# timing side-channel signal that might indicate that the Key Phase bit
        //# was invalid (see Section 9.4).

        //= https://www.rfc-editor.org/rfc/rfc9001#section-5.4
        //# The same header protection key is used for the duration of the
        //# connection, with the value not changing after a key update (see
        //# Section 6).  This allows header protection to be used to protect the
        //# key phase.
        // By pre-generating the next key, we can respond to a KeyUpdate without exposing a timing
        // side channel.
        let aead_integrity_limit = crypto.aead_integrity_limit();
        let next_key = limited::Key::new(crypto.derive_next_key());
        let active_key = limited::Key::new(crypto);

        Self {
            key_phase: KeyPhase::Zero,
            key_derivation_timer: Default::default(),
            packet_decryption_failures: 0,
            aead_integrity_limit,
            generation: 0,
            crypto: KeyArray([active_key, next_key]),
            limits,
        }
    }

    /// Rotating the phase will switch the active key
    fn rotate_phase(&mut self) {
        self.generation += 1;
        self.key_phase = KeyPhase::next_phase(self.key_phase);
    }

    /// Derive a new key based on the active key, and store it in the non-active slot
    fn derive_and_store_next_key(&mut self) {
        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.3
        //# Once generated, the next set of packet protection keys SHOULD be
        //# retained, even if the packet that was received was subsequently
        //# discarded.

        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
        //# After this period, old read keys and their corresponding secrets
        //# SHOULD be discarded.

        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
        //# These updated keys MAY replace the previous keys at that time.

        let next_key = self.active_key().derive_next_key();
        let next_phase = KeyPhase::next_phase(self.key_phase);
        self.crypto[next_phase] = limited::Key::new(next_key);
    }

    /// Set the timer to derive a new key after timestamp
    pub fn set_derivation_timer(&mut self, timestamp: Timestamp) {
        self.key_derivation_timer.set(timestamp)
    }

    /// Returns whether there is a key update in progress.
    pub fn key_update_in_progress(&self) -> bool {
        self.key_derivation_timer.is_armed()
    }

    /// Passes the key for the the requested phase to a callback function. Integrity limits are
    /// enforced.
    ///
    /// Returns the decrypted packet and generation if the key phase was rotated.
    pub fn decrypt_packet<'a>(
        &mut self,
        packet: EncryptedShort<'a>,
        largest_acknowledged_packet_number: PacketNumber,
        pto: Timestamp,
    ) -> Result<(CleartextShort<'a>, Option<u16>), ProcessingError> {
        let mut phase_to_use = self.key_phase() as u8;
        let packet_phase = packet.key_phase();
        let phase_switch = phase_to_use != (packet_phase as u8);
        phase_to_use ^= phase_switch as u8;

        if self.key_update_in_progress() && phase_switch {
            //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
            //# An endpoint MAY allow a period of approximately the Probe Timeout
            //# (PTO; see [QUIC-RECOVERY]) after promoting the next set of receive
            //# keys to be current before it creates the subsequent set of packet
            //# protection keys.

            //= https://www.rfc-editor.org/rfc/rfc9001#section-6.4
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

        let key = &mut self.crypto[phase_to_use.into()];

        let result = packet.decrypt(key.key());

        key.on_packet_decryption(&self.limits);

        match result {
            Ok(packet) => {
                let generation = if packet_phase != self.key_phase() {
                    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.2
                    //# Sending keys MUST be updated before sending an
                    //# acknowledgement for the packet that was received with updated keys.

                    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.2
                    //# The endpoint MUST update its
                    //# send keys to the corresponding key phase in response, as described in
                    //# Section 6.1.
                    self.rotate_phase();

                    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.3
                    //# Endpoints responding to an apparent key update MUST NOT generate a
                    //# timing side-channel signal that might indicate that the Key Phase bit
                    //# was invalid (see Section 9.4).

                    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
                    //# An endpoint SHOULD retain old read keys for no more than three times
                    //# the PTO after having received a packet protected using the new keys.

                    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.1
                    //# An endpoint SHOULD
                    //# retain old keys for some time after unprotecting a packet sent using
                    //# the new keys.
                    self.set_derivation_timer(pto);
                    Some(self.generation)
                } else {
                    None
                };

                Ok((packet, generation))
            }
            Err(err) => {
                //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
                //# In addition to counting packets sent, endpoints MUST count the number
                //# of received packets that fail authentication during the lifetime of a
                //# connection.
                self.packet_decryption_failures += 1;

                //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
                //# If a key update is not possible or
                //# integrity limits are reached, the endpoint MUST stop using the
                //# connection and only send stateless resets in response to receiving
                //# packets.

                //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
                //# If the total number of received packets that fail
                //# authentication within the connection, across all keys, exceeds the
                //# integrity limit for the selected AEAD, the endpoint MUST immediately
                //# close the connection with a connection error of type
                //# AEAD_LIMIT_REACHED and not process any more packets.
                if self.decryption_error_count() > self.aead_integrity_limit {
                    return Err(transport::Error::AEAD_LIMIT_REACHED.into());
                }

                Err(err)
            }
        }
    }

    /// This is the KeyPhase that should be used to encrypt a given packet.
    pub fn encryption_phase(&self) -> KeyPhase {
        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
        //# Endpoints MUST initiate a key update
        //# before sending more protected packets than the confidentiality limit
        //# for the selected AEAD permits.
        if self.active_key().needs_update(&self.limits) {
            return KeyPhase::next_phase(self.key_phase());
        }

        self.key_phase()
    }

    pub fn encrypt_packet<'a, F>(
        &mut self,
        buffer: EncoderBuffer<'a>,
        f: F,
    ) -> Result<(ProtectedPayload<'a>, EncoderBuffer<'a>), PacketEncodingError<'a>>
    where
        F: FnOnce(
            EncoderBuffer<'a>,
            &K,
            KeyPhase,
        )
            -> Result<(ProtectedPayload<'a>, EncoderBuffer<'a>), PacketEncodingError<'a>>,
    {
        let phase = self.encryption_phase();
        if self.crypto[phase].expired() {
            //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
            //# If the total number of encrypted packets with the same key
            //# exceeds the confidentiality limit for the selected AEAD, the endpoint
            //# MUST stop using those keys.

            //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
            //# If a key update is not possible or
            //# integrity limits are reached, the endpoint MUST stop using the
            //# connection and only send stateless resets in response to receiving
            //# packets.
            return Err(PacketEncodingError::AeadLimitReached(buffer));
        }

        let r = f(buffer, self.crypto[phase].key(), phase)?;

        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
        //# Endpoints MUST count the number of encrypted packets for each set of
        //# keys.
        self.crypto[phase].on_packet_encryption(&self.limits);

        Ok(r)
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        if self
            .key_derivation_timer
            .poll_expiration(timestamp)
            .is_ready()
        {
            //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
            //# An endpoint SHOULD retain old read keys for no more than three times
            //# the PTO after having received a packet protected using the new keys.
            self.derive_and_store_next_key();
        }
    }

    pub fn key_phase(&self) -> KeyPhase {
        self.key_phase
    }

    pub fn active_key(&self) -> &limited::Key<K> {
        &self.crypto[self.key_phase]
    }

    pub fn active_key_mut(&mut self) -> &mut limited::Key<K> {
        &mut self.crypto[self.key_phase]
    }

    fn decryption_error_count(&self) -> u64 {
        self.packet_decryption_failures
    }

    pub fn cipher_suite(&self) -> crate::crypto::tls::CipherSuite {
        self.crypto.0[0].key().cipher_suite()
    }
}

impl<K> timer::Provider for KeySet<K> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.key_derivation_timer.timers(query)?;
        Ok(())
    }
}

struct KeyArray<K>([limited::Key<K>; 2]);

impl<K> ops::Index<KeyPhase> for KeyArray<K> {
    type Output = limited::Key<K>;

    #[inline]
    fn index(&self, key_phase: KeyPhase) -> &Self::Output {
        &self.0[(key_phase as u8) as usize]
    }
}

impl<K> ops::IndexMut<KeyPhase> for KeyArray<K> {
    #[inline]
    fn index_mut(&mut self, key_phase: KeyPhase) -> &mut Self::Output {
        &mut self.0[(key_phase as u8) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        connection::id::ConnectionInfo,
        crypto::{
            testing::{HeaderKey as TestHeaderKey, Key as TestKey},
            ProtectedPayload,
        },
        inet::SocketAddress,
        packet::{
            encoding::PacketEncodingError, number::PacketNumberSpace, short::ProtectedShort,
            KeyPhase,
        },
        time::{testing::Clock, Clock as _},
        varint::VarInt,
    };
    use core::time::Duration;
    use s2n_codec::{DecoderBufferMut, EncoderBuffer};

    #[test]
    fn test_key_derivation_timer() {
        let mut clock = Clock::default();
        let now = clock.get_time();
        let mut keyset = KeySet::new(TestKey::default(), Default::default());
        keyset.rotate_phase();

        keyset.set_derivation_timer(now + Duration::from_millis(10));

        clock.inc_by(Duration::from_millis(8));
        keyset.on_timeout(clock.get_time());
        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.1
        //= type=test
        //# An endpoint SHOULD
        //# retain old keys for some time after unprotecting a packet sent using
        //# the new keys.
        assert_eq!(keyset.crypto[KeyPhase::Zero].key().derivations, 0);

        clock.inc_by(Duration::from_millis(8));
        keyset.on_timeout(clock.get_time());

        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
        //= type=test
        //# After this period, old read keys and their corresponding secrets
        //# SHOULD be discarded.
        assert_eq!(keyset.crypto[KeyPhase::Zero].key().derivations, 2);
    }

    #[test]
    fn test_key_set() {
        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.3
        //= type=test
        //# For this reason, endpoints MUST be able to retain two sets of packet
        //# protection keys for receiving packets: the current and the next.

        let keyset = KeySet::new(TestKey::default(), Default::default());

        assert_eq!(keyset.crypto[KeyPhase::Zero].key().derivations, 0);
        assert_eq!(keyset.crypto[KeyPhase::One].key().derivations, 1);
    }

    #[test]
    fn test_phase_rotation() {
        let mut keyset = KeySet::new(TestKey::default(), Default::default());

        assert_eq!(keyset.active_key().key().derivations, 0);
        keyset.rotate_phase();
        assert_eq!(keyset.active_key().key().derivations, 1);
    }

    #[test]
    fn test_key_derivation() {
        let mut keyset = KeySet::new(TestKey::default(), Default::default());

        keyset.rotate_phase();
        keyset.derive_and_store_next_key();
        keyset.rotate_phase();
        assert_eq!(keyset.active_key().key().derivations, 2);
    }

    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
    //= type=test
    //# In addition to counting packets sent, endpoints MUST count the number
    //# of received packets that fail authentication during the lifetime of a
    //# connection.
    #[test]
    fn test_decryption_failure_counter() {
        let clock = Clock::default();
        let key = TestKey {
            fail_on_decrypt: true,
            ..Default::default()
        };
        let mut keyset = KeySet::new(key, Default::default());
        let mut data = [0; 128];
        let remote_address = SocketAddress::default();
        let connection_info = ConnectionInfo::new(&remote_address);
        let decoder_buffer = DecoderBufferMut::new(&mut data);

        let (encoded_packet, _remaining) =
            ProtectedShort::decode(0, decoder_buffer, &connection_info, &20).unwrap();

        let encrypted_packet = encoded_packet
            .unprotect(
                &TestHeaderKey::default(),
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

    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
    //= type=test
    //# If the total number of received packets that fail
    //# authentication within the connection, across all keys, exceeds the
    //# integrity limit for the selected AEAD, the endpoint MUST immediately
    //# close the connection with a connection error of type
    //# AEAD_LIMIT_REACHED and not process any more packets.
    #[test]
    fn test_decryption_failure_enforced_aead_limit() {
        let clock = Clock::default();
        let key = TestKey {
            integrity_limit: 0,
            fail_on_decrypt: true,
            ..Default::default()
        };
        let mut keyset = KeySet::new(key, Default::default());
        let mut data = [0; 128];
        let remote_address = SocketAddress::default();
        let connection_info = ConnectionInfo::new(&remote_address);
        let decoder_buffer = DecoderBufferMut::new(&mut data);

        let (encoded_packet, _remaining) =
            ProtectedShort::decode(0, decoder_buffer, &connection_info, &20).unwrap();

        let encrypted_packet = encoded_packet
            .unprotect(
                &TestHeaderKey::default(),
                PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0)),
            )
            .unwrap();

        assert_eq!(keyset.decryption_error_count(), 0);
        assert_eq!(
            keyset
                .decrypt_packet(
                    encrypted_packet,
                    PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0)),
                    clock.get_time(),
                )
                .err(),
            Some(ProcessingError::ConnectionError(
                (transport::Error::AEAD_LIMIT_REACHED).into()
            ))
        );
    }

    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
    //= type=test
    //# Endpoints MUST count the number of encrypted packets for each set of
    //# keys.
    #[test]
    fn test_encrypted_packet_count_increased() {
        let key = TestKey::default();
        let mut keyset = KeySet::new(key, Default::default());
        let mut encoder_bytes = [0; 512];
        let buffer = EncoderBuffer::new(&mut encoder_bytes);
        let mut decoder_bytes = [0; 512];

        assert_eq!(keyset.active_key().encrypted_packets(), 0);
        assert!(keyset
            .encrypt_packet(buffer, |buffer, _key, _phase| {
                let payload = ProtectedPayload::new(0, &mut decoder_bytes);

                Ok((payload, buffer))
            })
            .is_ok());

        assert_eq!(keyset.active_key().encrypted_packets(), 1);
    }

    #[test]
    fn test_encrypted_packet_key_update_window() {
        let key = TestKey {
            confidentiality_limit: 10000,
            ..Default::default()
        };
        let mut keyset = KeySet::new(key, Default::default());
        let mut encoder_bytes = [0; 512];
        let buffer = EncoderBuffer::new(&mut encoder_bytes);
        let mut decoder_bytes = [0; 512];

        // The first encryption should use the expected keyphase, and put us into the
        // KEY_UPDATE_WINDOW.
        assert_eq!(keyset.active_key().encrypted_packets(), 0);
        assert!(!keyset.active_key().needs_update(&keyset.limits));
        assert!(keyset
            .encrypt_packet(buffer, |buffer, _key, _phase| {
                let payload = ProtectedPayload::new(0, &mut decoder_bytes);

                Ok((payload, buffer))
            })
            .is_ok());

        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
        //= type=test
        //# Endpoints MUST initiate a key update
        //# before sending more protected packets than the confidentiality limit
        //# for the selected AEAD permits.

        // Subsequent encryptions should be in the next phase and our key should need an update.
        assert_eq!(keyset.encryption_phase(), KeyPhase::One);
        assert!(keyset.active_key().needs_update(&keyset.limits));
    }

    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
    //= type=test
    //# If the total number of encrypted packets with the same key
    //# exceeds the confidentiality limit for the selected AEAD, the endpoint
    //# MUST stop using those keys.
    #[test]
    fn test_encrypted_packet_aead_limit() {
        let limit = 10_000;
        let key = TestKey {
            confidentiality_limit: limit,
            ..Default::default()
        };
        let mut keyset = KeySet::new(key, Default::default());
        let mut encoder_bytes = [0; 512];

        // The KeySet chooses the appropriate key phase. Trying to encrypt one more than the limit
        // will attempt a key update after the first encryption, and fill the update window of the
        // next key (because the key update never completes).
        for _ in 0..limit + 1 {
            let buffer = EncoderBuffer::new(&mut encoder_bytes);
            let mut decoder_bytes = [0; 512];
            assert!(keyset
                .encrypt_packet(buffer, |buffer, _key, _phase| {
                    let payload = ProtectedPayload::new(0, &mut decoder_bytes);

                    Ok((payload, buffer))
                })
                .is_ok());

            // As long as the keyphase is constant, we have not initiated any KeyUpdate, and we
            // have not derived any new keys.
            assert_eq!(keyset.key_phase(), KeyPhase::Zero);
        }

        // The key in KeyPhase::Zero will have encrypted a single packet.
        // Each additional request will be within the KEY_UPDATE_WINDOW, so the next key phase is
        // used.
        assert_eq!(keyset.crypto[KeyPhase::Zero].encrypted_packets(), 1);

        // The next key phase should have limit encryptions
        assert_eq!(keyset.crypto[KeyPhase::One].encrypted_packets(), limit);

        // The final encryption should push us over the AEAD limit and we should fail.
        let buffer = EncoderBuffer::new(&mut encoder_bytes);
        let mut decoder_bytes = [0; 512];
        assert!(matches!(
            keyset.encrypt_packet(buffer, |buffer, _key, _phase| {
                let payload = ProtectedPayload::new(0, &mut decoder_bytes);

                Ok((payload, buffer))
            }),
            Err(PacketEncodingError::AeadLimitReached(_))
        ));
    }
}
