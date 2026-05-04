// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `stream::recv::state::State` exercising packet-level invariants.

use crate::{
    credentials,
    crypto::{self, awslc, open::Application as _},
    event,
    packet::stream::{self, decoder::Packet, encoder},
    stream::{
        recv::{self, state::State},
        shared::AcceptState,
        TransportFeatures,
    },
};
use s2n_codec::{DecoderBufferMut, EncoderBuffer};
use s2n_quic_core::{
    buffer::{reader::incremental::Incremental, Reassembler},
    dc,
    inet::ExplicitCongestionNotification,
    time::clock::testing as clock,
    varint::VarInt,
};

// ---------------------------------------------------------------------------
// Custom reader for encoding packets with arbitrary final_offset values.
// ---------------------------------------------------------------------------

use s2n_quic_core::buffer::{reader::storage::Chunk, Reader as BufReader};

/// A reader that wraps a payload slice and allows setting an arbitrary final_offset,
/// independent of stream_offset + payload_len.
struct ArbitraryFinReader<'a> {
    offset: VarInt,
    payload: &'a [u8],
    cursor: usize,
    final_offset: Option<VarInt>,
}

impl<'a> ArbitraryFinReader<'a> {
    fn new(stream_offset: u64, payload: &'a [u8], final_offset: u64) -> Self {
        Self {
            offset: VarInt::new(stream_offset).unwrap(),
            payload,
            cursor: 0,
            final_offset: Some(VarInt::new(final_offset).unwrap()),
        }
    }
}

impl s2n_quic_core::buffer::reader::Storage for ArbitraryFinReader<'_> {
    type Error = core::convert::Infallible;

    fn buffered_len(&self) -> usize {
        self.payload.len() - self.cursor
    }

    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk<'_>, Self::Error> {
        let remaining = &self.payload[self.cursor..];
        let len = remaining.len().min(watermark);
        self.cursor += len;
        Ok((&remaining[..len]).into())
    }

    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
    where
        Dest: s2n_quic_core::buffer::writer::Storage + ?Sized,
    {
        self.read_chunk(dest.remaining_capacity())
    }
}

impl BufReader for ArbitraryFinReader<'_> {
    fn current_offset(&self) -> VarInt {
        self.offset + self.cursor
    }

    fn final_offset(&self) -> Option<VarInt> {
        self.final_offset
    }
}

// ---------------------------------------------------------------------------
// Static crypto keys — same key material for seal and open.
// ---------------------------------------------------------------------------

const KEY: &[u8; 16] = b"test-key-128bit!";
const IV: [u8; 12] = [0x42; 12];

fn sealer() -> awslc::seal::Application {
    awslc::seal::Application::new(KEY, IV, &awslc::AES_128_GCM)
}

fn opener() -> awslc::open::Application {
    awslc::open::Application::new(KEY, IV, &awslc::AES_128_GCM)
}

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

struct Harness {
    state: State,
    credentials: credentials::Credentials,
    stream_id: stream::Id,
    clock: clock::Clock,
    sealer: awslc::seal::Application,
    opener: awslc::open::Application,
    next_pn: u64,
    incremental: Incremental,
    out_buf: Reassembler,
}

impl Harness {
    fn new() -> Self {
        let clock = clock::Clock::default();
        let stream_id = stream::Id::default().reliable();
        let credentials = credentials::testing::new(0, 0);
        let params = dc::testing::TEST_APPLICATION_PARAMS;
        let state = State::new(stream_id, &params, TransportFeatures::TCP, &clock);

        Self {
            state,
            credentials,
            stream_id,
            clock,
            sealer: sealer(),
            opener: opener(),
            next_pn: 0,
            incremental: Incremental::new(VarInt::ZERO),
            out_buf: Reassembler::default(),
        }
    }

    /// Encode a stream packet using the next sequential pn and stream offset.
    fn encode_packet(&mut self, payload: &[u8], is_fin: bool) -> Vec<u8> {
        self.encode_packet_with_pn(self.next_pn, payload, is_fin)
    }

    /// Encode a stream packet with an explicit packet number but sequential offset.
    fn encode_packet_with_pn(&mut self, pn: u64, mut payload: &[u8], is_fin: bool) -> Vec<u8> {
        let mut buf = vec![0u8; 1024];
        let encoder_buf = EncoderBuffer::new(&mut buf);

        let mut reader = self.incremental.with_storage(&mut payload, is_fin).unwrap();

        let packet_len = encoder::encode(
            encoder_buf,
            None,
            self.stream_id,
            VarInt::new(pn).unwrap(),
            VarInt::ZERO,
            VarInt::ZERO,
            &mut &[][..],
            VarInt::ZERO,
            &(),
            &mut reader,
            &self.sealer,
            &self.credentials,
        );

        self.next_pn = pn + 1;
        buf.truncate(packet_len);
        buf
    }

    /// Encode a packet at a specific stream offset (non-sequential).
    fn encode_packet_at_offset(
        &mut self,
        pn: u64,
        stream_offset: u64,
        mut payload: &[u8],
        is_fin: bool,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 1024];
        let encoder_buf = EncoderBuffer::new(&mut buf);

        let mut inc = Incremental::new(VarInt::new(stream_offset).unwrap());
        let mut reader = inc.with_storage(&mut payload, is_fin).unwrap();

        let packet_len = encoder::encode(
            encoder_buf,
            None,
            self.stream_id,
            VarInt::new(pn).unwrap(),
            VarInt::ZERO,
            VarInt::ZERO,
            &mut &[][..],
            VarInt::ZERO,
            &(),
            &mut reader,
            &self.sealer,
            &self.credentials,
        );

        self.next_pn = pn + 1;
        buf.truncate(packet_len);
        buf
    }

    /// Encode a packet with an arbitrary final_offset (not derived from payload).
    fn encode_packet_with_final_offset(
        &mut self,
        pn: u64,
        stream_offset: u64,
        payload: &[u8],
        final_offset: u64,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 1024];
        let encoder_buf = EncoderBuffer::new(&mut buf);

        let mut reader = ArbitraryFinReader::new(stream_offset, payload, final_offset);

        let packet_len = encoder::encode(
            encoder_buf,
            None,
            self.stream_id,
            VarInt::new(pn).unwrap(),
            VarInt::ZERO,
            VarInt::ZERO,
            &mut &[][..],
            VarInt::ZERO,
            &(),
            &mut reader,
            &self.sealer,
            &self.credentials,
        );

        self.next_pn = pn + 1;
        buf.truncate(packet_len);
        buf
    }

    /// Decode raw bytes and feed the resulting packet into the recv State.
    fn feed(&mut self, raw: &mut [u8]) -> Result<(), recv::ErrorKind> {
        let tag_len = self.opener.tag_len();
        let decoder = DecoderBufferMut::new(raw);
        let (mut packet, _) = Packet::decode(decoder, (), tag_len).unwrap();

        let control = crypto::open::control::stream::Reliable::default();
        let publisher = event::testing::Publisher::no_snapshot();

        self.state
            .on_stream_packet(
                &self.opener,
                &control,
                &self.credentials,
                &mut packet,
                ExplicitCongestionNotification::default(),
                AcceptState::Accepted,
                &self.clock,
                &mut self.out_buf,
                &publisher,
            )
            .map_err(|e| e.kind)
    }
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[test]
fn sequential_packets_accepted() {
    let mut h = Harness::new();

    for i in 0..5u8 {
        let payload = vec![i; 100];
        let mut pkt = h.encode_packet(&payload, false);
        h.feed(&mut pkt).expect("sequential packet should succeed");
    }
}

#[test]
fn sequential_packets_with_fin_accepted() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"hello", false);
    h.feed(&mut pkt).unwrap();

    let mut pkt = h.encode_packet(b" world", true);
    h.feed(&mut pkt).unwrap();
}

// ---------------------------------------------------------------------------
// Non-sequential packet numbers
// ---------------------------------------------------------------------------

#[test]
fn out_of_order_packet_number_rejected() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"first", false);
    h.feed(&mut pkt).unwrap();

    // Skip packet 1, send packet 2
    let mut pkt = h.encode_packet_with_pn(2, b"third", false);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(
            err,
            recv::ErrorKind::OutOfOrder {
                expected: 1,
                actual: 2
            }
        ),
        "expected OutOfOrder, got {err:?}"
    );
}

#[test]
fn duplicate_packet_number_rejected() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"first", false);
    h.feed(&mut pkt).unwrap();

    // Re-send packet 0
    let mut pkt = h.encode_packet_with_pn(0, b"first", false);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(
            err,
            recv::ErrorKind::OutOfOrder {
                expected: 1,
                actual: 0
            }
        ),
        "expected OutOfOrder, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Non-contiguous stream offsets
// ---------------------------------------------------------------------------

/// STREAM transports enforce that stream offsets are contiguous — a gap in
/// offsets is rejected even if the packet number is sequential.
#[test]
fn gap_in_stream_offset_with_sequential_pn() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"0123456789", false);
    h.feed(&mut pkt).unwrap();

    // Packet 1 at offset 20 (gap of 10 bytes)
    let mut pkt = h.encode_packet_at_offset(1, 20, b"abcdefghij", false);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(
            err,
            recv::ErrorKind::OutOfOrder {
                expected: 10,
                actual: 20
            }
        ),
        "expected OutOfOrder for non-contiguous stream offset, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Invalid final_offset
// ---------------------------------------------------------------------------

#[test]
fn conflicting_final_offset_rejected() {
    let mut h = Harness::new();

    // Packet 0: 10 bytes, fin → final_offset = 10
    let mut pkt = h.encode_packet(b"0123456789", true);
    h.feed(&mut pkt).unwrap();

    // Packet 1: offset=10, 5 bytes, fin → final_offset = 15 (conflicts with 10)
    let mut pkt = h.encode_packet_at_offset(1, 10, b"extra", true);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(err, recv::ErrorKind::InvalidFin),
        "expected InvalidFin, got {err:?}"
    );
}

#[test]
fn final_offset_regresses_rejected() {
    let mut h = Harness::new();

    // Send 100 bytes then 10 more with fin → final_offset = 110
    let mut pkt = h.encode_packet(&[0xAB; 100], false);
    h.feed(&mut pkt).unwrap();
    let mut pkt = h.encode_packet(&[0xCD; 10], true);
    h.feed(&mut pkt).unwrap();

    // Packet at offset 110 with fin claiming final_offset=115 (conflicts with 110)
    let mut pkt = h.encode_packet_at_offset(2, 110, &[0xEF; 5], true);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(err, recv::ErrorKind::InvalidFin),
        "expected InvalidFin, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Tampered packets (AEAD verification)
// ---------------------------------------------------------------------------

#[test]
fn tampered_payload_rejected() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"authentic", false);
    let tag_start = pkt.len() - 16;
    pkt[tag_start] ^= 0xFF;

    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(err, recv::ErrorKind::Crypto(_)),
        "tampered packet should fail AEAD: {err:?}"
    );
}

#[test]
fn tampered_header_rejected() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"authentic", false);
    pkt[5] ^= 0xFF;

    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(
            err,
            recv::ErrorKind::Crypto(_) | recv::ErrorKind::CredentialMismatch { .. }
        ),
        "tampered header should be rejected: {err:?}"
    );
}

/// A forged packet at a non-contiguous offset is rejected by the stream
/// offset contiguity check before the reassembler ever sees the final_offset.
#[test]
fn forged_final_offset_at_wrong_offset_rejected() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"0123456789", false);
    h.feed(&mut pkt).unwrap();

    // Forged packet at offset 0 (expected 10) with final_offset=10
    let mut forged = h.encode_packet_at_offset(1, 0, b"XXXXXXXXXX", true);
    let tag_start = forged.len() - 16;
    forged[tag_start..].fill(0x00);

    let err = h.feed(&mut forged).unwrap_err();
    assert!(
        matches!(
            err,
            recv::ErrorKind::OutOfOrder {
                expected: 10,
                actual: 0
            }
        ),
        "expected OutOfOrder, got {err:?}"
    );
    assert_eq!(h.out_buf.final_size(), None);
}

/// A forged packet at the correct offset but with invalid AEAD is rejected
/// during decryption, and the reassembler does not retain the final_offset.
#[test]
fn forged_final_offset_at_correct_offset_rejected_by_aead() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"0123456789", false);
    h.feed(&mut pkt).unwrap();

    // Forged packet at offset 10 (correct) with final_offset=20, bad AEAD
    let mut forged = h.encode_packet_at_offset(1, 10, b"YYYYYYYYYY", true);
    let tag_start = forged.len() - 16;
    forged[tag_start..].fill(0x00);

    let err = h.feed(&mut forged).unwrap_err();
    assert!(
        matches!(err, recv::ErrorKind::Crypto(_)),
        "expected Crypto error, got {err:?}"
    );
    assert_eq!(
        h.out_buf.final_size(),
        None,
        "final_offset must not be set for unauthenticated packet"
    );
}

/// Harness for UDP (non-stream) transport where out-of-order and overlapping
/// data is permitted.
struct UdpHarness {
    state: State,
    credentials: credentials::Credentials,
    stream_id: stream::Id,
    clock: clock::Clock,
    sealer: awslc::seal::Application,
    opener: awslc::open::Application,
    out_buf: Reassembler,
}

impl UdpHarness {
    fn new() -> Self {
        let clock = clock::Clock::default();
        // Non-reliable stream ID for UDP
        let stream_id = stream::Id::default();
        let credentials = credentials::testing::new(0, 0);
        let params = dc::testing::TEST_APPLICATION_PARAMS;
        let state = State::new(stream_id, &params, TransportFeatures::UDP, &clock);

        Self {
            state,
            credentials,
            stream_id,
            clock,
            sealer: sealer(),
            opener: opener(),
            out_buf: Reassembler::default(),
        }
    }

    fn encode_packet_at_offset(
        &self,
        pn: u64,
        stream_offset: u64,
        payload: &[u8],
        is_fin: bool,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 1024];
        let encoder_buf = EncoderBuffer::new(&mut buf);

        let mut inc = Incremental::new(VarInt::new(stream_offset).unwrap());
        let mut storage = payload;
        let mut reader = inc.with_storage(&mut storage, is_fin).unwrap();

        let packet_len = encoder::encode(
            encoder_buf,
            None,
            self.stream_id,
            VarInt::new(pn).unwrap(),
            VarInt::ZERO,
            VarInt::ZERO,
            &mut &[][..],
            VarInt::ZERO,
            &(),
            &mut reader,
            &self.sealer,
            &self.credentials,
        );

        buf.truncate(packet_len);
        buf
    }

    /// Encode a packet with an arbitrary final_offset (not derived from payload).
    fn encode_packet_with_final_offset(
        &self,
        pn: u64,
        stream_offset: u64,
        payload: &[u8],
        final_offset: u64,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 1024];
        let encoder_buf = EncoderBuffer::new(&mut buf);

        let mut reader = ArbitraryFinReader::new(stream_offset, payload, final_offset);

        let packet_len = encoder::encode(
            encoder_buf,
            None,
            self.stream_id,
            VarInt::new(pn).unwrap(),
            VarInt::ZERO,
            VarInt::ZERO,
            &mut &[][..],
            VarInt::ZERO,
            &(),
            &mut reader,
            &self.sealer,
            &self.credentials,
        );

        buf.truncate(packet_len);
        buf
    }

    fn feed(&mut self, raw: &mut [u8]) -> Result<(), recv::ErrorKind> {
        let tag_len = self.opener.tag_len();
        let decoder = DecoderBufferMut::new(raw);
        let (mut packet, _) = Packet::decode(decoder, (), tag_len).unwrap();

        let control = crypto::open::control::stream::Reliable::default();
        let publisher = event::testing::Publisher::no_snapshot();

        self.state
            .on_stream_packet(
                &self.opener,
                &control,
                &self.credentials,
                &mut packet,
                ExplicitCongestionNotification::default(),
                AcceptState::Accepted,
                &self.clock,
                &mut self.out_buf,
                &publisher,
            )
            .map_err(|e| e.kind)
    }
}

/// On UDP, a forged packet whose payload fully overlaps already-received data and carries a
/// final_offset must be rejected by AEAD.
#[test]
fn udp_forged_fin_on_overlapping_data_rejected_by_aead() {
    use s2n_quic_core::buffer::reader::Storage as _;

    let mut h = UdpHarness::new();

    // Send packet 0: 10 bytes at offset 0
    let mut pkt = h.encode_packet_at_offset(0, 0, b"0123456789", false);
    h.feed(&mut pkt).unwrap();

    // Advance the reassembler's start_offset by reading the buffered data
    let _ = h.out_buf.read_chunk(10).unwrap();
    assert_eq!(h.out_buf.consumed_len(), 10);

    // Forged packet 1: overlapping offset 0, same payload, with FIN + bad AEAD tag.
    // The reassembler will call skip_until(10) since start_offset=10 > stream_offset=0.
    let mut forged = h.encode_packet_at_offset(1, 0, b"0123456789", true);
    let tag_start = forged.len() - 16;
    forged[tag_start..].fill(0x00);

    let err = h.feed(&mut forged).unwrap_err();
    assert!(
        matches!(err, recv::ErrorKind::Crypto(_)),
        "expected Crypto error from AEAD in skip_until, got {err:?}"
    );
    assert_eq!(
        h.out_buf.final_size(),
        None,
        "forged final_offset must not be accepted"
    );
}

/// On UDP, a legitimate retransmission with overlapping data and valid FIN
/// should still be accepted when AEAD passes.
#[test]
fn udp_valid_fin_on_overlapping_data_accepted() {
    use s2n_quic_core::buffer::reader::Storage as _;

    let mut h = UdpHarness::new();

    // Send packet 0: 10 bytes at offset 0
    let mut pkt = h.encode_packet_at_offset(0, 0, b"0123456789", false);
    h.feed(&mut pkt).unwrap();

    // Advance the reassembler's start_offset
    let _ = h.out_buf.read_chunk(10).unwrap();

    // Legitimate packet 1: same offset 0, same 10 bytes, with FIN (valid AEAD)
    let mut pkt = h.encode_packet_at_offset(1, 0, b"0123456789", true);
    h.feed(&mut pkt).unwrap();

    assert_eq!(
        h.out_buf.final_size(),
        Some(10),
        "valid FIN should set final_size"
    );
}

// ---------------------------------------------------------------------------
// Empty payload FIN (zero-length FIN packet)
// ---------------------------------------------------------------------------

/// A zero-length payload packet with FIN at the correct offset is valid and
/// must not be rejected by the skip_until AEAD enforcement.
#[test]
fn empty_payload_fin_accepted() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"hello", false);
    h.feed(&mut pkt).unwrap();

    // Empty payload with FIN at offset 5 → final_offset = 5
    let mut pkt = h.encode_packet(b"", true);
    h.feed(&mut pkt).unwrap();

    assert_eq!(h.out_buf.final_size(), Some(5));
}

// ---------------------------------------------------------------------------
// Stream offset contiguity edge cases
// ---------------------------------------------------------------------------

/// A packet with stream_offset=0 but overlapping the first byte (duplicate
/// of the first packet) is rejected by contiguity on stream transports.
#[test]
fn duplicate_stream_offset_rejected() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"first", false);
    h.feed(&mut pkt).unwrap();

    // Same offset 0 again (expected 5) — even with valid AEAD
    let mut pkt = h.encode_packet_at_offset(1, 0, b"first", false);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(
            err,
            recv::ErrorKind::OutOfOrder {
                expected: 5,
                actual: 0
            }
        ),
        "expected OutOfOrder, got {err:?}"
    );
}

/// Contiguity check with a short first packet followed by a gap of 1 byte.
#[test]
fn one_byte_gap_in_stream_offset_rejected() {
    let mut h = Harness::new();

    let mut pkt = h.encode_packet(b"A", false);
    h.feed(&mut pkt).unwrap();

    // Offset 2 instead of expected 1
    let mut pkt = h.encode_packet_at_offset(1, 2, b"C", false);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(
            err,
            recv::ErrorKind::OutOfOrder {
                expected: 1,
                actual: 2
            }
        ),
        "expected OutOfOrder, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// final_offset as VarInt: exercising non-trivial final_offset values
//
// The wire format encodes final_offset as a VarInt, not a boolean FIN bit.
// A packet can declare final_offset > stream_offset + payload_len (announcing
// the stream's total size without being the last packet), or an attacker can
// forge a final_offset that is inconsistent with the stream state.
// ---------------------------------------------------------------------------

/// A packet that announces final_offset ahead of its own data range is valid
/// (it tells the receiver the total stream size before all data arrives).
#[test]
fn final_offset_ahead_of_payload_accepted_on_tcp() {
    let mut h = Harness::new();

    // Packet 0: 10 bytes at offset 0, final_offset=50 (stream will be 50 bytes total)
    let mut pkt = h.encode_packet_with_final_offset(0, 0, b"0123456789", 50);
    h.feed(&mut pkt).unwrap();

    assert_eq!(h.out_buf.final_size(), Some(50));
}

/// A second packet with a different final_offset than previously announced is rejected.
#[test]
fn conflicting_final_offset_varint_rejected() {
    let mut h = Harness::new();

    // Packet 0: 10 bytes, final_offset=50
    let mut pkt = h.encode_packet_with_final_offset(0, 0, b"0123456789", 50);
    h.feed(&mut pkt).unwrap();

    // Packet 1: 10 bytes at offset 10, final_offset=100 (conflicts with 50)
    let mut pkt = h.encode_packet_with_final_offset(1, 10, b"abcdefghij", 100);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(err, recv::ErrorKind::InvalidFin),
        "expected InvalidFin for conflicting final_offset, got {err:?}"
    );
}

/// final_offset less than data already received is rejected.
#[test]
fn final_offset_less_than_received_data_rejected() {
    let mut h = Harness::new();

    // Packet 0: 20 bytes at offset 0 (no final_offset)
    let mut pkt = h.encode_packet(b"01234567890123456789", false);
    h.feed(&mut pkt).unwrap();

    // Packet 1: 5 bytes at offset 20, but final_offset=15 (less than 20 bytes already received)
    let mut pkt = h.encode_packet_with_final_offset(1, 20, b"abcde", 15);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(err, recv::ErrorKind::InvalidFin),
        "expected InvalidFin for final_offset < received data, got {err:?}"
    );
}

/// On UDP: a forged packet with final_offset ahead of payload on overlapping
/// data must still be rejected by AEAD during skip_until.
#[test]
fn udp_forged_ahead_final_offset_on_overlap_rejected_by_aead() {
    use s2n_quic_core::buffer::reader::Storage as _;

    let mut h = UdpHarness::new();

    // Send 10 bytes at offset 0
    let mut pkt = h.encode_packet_at_offset(0, 0, b"0123456789", false);
    h.feed(&mut pkt).unwrap();

    // Advance reassembler start_offset
    let _ = h.out_buf.read_chunk(10).unwrap();

    // Forged packet: offset 0, 10 bytes overlap, final_offset=100 (far ahead), bad AEAD
    let mut forged = h.encode_packet_with_final_offset(1, 0, b"0123456789", 100);
    let tag_start = forged.len() - 16;
    forged[tag_start..].fill(0x00);

    let err = h.feed(&mut forged).unwrap_err();
    assert!(
        matches!(err, recv::ErrorKind::Crypto(_)),
        "expected Crypto error from AEAD in skip_until, got {err:?}"
    );
    assert_eq!(
        h.out_buf.final_size(),
        None,
        "forged ahead final_offset must not be accepted"
    );
}

/// On UDP: a valid packet with final_offset ahead of its payload on
/// overlapping data should be accepted (AEAD passes).
#[test]
fn udp_valid_ahead_final_offset_on_overlap_accepted() {
    use s2n_quic_core::buffer::reader::Storage as _;

    let mut h = UdpHarness::new();

    // Send 10 bytes at offset 0
    let mut pkt = h.encode_packet_at_offset(0, 0, b"0123456789", false);
    h.feed(&mut pkt).unwrap();

    // Advance reassembler start_offset
    let _ = h.out_buf.read_chunk(10).unwrap();

    // Valid packet: offset 0, 10 bytes overlap, final_offset=50 (valid AEAD)
    let mut pkt = h.encode_packet_with_final_offset(1, 0, b"0123456789", 50);
    h.feed(&mut pkt).unwrap();

    assert_eq!(
        h.out_buf.final_size(),
        Some(50),
        "valid ahead final_offset should be accepted"
    );
}

/// On UDP: once final_offset is set, a subsequent overlapping packet with a
/// different final_offset is rejected even with valid AEAD.
#[test]
fn udp_conflicting_final_offset_on_overlap_rejected() {
    use s2n_quic_core::buffer::reader::Storage as _;

    let mut h = UdpHarness::new();

    // Send 10 bytes at offset 0 with final_offset=50
    let mut pkt = h.encode_packet_with_final_offset(0, 0, b"0123456789", 50);
    h.feed(&mut pkt).unwrap();

    // Advance reassembler start_offset
    let _ = h.out_buf.read_chunk(10).unwrap();

    // Valid AEAD packet: offset 0, 10 bytes overlap, but final_offset=100 (conflicts)
    let mut pkt = h.encode_packet_with_final_offset(1, 0, b"0123456789", 100);
    let err = h.feed(&mut pkt).unwrap_err();
    assert!(
        matches!(err, recv::ErrorKind::InvalidFin),
        "expected InvalidFin for conflicting final_offset on overlap, got {err:?}"
    );
}
