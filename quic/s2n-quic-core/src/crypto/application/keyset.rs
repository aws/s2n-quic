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

/// s2n-quic keeps track of two keys at all times, the current key in use
/// as well as the next key that will be used after a key update is initiated.
/// We keep the older key around for a short while after a key update, which
/// enables us to read older messages that might have been lost around the time
/// of the key update. Once the timer expires the older key is dropped and a new
/// key is generated that will start being used at the next key update.
///
/// To further explain, lets say we track a keyset: k[0] = 'a', k[1] = 'b'. 'a' is currently
/// being used for decryption. Incoming packets signal that they can be decrypted by 'a'
/// with the key phase bit in their packet header.
/// P1 k[0]
/// P2 k[0]
/// P3 k[0]
/// ...
/// P10 k[1]        --> Once we receive a packet with the key phase bit changed, we use 'b' for
/// -> Timer set        decryption. This also begins a timer to rotate the old key.
///
/// P8 k[0]         --> We are still able to decrypt packets with the old key, 'a', if they arrive in
///                     this period.
///
/// -> Timer expires --> We now generate a new k[0] = 'c'. Any packets that arrive encrypted with k[0] = 'a'
///                      will fail to decrypt since we no longer store that key. Now k[1] = 'b' is our
///                      current key and k[0] = c is our future key.
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

    /// The largest packet number that has been successfully authenticated.
    ///
    /// This is used to distinguish a genuine key update from a reordered or
    /// replayed packet carrying the opposite Key Phase bit: only a packet whose
    /// number is higher than any previously authenticated packet may initiate a
    /// key-phase rotation (RFC 9001 Section 6.5).
    largest_authenticated_packet_number: Option<PacketNumber>,
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
            largest_authenticated_packet_number: None,
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

    /// Passes the key for the requested phase to a callback function. Integrity limits are
    /// enforced.
    ///
    /// Returns the decrypted packet and generation if the key phase was rotated.
    pub fn decrypt_packet<'a>(
        &mut self,
        packet: EncryptedShort<'a>,
        pto: Timestamp,
    ) -> Result<(CleartextShort<'a>, Option<u16>), ProcessingError> {
        let packet_phase = packet.key_phase();
        let packet_number = packet.packet_number;
        let phase_switch = packet_phase != self.key_phase();

        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
        //# Alternatively, endpoints can retain only two sets of packet
        //# protection keys, swapping previous for next after enough time has
        //# passed to allow for reordering in the network.  In this case, the Key
        //# Phase bit alone can be used to select keys.
        // We retain exactly two key sets, so the packet's Key Phase bit selects
        // the key directly: the current key when the phase matches, and the
        // adjacent (retired or next) key when it differs.
        let key = &mut self.crypto[packet_phase];

        let result = packet.decrypt(key.key_mut());

        key.on_packet_decryption();

        match result {
            Ok(packet) => {
                //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
                //# A recovered packet number that is lower than any packet number from
                //# the current key phase uses the previous packet protection keys; a
                //# recovered packet number that is higher than any packet number from
                //# the current key phase requires the use of the next packet protection
                //# keys.
                // A key-phase rotation is only initiated when an opposite-phase packet
                // advances the packet number beyond anything previously authenticated.
                // A reordered or replayed packet carrying the previous phase (i.e. a
                // lower packet number) was decrypted with the retained previous keys and
                // MUST NOT change the key phase, generation, or derivation timer.
                let advances_packet_number = self
                    .largest_authenticated_packet_number
                    .is_none_or(|largest| packet_number > largest);

                let generation = if phase_switch && advances_packet_number {
                    if self.key_update_in_progress() {
                        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.4
                        //# An endpoint that successfully removes protection with old
                        //# keys when newer keys were used for packets with lower packet
                        //# numbers MUST treat this as a connection error of type
                        //# KEY_UPDATE_ERROR.
                        // A key update is in progress, so the opposite-phase slot still
                        // holds the retired keys. An opposite-phase packet with a higher
                        // packet number was therefore protected with old keys, which is
                        // illegal.
                        return Err(transport::Error::KEY_UPDATE_ERROR
                            .with_reason(
                                "packet protected with old keys carried a larger packet number",
                            )
                            .into());
                    }

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

                //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
                //# For receiving packets during a key update, packets protected with
                //# older keys might arrive if they were delayed by the network.
                // Record the largest authenticated packet number so that later
                // delayed or replayed packets cannot be mistaken for a key update.
                self.largest_authenticated_packet_number = Some(
                    self.largest_authenticated_packet_number
                        .map_or(packet_number, |largest| largest.max(packet_number)),
                );

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
                if self.decryption_error_count() >= self.aead_integrity_limit {
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
            &mut K,
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

        let r = f(buffer, self.crypto[phase].key_mut(), phase)?;

        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
        //# Endpoints MUST count the number of encrypted packets for each set of
        //# keys.
        self.crypto[phase].on_packet_encryption();

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

    pub fn cipher_suite(&mut self) -> crate::crypto::tls::CipherSuite {
        self.crypto.0[0].key_mut().cipher_suite()
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
            encoding::PacketEncodingError,
            number::PacketNumberSpace,
            short::{EncryptedShort, ProtectedShort},
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
        assert_eq!(keyset.crypto[KeyPhase::Zero].key_mut().derivations, 0);

        clock.inc_by(Duration::from_millis(8));
        keyset.on_timeout(clock.get_time());

        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
        //= type=test
        //# After this period, old read keys and their corresponding secrets
        //# SHOULD be discarded.
        assert_eq!(keyset.crypto[KeyPhase::Zero].key_mut().derivations, 2);
    }

    #[test]
    fn test_key_set() {
        //= https://www.rfc-editor.org/rfc/rfc9001#section-6.3
        //= type=test
        //# For this reason, endpoints MUST be able to retain two sets of packet
        //# protection keys for receiving packets: the current and the next.

        let mut keyset = KeySet::new(TestKey::default(), Default::default());

        assert_eq!(keyset.crypto[KeyPhase::Zero].key_mut().derivations, 0);
        assert_eq!(keyset.crypto[KeyPhase::One].key_mut().derivations, 1);
    }

    #[test]
    fn test_phase_rotation() {
        let mut keyset = KeySet::new(TestKey::default(), Default::default());

        assert_eq!(keyset.active_key_mut().key_mut().derivations, 0);
        keyset.rotate_phase();
        assert_eq!(keyset.active_key_mut().key_mut().derivations, 1);
    }

    #[test]
    fn test_key_derivation() {
        let mut keyset = KeySet::new(TestKey::default(), Default::default());

        keyset.rotate_phase();
        keyset.derive_and_store_next_key();
        keyset.rotate_phase();
        assert_eq!(keyset.active_key_mut().key_mut().derivations, 2);
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
            .decrypt_packet(encrypted_packet, clock.get_time(),)
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
                .decrypt_packet(encrypted_packet, clock.get_time(),)
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

    /// Builds an `EncryptedShort` (1-RTT) packet whose key-phase bit and packet
    /// number can be controlled, using the all-zero test crypto so decryption
    /// always succeeds (modeling an authentic, captured packet).
    fn make_short_packet(
        data: &mut [u8; 128],
        key_phase_one: bool,
        wire_pn: u8,
    ) -> EncryptedShort<'_> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
        // The Key Phase bit is 0x04 of the first (tag) byte of a short header.
        const KEY_PHASE_BIT: u8 = 0x04;

        let tag = if key_phase_one { KEY_PHASE_BIT } else { 0 };
        // Header layout: 1 tag byte, then a 20-byte DCID (see the `&20` validator
        // below), so the 1-byte packet number lives at offset 21.
        data[0] = tag;
        data[21] = wire_pn;

        let remote_address = SocketAddress::default();
        let connection_info = ConnectionInfo::new(&remote_address);
        let decoder_buffer = DecoderBufferMut::new(data);

        let (protected, _remaining) =
            ProtectedShort::decode(tag, decoder_buffer, &connection_info, &20).unwrap();

        // Expanding the truncated packet number against a largest-acknowledged of
        // 0 yields the wire value verbatim for these small numbers.
        protected
            .unprotect(
                &TestHeaderKey::default(),
                PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0)),
            )
            .unwrap()
    }

    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
    //= type=test
    //# A recovered packet number that is lower than any packet number from
    //# the current key phase uses the previous packet protection keys; a
    //# recovered packet number that is higher than any packet number from
    //# the current key phase requires the use of the next packet protection
    //# keys.
    //
    // Regression test: after a legitimate key update, a reordered or replayed
    // OLD-phase packet (lower packet number, decrypted with the retained retired
    // key) must NOT rewind the endpoint's key phase, bump the generation, or
    // re-arm the derivation timer. It should still decrypt successfully.
    #[test]
    fn replayed_packets_do_not_rotate_keys() {
        let clock = Clock::default();
        let pto = clock.get_time();
        let mut keyset = KeySet::new(TestKey::default(), Default::default());

        // Start in phase Zero, generation 0, with no update in progress.
        assert_eq!(keyset.key_phase(), KeyPhase::Zero);
        assert_eq!(keyset.generation, 0);
        assert!(!keyset.key_update_in_progress());

        // 1. Legitimate key update: a NEW-phase (One) packet with a higher packet
        //    number. This rotates the endpoint to phase One, bumps the generation,
        //    and arms the derivation timer while retaining the old (phase Zero) key.
        let mut update_bytes = [0u8; 128];
        let update = make_short_packet(&mut update_bytes, true, 10);
        let (_pkt, gen) = keyset.decrypt_packet(update, pto).unwrap();
        assert_eq!(gen, Some(1));
        assert_eq!(keyset.key_phase(), KeyPhase::One);
        assert_eq!(keyset.generation, 1);
        assert!(keyset.key_update_in_progress());

        // 2. Reordered/replayed OLD-phase (Zero) packet with a LOWER packet number,
        //    delivered within the PTO window while the retired key is still held.
        let mut replay_bytes = [0u8; 128];
        let replay = make_short_packet(&mut replay_bytes, false, 7);
        let (_pkt, gen) = keyset.decrypt_packet(replay, pto).unwrap();

        // The replayed packet still authenticates...
        // ...but it MUST NOT drive the key-update state machine:
        assert_eq!(
            gen, None,
            "a replayed old-phase packet must not report a new generation"
        );
        assert_eq!(
            keyset.key_phase(),
            KeyPhase::One,
            "the send key phase must not rewind to the retired generation"
        );
        assert_eq!(
            keyset.generation, 1,
            "the generation counter must not advance on a replayed old-phase packet"
        );

        // 3. OLD-phase (Zero) packet with a HIGHER packet number. This is an error case as it indicates
        // a bad peer who is encrypting newer packets with old keys.
        let mut replay_bytes = [0u8; 128];
        let replay = make_short_packet(&mut replay_bytes, false, 12);
        let err = keyset
            .decrypt_packet(replay, pto)
            .expect_err("Higher packet number should not be accepted with older key");

        assert_eq!(
            err,
            ProcessingError::ConnectionError(
                transport::Error::KEY_UPDATE_ERROR
                    .with_reason("packet protected with old keys carried a larger packet number")
                    .into()
            )
        );
    }
}
