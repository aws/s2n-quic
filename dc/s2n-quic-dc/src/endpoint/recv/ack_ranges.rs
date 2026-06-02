// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Lightweight ACK range tracker for the stream recv path.
//!
//! Tracks received packet numbers and produces pre-encoded ACK range bodies suitable
//! for writing into the shared ACK state.

use bytes::Bytes;
use core::{fmt, ops::Bound};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    ack,
    packet::number::{PacketNumber, PacketNumberSpace},
    time::Timestamp,
    varint::VarInt,
};

/// Conservative overhead estimate for packet-level framing around an ACK body.
///
/// Accounts for: tag, credentials, wire_version, source_control_port, packet_number,
/// routing_info, header_len varint, Header::Ack metadata (dest_sender_id, ack_delay,
/// largest_acknowledged, ack_range, 3x ecn counts, payload_len varint), crypto tag.
pub const PACKET_OVERHEAD: usize = 120;

/// Result of encoding ACK ranges with the first range split out for the header.
pub struct EncodedAck {
    /// Largest acknowledged packet number (upper bound of first range).
    pub largest_acknowledged: VarInt,
    /// Smallest packet number in the first contiguous range.
    pub ack_range: VarInt,
    /// Additional gap/range VarInt pairs beyond the first range. Empty when no loss.
    pub extra_ranges: Bytes,
}

const CULL_DEPTH: usize = 2;

/// Fixed-capacity ring buffer tracking the largest PN from recent ACK encode+complete cycles.
///
/// When full, pushing a new value evicts and returns the oldest entry.
struct CullRing {
    buf: [VarInt; CULL_DEPTH],
    /// Packed: low bit = write index, bits 1..2 = valid entry count (0..=CULL_DEPTH).
    state: u8,
}

impl CullRing {
    const fn new() -> Self {
        Self {
            buf: [VarInt::ZERO; CULL_DEPTH],
            state: 0,
        }
    }

    fn idx(&self) -> usize {
        (self.state & 1) as usize
    }

    fn len(&self) -> usize {
        (self.state >> 1) as usize
    }

    /// Push a value. If full, returns the evicted (oldest) entry.
    fn push(&mut self, value: VarInt) -> Option<VarInt> {
        let idx = self.idx();
        let len = self.len();

        if len < CULL_DEPTH {
            self.buf[idx] = value;
            let next_idx = (idx + 1) % CULL_DEPTH;
            self.state = (((len + 1) as u8) << 1) | next_idx as u8;
            None
        } else {
            let evicted = self.buf[idx];
            self.buf[idx] = value;
            let next_idx = (idx + 1) % CULL_DEPTH;
            self.state = ((CULL_DEPTH as u8) << 1) | next_idx as u8;
            Some(evicted)
        }
    }
}

/// Tracks received packet numbers and encodes ACK range bodies for the shared state.
pub(crate) struct AckRanges {
    packets: ack::Ranges,
    /// When the largest packet number was received — written to the shared state so
    /// the sender can compute ack_delay at assembly time.
    max_received_packet_time: Option<Timestamp>,
    /// Largest PN from the most recent encode_body (awaiting completion confirmation).
    pending_cull_pn: Option<VarInt>,
    /// Tracks recent completions to determine when old ranges can be culled.
    cull_ring: CullRing,
}

impl fmt::Debug for AckRanges {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AckRanges")
            .field("range_count", &self.packets.interval_len())
            .field("max_received_packet_time", &self.max_received_packet_time)
            .finish()
    }
}

impl Default for AckRanges {
    fn default() -> Self {
        Self {
            packets: ack::Ranges::new(usize::MAX),
            max_received_packet_time: None,
            pending_cull_pn: None,
            cull_ring: CullRing::new(),
        }
    }
}

impl AckRanges {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn packets_for_test(&self) -> &ack::Ranges {
        &self.packets
    }

    /// Record a received packet number and its arrival time.
    pub fn on_packet_received(&mut self, packet_number: VarInt, now: Timestamp) {
        let pn = PacketNumberSpace::Initial.new_packet_number(packet_number);
        let prev_max = self.packets.max_value();
        if self.packets.insert_packet_number(pn).is_err() {
            return;
        }
        // Only update the arrival timestamp when pn is a strictly new maximum.
        // `insert_packet_number` returns Ok for duplicates (the interval set absorbs
        // them silently), so we must compare against the previous max rather than
        // relying solely on the Ok/Err return value.
        if self.packets.max_value() != prev_max && self.packets.max_value() == Some(pn) {
            self.max_received_packet_time = Some(now);
        }
    }

    /// Returns when the largest acknowledged packet was received, if any.
    pub fn largest_recv_time(&self) -> Option<Timestamp> {
        self.max_received_packet_time
    }

    /// Encode ACK ranges with the first range split out for the header.
    ///
    /// Returns the first range (`ack_range..=largest_acknowledged`) as direct fields
    /// and any additional gap/range pairs in `extra_ranges`. In the common no-loss case,
    /// `extra_ranges` is empty (no allocation).
    ///
    /// Pops the lowest ranges if the encoding exceeds `max_extra_len` so the ACK
    /// always fits in a single packet. The most recent ranges (highest PNs) are
    /// preserved since those are most useful for loss detection.
    ///
    /// Returns `None` if there are no ranges to encode.
    pub fn encode_ack(&mut self, max_extra_len: usize) -> Option<EncodedAck> {
        loop {
            if self.packets.is_empty() {
                self.max_received_packet_time = None;
                return None;
            }

            let extra_encoding_size = self.extra_ranges_encoding_size();
            if extra_encoding_size <= max_extra_len {
                let max_pn = self.packets.max_value().unwrap();
                self.pending_cull_pn = Some(PacketNumber::as_varint(max_pn));

                let largest_acknowledged = PacketNumber::as_varint(max_pn);
                let min_pn_in_first_range = self.first_range_min();
                let ack_range = min_pn_in_first_range;

                let extra_ranges = if extra_encoding_size == 0 {
                    Bytes::new()
                } else {
                    self.encode_extra_ranges()
                };

                return Some(EncodedAck {
                    largest_acknowledged,
                    ack_range,
                    extra_ranges,
                });
            }

            let _ = self.packets.pop_min();
            if self.packets.is_empty() {
                self.max_received_packet_time = None;
            }
        }
    }

    /// Returns the smallest PN in the first (highest) range.
    fn first_range_min(&self) -> VarInt {
        let first_range = self.packets.inclusive_ranges().next_back().unwrap();
        let (min_pn, _max_pn) = first_range.into_inner();
        PacketNumber::as_varint(min_pn)
    }

    /// Computes the encoding size for additional gap/range pairs (excludes the first range).
    fn extra_ranges_encoding_size(&self) -> usize {
        if self.packets.interval_len() <= 1 {
            return 0;
        }

        let mut size = 0usize;
        let mut prev_smallest: Option<VarInt> = None;
        // Iterate ranges in descending order (largest first)
        for range in self.packets.inclusive_ranges().rev() {
            let (range_start, range_end) = range.into_inner();
            let start = PacketNumber::as_varint(range_start);
            let end = PacketNumber::as_varint(range_end);
            if prev_smallest.is_none() {
                // First (highest) range — skip, it's in the header
                prev_smallest = Some(start);
                continue;
            }
            let prev = prev_smallest.unwrap();
            // gap = prev_smallest - end - 2 (RFC 9000 encoding)
            let gap = VarInt::new(prev.as_u64() - end.as_u64() - 2).unwrap_or(VarInt::ZERO);
            let range_len = VarInt::new(end.as_u64() - start.as_u64()).unwrap_or(VarInt::ZERO);
            size += gap.encoding_size() + range_len.encoding_size();
            prev_smallest = Some(start);
        }
        size
    }

    /// Encodes additional gap/range pairs into a Bytes buffer.
    fn encode_extra_ranges(&self) -> Bytes {
        let size = self.extra_ranges_encoding_size();
        let mut buf = vec![0u8; size];
        let mut encoder = EncoderBuffer::new(&mut buf);

        let mut prev_smallest: Option<VarInt> = None;
        for range in self.packets.inclusive_ranges().rev() {
            let (range_start, range_end) = range.into_inner();
            let start = PacketNumber::as_varint(range_start);
            let end = PacketNumber::as_varint(range_end);
            if prev_smallest.is_none() {
                prev_smallest = Some(start);
                continue;
            }
            let prev = prev_smallest.unwrap();
            let gap = VarInt::new(prev.as_u64() - end.as_u64() - 2).unwrap_or(VarInt::ZERO);
            let range_len = VarInt::new(end.as_u64() - start.as_u64()).unwrap_or(VarInt::ZERO);
            encoder.encode(&gap);
            encoder.encode(&range_len);
            prev_smallest = Some(start);
        }

        Bytes::from(buf)
    }

    /// Called when an ACK submission has been confirmed sent (completion returned).
    ///
    /// Advances the cull ring. Once the ring is full, the oldest entry is evicted
    /// and all ranges at or below it are removed. Returns the number of intervals
    /// removed (0 if no culling occurred).
    pub fn on_completion(&mut self) -> u64 {
        let Some(largest_pn) = self.pending_cull_pn.take() else {
            return 0;
        };

        let Some(cull_threshold) = self.cull_ring.push(largest_pn) else {
            return 0;
        };

        self.cull_ranges_up_to(cull_threshold)
    }

    fn cull_ranges_up_to(&mut self, threshold: VarInt) -> u64 {
        let before = self.packets.interval_len();
        if before <= 1 {
            return 0;
        }
        let threshold_pn = PacketNumberSpace::Initial.new_packet_number(threshold);
        // Never cull the highest range — ensures the set is never emptied.
        let max_pn = self.packets.max_value().unwrap();
        if threshold_pn >= max_pn {
            return 0;
        }
        let min_pn = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
        let _ = self
            .packets
            .remove((Bound::Included(min_pn), Bound::Included(threshold_pn)));
        let after = self.packets.interval_len();
        (before - after) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;
    use s2n_quic_core::varint::VarInt;

    fn ts(millis: u64) -> Timestamp {
        unsafe { Timestamp::from_duration(Duration::from_millis(millis)) }
    }

    fn pn(n: u64) -> VarInt {
        VarInt::new(n).unwrap()
    }

    // ── empty / basic ─────────────────────────────────────────────────────────

    #[test]
    fn empty_encode_returns_none() {
        let mut ranges = AckRanges::default();
        assert!(ranges.encode_ack(1024).is_none());
    }

    #[test]
    fn empty_largest_recv_time_is_none() {
        let ranges = AckRanges::default();
        assert!(ranges.largest_recv_time().is_none());
    }

    #[test]
    fn single_packet_encodes() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(0), ts(100));
        let encoded = ranges.encode_ack(1024);
        assert!(encoded.is_some(), "single packet should encode");
        let encoded = encoded.unwrap();
        assert_eq!(encoded.largest_acknowledged, pn(0));
        assert_eq!(encoded.ack_range, pn(0));
        assert!(encoded.extra_ranges.is_empty());
    }

    #[test]
    fn single_packet_sets_largest_recv_time() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(42), ts(100));
        assert_eq!(ranges.largest_recv_time(), Some(ts(100)));
    }

    // ── max_received_packet_time tracking ─────────────────────────────────────

    #[test]
    fn largest_recv_time_updated_on_new_maximum() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(5), ts(100));
        ranges.on_packet_received(pn(10), ts(200));
        assert_eq!(ranges.largest_recv_time(), Some(ts(200)));
    }

    #[test]
    fn largest_recv_time_not_updated_for_out_of_order() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(10), ts(200));
        ranges.on_packet_received(pn(3), ts(50));
        assert_eq!(
            ranges.largest_recv_time(),
            Some(ts(200)),
            "out-of-order packet should not update largest_recv_time"
        );
    }

    #[test]
    fn duplicate_packet_not_re_tracked() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(7), ts(100));
        ranges.on_packet_received(pn(7), ts(999));
        assert_eq!(
            ranges.largest_recv_time(),
            Some(ts(100)),
            "duplicate packet should not update timestamp"
        );
    }

    #[test]
    fn duplicate_current_max_packet_does_not_regress_timestamp() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(7), ts(100));
        ranges.on_packet_received(pn(9), ts(200));
        ranges.on_packet_received(pn(9), ts(50));

        assert_eq!(
            ranges.largest_recv_time(),
            Some(ts(200)),
            "duplicate of the current max packet should not rewind the timestamp"
        );
    }

    #[test]
    fn largest_recv_time_advances_monotonically() {
        let mut ranges = AckRanges::default();
        for i in 0u64..10 {
            ranges.on_packet_received(pn(i), ts(i * 10 + 1));
        }
        assert_eq!(ranges.largest_recv_time(), Some(ts(91)));
    }

    // ── encode_ack / trimming ─────────────────────────────────────────────────

    #[test]
    fn contiguous_range_has_empty_extra_ranges() {
        let mut ranges = AckRanges::default();
        for i in 0u64..5 {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        let encoded = ranges.encode_ack(1024).unwrap();
        assert_eq!(encoded.largest_acknowledged, pn(4));
        assert_eq!(encoded.ack_range, pn(0));
        assert!(
            encoded.extra_ranges.is_empty(),
            "contiguous range should produce empty extra_ranges"
        );
    }

    #[test]
    fn non_contiguous_range_has_extra_ranges() {
        let mut ranges = AckRanges::default();
        // Two disjoint ranges: 0..=2 and 5..=7
        for i in 0u64..=2 {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        for i in 5u64..=7 {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        let encoded = ranges.encode_ack(1024).unwrap();
        assert_eq!(encoded.largest_acknowledged, pn(7));
        assert_eq!(encoded.ack_range, pn(5));
        assert!(
            !encoded.extra_ranges.is_empty(),
            "non-contiguous ranges should produce non-empty extra_ranges"
        );
    }

    #[test]
    fn encode_trims_lowest_ranges_on_overflow() {
        let mut ranges = AckRanges::default();
        for i in (0u64..50).step_by(2) {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }

        let unconstrained = ranges.encode_ack(usize::MAX).unwrap();
        let mut ranges2 = AckRanges::default();
        for i in (0u64..50).step_by(2) {
            ranges2.on_packet_received(pn(i), ts(i + 1));
        }
        let constrained = ranges2.encode_ack(4);
        assert!(
            constrained.is_some(),
            "should return Some even with tight limit"
        );
        let constrained = constrained.unwrap();
        assert!(
            constrained.extra_ranges.len() <= unconstrained.extra_ranges.len(),
            "constrained extra_ranges should be no larger than unconstrained"
        );
        assert!(
            constrained.extra_ranges.len() <= 4,
            "constrained extra_ranges must fit within max_extra_len"
        );
    }

    #[test]
    fn encode_preserves_highest_range_after_trim() {
        let mut ranges = AckRanges::default();
        let high_pn = 99u64;
        for i in (0u64..=high_pn).step_by(3) {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        let encoded = ranges.encode_ack(4).unwrap();
        assert_eq!(
            encoded.largest_acknowledged,
            pn(99),
            "highest PN must be preserved after trimming"
        );
    }

    #[test]
    fn encode_zero_limit_produces_single_range() {
        let mut ranges = AckRanges::default();
        for i in 0u64..10 {
            ranges.on_packet_received(pn(i * 5), ts(i + 1));
        }
        // max_extra_len=0 forces trimming until single range remains
        let encoded = ranges.encode_ack(0);
        assert!(encoded.is_some(), "should still encode with single range");
        let encoded = encoded.unwrap();
        assert!(encoded.extra_ranges.is_empty());
    }

    #[test]
    fn encode_empty_result_clears_largest_recv_time() {
        let mut ranges = AckRanges::default();
        // Insert only non-contiguous single-element ranges. With max_extra_len=0,
        // encoding will pop ranges until only one remains, then succeed.
        // But if we have a case where popping results in empty, we test that path.
        // Actually, encode_ack with a single range always succeeds (extra_ranges=empty).
        // So let's test that an empty AckRanges returns None and clears time.
        assert!(ranges.encode_ack(0).is_none());
        assert!(ranges.largest_recv_time().is_none());
    }

    // ── repeated encode_ack calls ─────────────────────────────────────────────

    #[test]
    fn encode_ack_idempotent_when_no_new_packets() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(0), ts(1));
        ranges.on_packet_received(pn(1), ts(2));

        let e1 = ranges.encode_ack(1024).unwrap();
        let e2 = ranges.encode_ack(1024).unwrap();
        assert_eq!(e1.largest_acknowledged, e2.largest_acknowledged);
        assert_eq!(e1.ack_range, e2.ack_range);
        assert_eq!(e1.extra_ranges, e2.extra_ranges);
    }

    // ── culling ───────────────────────────────────────────────────────────────

    /// Simulate a full encode+complete cycle and return the cull count.
    fn encode_complete(ranges: &mut AckRanges) -> u64 {
        ranges.encode_ack(1024);
        ranges.on_completion()
    }

    #[test]
    fn cull_does_not_happen_below_threshold() {
        let mut ranges = AckRanges::default();
        for i in 0..10 {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        // First two completions fill the ring — no culling yet.
        assert_eq!(encode_complete(&mut ranges), 0);
        ranges.on_packet_received(pn(10), ts(11));
        assert_eq!(encode_complete(&mut ranges), 0);
        assert!(
            ranges
                .packets
                .contains(&PacketNumberSpace::Initial.new_packet_number(pn(0))),
            "ranges should still be intact before ring is full"
        );
    }

    #[test]
    fn cull_removes_old_ranges_after_threshold() {
        let mut ranges = AckRanges::default();
        // Phase 1: receive PNs 0..=9 (non-contiguous to create multiple intervals)
        for i in (0..10).step_by(2) {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        // First encode+complete: ring slot 0 filled (largest=8), no cull
        assert_eq!(encode_complete(&mut ranges), 0);

        // Phase 2: receive PNs 20..=29
        for i in (20..30).step_by(2) {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        // Second encode+complete: ring slot 1 filled (largest=28), no cull
        assert_eq!(encode_complete(&mut ranges), 0);

        // Phase 3: receive PNs 40..=49
        for i in (40..50).step_by(2) {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        // Third encode+complete: ring full, evicts slot 0 (threshold=8), culls <=8
        let culled = encode_complete(&mut ranges);
        assert!(culled > 0, "should have culled old ranges");

        // PNs 0,2,4,6,8 should be gone
        for i in (0..=8).step_by(2) {
            assert!(
                !ranges
                    .packets
                    .contains(&PacketNumberSpace::Initial.new_packet_number(pn(i))),
                "PN {} should have been culled",
                i
            );
        }
        // PNs 20+ should survive
        for i in (20..30).step_by(2) {
            assert!(
                ranges
                    .packets
                    .contains(&PacketNumberSpace::Initial.new_packet_number(pn(i))),
                "PN {} should still be present",
                i
            );
        }
    }

    #[test]
    fn cull_preserves_largest_recv_time() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(0), ts(100));
        encode_complete(&mut ranges);
        ranges.on_packet_received(pn(10), ts(200));
        encode_complete(&mut ranges);
        ranges.on_packet_received(pn(20), ts(300));
        encode_complete(&mut ranges);
        // PN 0 culled, but largest_recv_time is for PN 20
        assert_eq!(ranges.largest_recv_time(), Some(ts(300)));
    }

    #[test]
    fn encode_after_cull_still_works() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(0), ts(1));
        encode_complete(&mut ranges);
        ranges.on_packet_received(pn(5), ts(2));
        encode_complete(&mut ranges);
        ranges.on_packet_received(pn(10), ts(3));
        encode_complete(&mut ranges);
        // After culling, encode should still succeed with remaining ranges
        let encoded = ranges.encode_ack(1024);
        assert!(encoded.is_some());
    }

    #[test]
    fn cull_ring_push_returns_none_until_full() {
        let mut ring = CullRing::new();
        assert_eq!(ring.push(pn(10)), None);
        assert_eq!(ring.push(pn(20)), None);
        // Now full — next push evicts oldest
        assert_eq!(ring.push(pn(30)), Some(pn(10)));
        assert_eq!(ring.push(pn(40)), Some(pn(20)));
    }

    #[test]
    fn on_completion_without_encode_is_noop() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(5), ts(1));
        // Call on_completion without prior encode_body
        assert_eq!(ranges.on_completion(), 0);
    }
}
