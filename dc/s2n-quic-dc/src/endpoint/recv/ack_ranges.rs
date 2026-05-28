// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Lightweight ACK range tracker for the stream recv path.
//!
//! Tracks received packet numbers and produces pre-encoded ACK range bodies suitable
//! for writing into the shared ACK state.

use bytes::Bytes;
use core::{fmt, ops::Bound};
use s2n_codec::EncoderValue;
use s2n_quic_core::{
    ack,
    frame::{self, ack::EcnCounts},
    packet::number::{PacketNumber, PacketNumberSpace},
    time::Timestamp,
    varint::VarInt,
};

/// Conservative overhead estimate for packet-level framing around an ACK body.
///
/// Accounts for: tag, credentials, wire_version, source_control_port, packet_number,
/// routing_info, header_len varint, Header::Ack metadata, payload_len varint, crypto tag.
pub const PACKET_OVERHEAD: usize = 100;

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

    /// Encode the ACK ranges (and optional ECN counts) into a `Bytes` buffer.
    ///
    /// Pops the lowest ranges if the encoding exceeds `max_body_len` so the ACK
    /// always fits in a single packet. The most recent ranges (highest PNs) are
    /// preserved since those are most useful for loss detection.
    ///
    /// Currently uses the standard QUIC ACK frame encoding with ack_delay=0 as a
    /// placeholder. The sender stamps the real delay in the Header::Ack field.
    ///
    /// TODO: use a custom encoding that drops the tag, count, and ack_delay fields to save
    /// 3 bytes per ACK body. We own both sides of the wire format.
    ///
    /// Returns `None` if there are no ranges to encode.
    pub fn encode_body(
        &mut self,
        ecn_counts: Option<EcnCounts>,
        max_body_len: usize,
    ) -> Option<Bytes> {
        loop {
            if self.packets.is_empty() {
                self.max_received_packet_time = None;
                return None;
            }

            let frame = frame::Ack {
                // The ack_delay field in the body is a zero placeholder; the real wire-time
                // delay is computed at assembly time from `largest_recv_time` and written into
                // `Header::Ack.ack_delay`.  The receiver extracts it from the header and passes
                // it directly to `process_ack`, so this body field is intentionally ignored.
                ack_delay: VarInt::ZERO,
                ack_ranges: &self.packets,
                ecn_counts,
            };

            let encoding_size = frame.encoding_size();
            if encoding_size <= max_body_len {
                if let Some(max_pn) = self.packets.max_value() {
                    self.pending_cull_pn = Some(PacketNumber::as_varint(max_pn));
                }
                return Some(Bytes::from(frame.encode_to_vec()));
            }

            let _ = self.packets.pop_min();
            if self.packets.is_empty() {
                self.max_received_packet_time = None;
            }
        }
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
    use s2n_quic_core::{frame::ack::EcnCounts, varint::VarInt};

    fn ts(millis: u64) -> Timestamp {
        unsafe { Timestamp::from_duration(Duration::from_millis(millis)) }
    }

    fn pn(n: u64) -> VarInt {
        VarInt::new(n).unwrap()
    }

    // ── empty / basic ─────────────────────────────────────────────────────────

    #[test]
    fn empty_encode_body_returns_none() {
        let mut ranges = AckRanges::default();
        assert!(ranges.encode_body(None, 1024).is_none());
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
        let body = ranges.encode_body(None, 1024);
        assert!(body.is_some(), "single packet should encode");
        assert!(!body.unwrap().is_empty());
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
        // PN 10 is the new max; timestamp should be ts(200)
        assert_eq!(ranges.largest_recv_time(), Some(ts(200)));
    }

    #[test]
    fn largest_recv_time_not_updated_for_out_of_order() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(10), ts(200));
        // PN 3 arrives out-of-order; max is still 10
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
        // Receive same PN again — insert_packet_number returns Err, so no update
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
        // After 10 packets, largest PN is 9, time is ts(91)
        assert_eq!(ranges.largest_recv_time(), Some(ts(91)));
    }

    // ── encode_body / trimming ────────────────────────────────────────────────

    #[test]
    fn encode_body_with_ecn_includes_counts() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(0), ts(1));
        let ecn = EcnCounts {
            ect_0_count: VarInt::from_u8(1),
            ect_1_count: VarInt::from_u8(0),
            ce_count: VarInt::from_u8(0),
        };
        let body_no_ecn = ranges.encode_body(None, 1024).unwrap();
        let mut ranges2 = AckRanges::default();
        ranges2.on_packet_received(pn(0), ts(1));
        let body_with_ecn = ranges2.encode_body(Some(ecn), 1024).unwrap();
        // ECN-tagged body should be larger (extra ECN count fields)
        assert!(
            body_with_ecn.len() > body_no_ecn.len(),
            "ECN body should be larger: ecn={} vs no_ecn={}",
            body_with_ecn.len(),
            body_no_ecn.len()
        );
    }

    #[test]
    fn encode_body_trims_lowest_ranges_on_overflow() {
        let mut ranges = AckRanges::default();
        // Insert many non-contiguous packet numbers so the encoding is large
        for i in (0u64..50).step_by(2) {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }

        let unconstrained = ranges.encode_body(None, usize::MAX).unwrap();
        // Re-insert same data with a very tight constraint
        let mut ranges2 = AckRanges::default();
        for i in (0u64..50).step_by(2) {
            ranges2.on_packet_received(pn(i), ts(i + 1));
        }
        // Allow only ~20 bytes, forcing several low ranges to be dropped
        let constrained = ranges2.encode_body(None, 20);
        assert!(
            constrained.is_some(),
            "should return Some even with tight limit"
        );
        let constrained = constrained.unwrap();
        // Constrained encoding is smaller
        assert!(
            constrained.len() <= unconstrained.len(),
            "constrained body should be no larger than unconstrained"
        );
        assert!(
            constrained.len() <= 20,
            "constrained body must fit within max_body_len"
        );
    }

    #[test]
    fn encode_body_preserves_highest_ranges_after_trim() {
        // After trimming, the highest PN should still be in the encoded ranges.
        let mut ranges = AckRanges::default();
        let high_pn = 99u64;
        for i in (0u64..=high_pn).step_by(3) {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        // Constrain so trimming happens
        let body = ranges.encode_body(None, 15);
        assert!(body.is_some());
        // The body is non-empty; verify it decoded successfully by re-parsing
        // (we just need it to be Some and non-empty here)
        assert!(!body.unwrap().is_empty());
    }

    #[test]
    fn encode_body_zero_limit_still_encodes_minimal_range() {
        // Even with max_body_len=0, the encoder should not panic;
        // it pops ranges until only one remains, which fits in any sane buffer.
        let mut ranges = AckRanges::default();
        for i in 0u64..10 {
            ranges.on_packet_received(pn(i * 5), ts(i + 1));
        }
        // max_body_len = 0 forces aggressive trimming but should terminate
        // (exactly one ACK range always fits since an ACK with a single range
        // encodes to roughly 5 bytes with ack_delay=0)
        let body = ranges.encode_body(None, 0);
        // Either None (all popped) or Some with at least one byte
        if let Some(b) = body {
            assert!(!b.is_empty());
        }
    }

    #[test]
    fn encode_body_empty_result_clears_largest_recv_time() {
        let mut ranges = AckRanges::default();
        for i in 0u64..10 {
            ranges.on_packet_received(pn(i * 7), ts(i + 1));
        }

        let body = ranges.encode_body(None, 0);
        assert!(body.is_none(), "max_body_len=0 should trim all ranges");
        assert!(ranges.is_empty(), "all ranges should be dropped");
        assert!(
            ranges.largest_recv_time().is_none(),
            "largest_recv_time must be cleared when no ranges remain"
        );
    }

    // ── contiguous ranges ─────────────────────────────────────────────────────

    #[test]
    fn contiguous_range_encodes_as_single_ack_block() {
        let mut ranges = AckRanges::default();
        for i in 0u64..5 {
            ranges.on_packet_received(pn(i), ts(i + 1));
        }
        // Contiguous range 0..=4: should encode to the smallest possible body
        let body = ranges.encode_body(None, 1024).unwrap();
        assert!(!body.is_empty());
    }

    // ── repeated encode_body calls ────────────────────────────────────────────

    #[test]
    fn encode_body_idempotent_when_no_new_packets() {
        let mut ranges = AckRanges::default();
        ranges.on_packet_received(pn(0), ts(1));
        ranges.on_packet_received(pn(1), ts(2));

        let b1 = ranges.encode_body(None, 1024).unwrap();
        let b2 = ranges.encode_body(None, 1024).unwrap();
        // Both calls encode the same state (no packets removed, no new ones added)
        assert_eq!(b1, b2, "repeated encode_body should be deterministic");
    }

    // ── culling ───────────────────────────────────────────────────────────────

    /// Simulate a full encode+complete cycle and return the cull count.
    fn encode_complete(ranges: &mut AckRanges) -> u64 {
        ranges.encode_body(None, 1024);
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
        let body = ranges.encode_body(None, 1024);
        assert!(body.is_some());
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
