// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Packet number map for tracking sent packets in the frame aggregation model.
//!
//! Each packet number maps to a PacketEntry containing a list of Frames and shared
//! transmission metadata. When a packet is ACKed, all constituent frames get their
//! completion notifications. When a packet is lost, frames are individually evaluated
//! for retransmission (checking TTL and should_transmit).

use crate::{congestion, counter::QueueGauge, endpoint::frame::Frame, intrusive::Queue};
use s2n_quic_core::{
    packet::number::{map::SortedVecMap as Inner, PacketNumber, PacketNumberRange},
    varint::VarInt,
};

/// Metadata captured when a packet is sent, shared across all frames in that packet.
pub(crate) struct TransmissionInfo {
    pub cc_info: congestion::PacketInfo,
    pub time_sent: s2n_quic_core::time::Timestamp,
    /// Total bytes on the wire for this packet (all frame payloads + headers + encryption overhead)
    pub sent_bytes: u16,
}

/// A single entry in the packet number map.
///
/// Contains all frames that were packed into this packet, plus the shared transmission
/// metadata. When ACKed, each frame's completion fires. When lost, each frame is
/// individually evaluated for retransmission.
///
/// ACK (control/immediate) frames are stripped before insertion — they are stale on
/// any retransmit and must not be re-sent. So `frames` contains only data (ack-eliciting)
/// frames.
pub(crate) struct Packet {
    /// All data frames packed into this packet.
    ///
    /// ACK/control frames are stripped before insertion — they are stale on retransmit.
    /// When this packet is a "shell" (probed to a newer PN), the list is empty because
    /// the frames have been moved to the probe entry.
    pub frames: Queue<Frame>,
    /// Transmission metadata shared by all frames in this packet (CCA info, send time,
    /// wire byte count). Taken on first ACK or loss so that RTT/CCA updates are applied
    /// exactly once.
    pub transmission_info: Option<TransmissionInfo>,
    /// PTO probe chain forward pointer.
    ///
    /// When a PTO fires and the assembler retransmits this packet's frames under a new
    /// packet number, `probed_to` is set to that new PN and `frames` is emptied (this
    /// entry becomes a "shell"). The chain can extend across multiple PTO firings:
    ///
    /// ```text
    /// PN_0 (shell, probed_to=PN_1) -> PN_1 (shell, probed_to=PN_2) -> PN_2 (live frames)
    /// ```
    ///
    /// ACK processing follows the chain to the tail to complete the frames found there.
    /// Loss detection on a shell calls `on_packet_lost` for CCA but does not follow the
    /// chain — the probe is still in flight and may succeed independently.
    pub probed_to: Option<PacketNumber>,
}

impl Packet {
    pub fn new(frames: Queue<Frame>, info: TransmissionInfo) -> Self {
        // All stored frames must be ack-eliciting (ACK/control frames are stripped
        // before insertion by the assembler).
        #[cfg(debug_assertions)]
        for frame in frames.iter() {
            debug_assert!(
                frame.header.is_ack_eliciting(),
                "non-ack-eliciting frame stored in inflight — strip before insertion"
            );
        }
        Self {
            frames,
            transmission_info: Some(info),
            probed_to: None,
        }
    }
}

/// Tracks all packets currently in flight, keyed by packet number.
pub(crate) struct Map {
    inner: Inner<Packet>,
    inflight_gauge: QueueGauge,
}

impl Map {
    #[inline]
    pub fn new(inflight_gauge: QueueGauge) -> Self {
        Self {
            inner: Default::default(),
            inflight_gauge,
        }
    }

    #[inline]
    pub fn insert(&mut self, pn: PacketNumber, entry: Packet) {
        self.inflight_gauge.enqueue(1);
        self.inner.insert(pn, entry);
    }

    #[inline]
    pub fn get_range(&self) -> PacketNumberRange {
        self.inner.get_range()
    }

    /// Remove a range of ACKed packet numbers.
    ///
    /// Returns an iterator of (PacketNumber, Packet) for further processing
    /// (completion notifications, CCA updates).
    #[inline]
    pub fn remove_range(
        &mut self,
        range: PacketNumberRange,
    ) -> impl Iterator<Item = (VarInt, Packet)> + '_ {
        RemoveRange {
            inner: self.inner.remove_range(range),
            gauge: &self.inflight_gauge,
        }
    }

    #[inline]
    pub fn has_inflight(&self) -> bool {
        !self.inner.is_empty()
    }

    #[inline]
    pub fn max_packet_number(&self) -> Option<VarInt> {
        if !self.has_inflight() {
            return None;
        }

        let max = self.inner.get_range().end().as_u64();
        // SAFETY: packet numbers are encoded as QUIC varints.
        Some(unsafe { VarInt::new_unchecked(max) })
    }

    /// Returns the largest packet number in the contiguous lost prefix, if any.
    ///
    /// The scan stops as soon as a packet is not considered lost and therefore only
    /// walks the front prefix instead of the full map.
    #[inline]
    pub fn loss_cutoff(
        &self,
        largest_acked: PacketNumber,
        pn_threshold: Option<PacketNumber>,
        time_threshold: Option<s2n_quic_core::time::Timestamp>,
    ) -> Option<PacketNumber> {
        self.inner
            .contiguous_prefix_cutoff(largest_acked, |pn, packet| {
                let lost_by_pn = pn_threshold.is_some_and(|threshold| pn <= threshold);
                let lost_by_time = time_threshold
                    .zip(packet.transmission_info.as_ref())
                    .is_some_and(|(threshold, tx_info)| tx_info.time_sent <= threshold);
                lost_by_pn || lost_by_time
            })
    }

    /// Find the oldest inflight packet number that has data frames available for probing.
    ///
    /// Returns `None` if all inflight entries are shells or if the map is empty.
    #[inline]
    pub fn oldest_non_shell_pn(&self) -> Option<PacketNumber> {
        self.inner
            .iter()
            .find(|(_, p)| !p.frames.is_empty())
            .map(|(pn, _)| pn)
    }

    /// Take the frames from the oldest non-shell inflight entry for a PTO probe.
    ///
    /// The entry remains in the map with an empty `frames` list and its
    /// `TransmissionInfo` intact. The caller must then call
    /// [`set_probed_to_and_take_bytes`] to finalise the shell pointer and
    /// release the shell's bytes from the CCA.
    ///
    /// [`set_probed_to_and_take_bytes`]: Self::set_probed_to_and_take_bytes
    #[inline]
    pub fn take_oldest_for_probe(&mut self) -> Option<(PacketNumber, Queue<Frame>)> {
        let old_pn = self.oldest_non_shell_pn()?;
        let packet = self.inner.get_mut(old_pn)?;
        let frames = core::mem::take(&mut packet.frames);
        Some((old_pn, frames))
    }

    /// Restore frames back into an inflight entry after a failed probe attempt.
    ///
    /// Used when the probe frame doesn't fit in the current segment (e.g. because
    /// ACK frames already consumed the budget). The entry reverts from a shell back
    /// to a live entry so it can be probed on the next opportunity.
    #[inline]
    pub fn restore_probe_frames(&mut self, pn: PacketNumber, frames: Queue<Frame>) {
        let packet = self
            .inner
            .get_mut(pn)
            .expect("inflight entry must exist when restoring probe frames");
        debug_assert!(packet.frames.is_empty());
        debug_assert!(packet.probed_to.is_none());
        packet.frames = frames;
    }

    /// Verify structural invariants of the inflight map.
    ///
    /// Each stored packet must either have a `probed_to` link (shell) **or** contain
    /// non-empty, all-ack-eliciting frames. A packet with only ACK frames and no
    /// `probed_to` could trigger an ACK loop.
    ///
    /// The O(N × F) loop over all frames is only compiled in test builds. Cheaper
    /// per-entry checks can be added outside the `#[cfg(test)]` guard in the future.
    #[inline]
    pub fn invariants(&self) {
        #[cfg(test)]
        for (_, packet) in self.inner.iter() {
            if packet.probed_to.is_none() {
                assert!(
                    !packet.frames.is_empty(),
                    "inflight packet has no probed_to link and no frames — potential ACK loop"
                );
                for frame in packet.frames.iter() {
                    assert!(
                        frame.header.is_ack_eliciting(),
                        "non-ack-eliciting frame stored in inflight — strip before insertion"
                    );
                }
            }
        }
    }

    /// Sum the `sent_bytes` of all inflight entries that still have transmission info.
    #[inline]
    pub fn sum_sent_bytes(&self) -> u32 {
        self.inner
            .iter()
            .filter_map(|(_, p)| p.transmission_info.as_ref())
            .map(|info| info.sent_bytes as u32)
            .sum()
    }

    /// Remove a single packet from the map (e.g. when all its frames are cancelled).
    #[inline]
    pub fn remove(&mut self, pn: PacketNumber) -> Option<Packet> {
        let packet = self.inner.remove(pn)?;
        self.inflight_gauge.dequeue();
        Some(packet)
    }

    /// Drop every remaining entry once no bytes are in flight.
    ///
    /// When `bytes_in_flight` reaches zero, the byte-accounting invariant
    /// (`sum_sent_bytes == cca.bytes_in_flight`) guarantees every entry still in the
    /// map is a zero-byte shell: a live entry always carries `sent_bytes > 0`. Those
    /// shells are PTO-probe tombstones whose live tail is already gone (ACKed, lost,
    /// or cancelled), so they can never resolve to a completion or carry a probe —
    /// they only keep `has_inflight()` true and the PTO armed forever. The caller is
    /// responsible for confirming `bytes_in_flight == 0` before calling this.
    #[inline]
    pub fn clear_orphaned_shells(&mut self) {
        let count = self.inner.len() as u64;
        if count == 0 {
            return;
        }
        // SAFETY (logical, not memory): the caller guarantees `bytes_in_flight == 0`.
        // Combined with the accounting invariant `sum_sent_bytes == cca.bytes_in_flight`,
        // every remaining entry must have `sent_bytes == 0` (a live entry always carries
        // bytes), so clearing drops only shells. A violation here means a prior bug in the
        // byte accounting (on_packet_sent/ack/lost/discarded); the debug_assert surfaces it
        // in development. In release builds a violation would silently drop a live entry, so
        // the byte invariant in `send::Context::invariants` is the real guard.
        debug_assert!(
            self.inner.iter().all(|(_, p)| p.transmission_info.is_none()
                || p.transmission_info
                    .as_ref()
                    .is_some_and(|i| i.sent_bytes == 0)),
            "clear_orphaned_shells called with non-shell entries still in the map"
        );
        self.inner.clear();
        self.inflight_gauge.dequeue_n(count);
    }

    /// Set the `probed_to` forward pointer on an existing inflight entry and
    /// release its `sent_bytes` from `bytes_in_flight` accounting.
    ///
    /// Called after a probe segment is successfully encoded: the `old_pn` entry
    /// becomes a shell pointing to `new_pn` (the probe's packet number). The new
    /// probe packet already called `on_packet_sent()` with its own bytes, so the
    /// shell's bytes must be released to avoid double-counting in `bytes_in_flight`.
    ///
    /// The `transmission_info` is preserved (with `sent_bytes` zeroed) so that if
    /// the peer ACKs the shell's PN, the CCA/RTT estimator still receives the
    /// correct `cc_info` and `time_sent` for delivery rate sampling.
    ///
    /// Returns the number of bytes released (0 if the entry was not found or had
    /// no transmission info).
    #[inline]
    pub fn set_probed_to_and_take_bytes(
        &mut self,
        old_pn: PacketNumber,
        new_pn: PacketNumber,
    ) -> usize {
        if let Some(packet) = self.inner.get_mut(old_pn) {
            debug_assert!(
                packet.frames.is_empty(),
                "set_probed_to_and_take_bytes: old entry still has frames; \
                 take_oldest_for_probe should have taken them first"
            );
            packet.probed_to = Some(new_pn);
            if let Some(ref mut tx_info) = packet.transmission_info {
                let bytes = tx_info.sent_bytes as usize;
                tx_info.sent_bytes = 0;
                return bytes;
            }
        }
        0
    }

    /// Follow the `probed_to` chain starting at `pn`, remove every entry in the
    /// chain, and return the frames from the tail.
    ///
    /// Used in ACK processing when a shell is ACKed: the frames to complete live at
    /// the tail of the probe chain. All intermediate shells and the tail itself are
    /// removed from the map so no zombie entries remain.
    ///
    /// Returns the tail's frames and the total `sent_bytes` of all removed entries
    /// that still had `transmission_info`. The caller must release these bytes from
    /// the CCA via `on_packet_discarded`.
    #[inline]
    pub fn remove_chain(&mut self, mut pn: PacketNumber) -> ChainRemoval {
        let mut frames = Queue::new();
        let mut discarded_bytes: usize = 0;

        loop {
            match self.inner.remove(pn) {
                Some(packet) => {
                    self.inflight_gauge.dequeue();
                    if let Some(tx_info) = &packet.transmission_info {
                        discarded_bytes += tx_info.sent_bytes as usize;
                    }
                    if let Some(next_pn) = packet.probed_to {
                        pn = next_pn;
                    } else {
                        frames = packet.frames;
                        break;
                    }
                }
                None => break,
            }
        }

        ChainRemoval {
            frames,
            discarded_bytes,
        }
    }
}

#[must_use = "discarded_bytes must be released from the CCA via on_packet_discarded"]
pub(crate) struct ChainRemoval {
    pub frames: Queue<Frame>,
    pub discarded_bytes: usize,
}

pub(crate) struct RemoveRange<'a, I> {
    inner: I,
    gauge: &'a QueueGauge,
}

impl<'a, I> Iterator for RemoveRange<'a, I>
where
    I: Iterator<Item = (PacketNumber, Packet)>,
{
    type Item = (VarInt, Packet);

    fn next(&mut self) -> Option<Self::Item> {
        let (num, packet) = self.inner.next()?;
        self.gauge.dequeue();
        let num = unsafe { VarInt::new_unchecked(num.as_u64()) };
        Some((num, packet))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        byte_vec::ByteVec,
        endpoint::frame::{Frame, Header, TransmissionStatus, DEFAULT_TTL},
        packet::datagram::QueuePair,
        path::secret::map::Entry as PathSecretEntry,
    };
    use core::time::Duration;
    use s2n_quic_core::{
        packet::number::PacketNumberSpace, recovery::RttEstimator, varint::VarInt,
    };
    use std::sync::Arc;

    fn make_gauge() -> QueueGauge {
        let registry = crate::counter::Registry::new();
        registry.register_queue_gauge("test.inflight")
    }

    fn make_pn(n: u64) -> PacketNumber {
        PacketNumberSpace::Initial.new_packet_number(VarInt::new(n).unwrap())
    }

    fn fake_entry() -> Arc<PathSecretEntry> {
        PathSecretEntry::builder("127.0.0.1:9999".parse().unwrap()).build()
    }

    /// Create a Packet containing one QueueData (ack-eliciting) frame.
    fn make_packet(entry: Arc<PathSecretEntry>) -> Packet {
        make_packet_at(entry, Duration::from_millis(100))
    }

    fn make_packet_at(entry: Arc<PathSecretEntry>, at: Duration) -> Packet {
        let mut frames = Queue::new();
        let mut payload = ByteVec::new();
        payload.push_back(bytes::Bytes::from_static(b"x"));
        let frame = Frame {
            header: Header::QueueData {
                queue_pair: QueuePair {
                    source_queue_id: VarInt::from_u8(1),
                    dest_queue_id: VarInt::from_u8(2),
                },
                binding_id: VarInt::from_u8(1),
                offset: VarInt::ZERO,
                is_fin: false,
                dest_acceptor_id: None,
                priority: crate::credit::Priority::default(),
            },
            payload,
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            enqueued_at: None,
            flow_credits: 0,
        };
        frames.push_back(frame.into());

        let mut cca = crate::congestion::Controller::new(1500);
        let rtt = RttEstimator::new(Duration::from_millis(2));
        let now = unsafe { s2n_quic_core::time::Timestamp::from_duration(at) };
        let cc_info = cca.on_packet_sent(now, 100, false, &rtt);
        Packet::new(
            frames,
            TransmissionInfo {
                cc_info,
                time_sent: now,
                sent_bytes: 100,
            },
        )
    }

    // ── basic insertion / has_inflight ────────────────────────────────────────

    #[test]
    fn empty_map_has_no_inflight() {
        let map = Map::new(make_gauge());
        assert!(!map.has_inflight());
        assert!(map.oldest_non_shell_pn().is_none());
    }

    #[test]
    fn insert_single_packet_has_inflight() {
        let mut map = Map::new(make_gauge());
        let pn = make_pn(1);
        map.insert(pn, make_packet(fake_entry()));
        assert!(map.has_inflight());
        assert_eq!(map.oldest_non_shell_pn(), Some(pn));
    }

    #[test]
    fn oldest_non_shell_pn_returns_lowest_pn() {
        let mut map = Map::new(make_gauge());
        let pn1 = make_pn(1);
        let pn2 = make_pn(5);
        map.insert(pn1, make_packet(fake_entry()));
        map.insert(pn2, make_packet(fake_entry()));
        // Should return the lowest (oldest) PN
        assert_eq!(map.oldest_non_shell_pn(), Some(pn1));
    }

    #[test]
    fn loss_cutoff_uses_time_threshold_prefix() {
        let mut map = Map::new(make_gauge());
        let pn1 = make_pn(1);
        let pn2 = make_pn(2);
        let pn3 = make_pn(3);
        map.insert(
            pn1,
            make_packet_at(fake_entry(), Duration::from_millis(100)),
        );
        map.insert(
            pn2,
            make_packet_at(fake_entry(), Duration::from_millis(104)),
        );
        map.insert(
            pn3,
            make_packet_at(fake_entry(), Duration::from_millis(110)),
        );

        let threshold =
            unsafe { s2n_quic_core::time::Timestamp::from_duration(Duration::from_millis(105)) };
        assert_eq!(map.loss_cutoff(pn3, None, Some(threshold)), Some(pn2));
    }

    // ── take_oldest_for_probe ─────────────────────────────────────────────────

    #[test]
    fn take_oldest_for_probe_empty_map() {
        let mut map = Map::new(make_gauge());
        assert!(map.take_oldest_for_probe().is_none());
    }

    #[test]
    fn take_oldest_for_probe_returns_frames_and_makes_shell() {
        let mut map = Map::new(make_gauge());
        let pn = make_pn(3);
        map.insert(pn, make_packet(fake_entry()));

        let (taken_pn, frames) = map.take_oldest_for_probe().unwrap();
        assert_eq!(taken_pn, pn);
        assert!(!frames.is_empty(), "frames should have been returned");

        // Entry is still in the map (it is now a shell with empty frames)
        assert!(map.has_inflight(), "shell entry remains in map");
        assert!(
            map.oldest_non_shell_pn().is_none(),
            "no more non-shell entries"
        );
    }

    #[test]
    fn take_oldest_for_probe_picks_non_shell_oldest() {
        let mut map = Map::new(make_gauge());
        let pn1 = make_pn(1);
        let pn2 = make_pn(10);
        map.insert(pn1, make_packet(fake_entry()));
        map.insert(pn2, make_packet(fake_entry()));

        // Make pn1 a shell (take its frames and don't re-insert)
        let (_old_pn, _frames) = map.take_oldest_for_probe().unwrap(); // takes pn1
        map.set_probed_to_and_take_bytes(pn1, pn2); // link shell → pn2

        // Now pn1 is a shell; the only non-shell is pn2
        assert_eq!(map.oldest_non_shell_pn(), Some(pn2));
        // take_oldest_for_probe should now return pn2
        let (taken_pn, frames) = map.take_oldest_for_probe().unwrap();
        assert_eq!(taken_pn, pn2);
        assert!(!frames.is_empty());
    }

    #[test]
    fn take_oldest_for_probe_all_shells_returns_none() {
        let mut map = Map::new(make_gauge());
        let pn1 = make_pn(1);
        let pn2 = make_pn(10);
        map.insert(pn1, make_packet(fake_entry()));
        map.insert(pn2, make_packet(fake_entry()));

        // Make both shells
        map.take_oldest_for_probe(); // empties pn1's frames
        map.set_probed_to_and_take_bytes(pn1, pn2);
        map.take_oldest_for_probe(); // empties pn2's frames
                                     // pn2 has no probed_to yet — take_oldest_for_probe should still return None
                                     // because frames are empty

        assert!(map.oldest_non_shell_pn().is_none());
        assert!(map.take_oldest_for_probe().is_none());
    }

    // ── set_probed_to / remove_chain ───────────────────────────────────────────

    #[test]
    fn remove_chain_single_hop() {
        let mut map = Map::new(make_gauge());
        let pn_old = make_pn(1);
        let pn_new = make_pn(10);
        map.insert(pn_old, make_packet(fake_entry()));

        // Simulate probe assembly
        let (_old, frames) = map.take_oldest_for_probe().unwrap();
        let mut cca = crate::congestion::Controller::new(1500);
        let rtt = RttEstimator::new(Duration::from_millis(2));
        let now =
            unsafe { s2n_quic_core::time::Timestamp::from_duration(Duration::from_millis(200)) };
        let cc_info = cca.on_packet_sent(now, 100, false, &rtt);
        map.insert(
            pn_new,
            Packet::new(
                frames,
                TransmissionInfo {
                    cc_info,
                    time_sent: now,
                    sent_bytes: 100,
                },
            ),
        );
        map.set_probed_to_and_take_bytes(pn_old, pn_new);

        // remove_chain should follow shell → probe, remove probe, and return its frames
        let removal = map.remove_chain(pn_old);
        assert!(
            !removal.frames.is_empty(),
            "tail frames should be non-empty"
        );
        // Both entries should be removed
        assert!(!map.has_inflight());
    }

    #[test]
    fn remove_chain_already_removed_tail_returns_empty() {
        // Shell → probe, but probe was already ACKed and removed from the map.
        let mut map = Map::new(make_gauge());
        let pn_old = make_pn(1);
        let pn_new = make_pn(10);
        map.insert(pn_old, make_packet(fake_entry()));

        let (_old, frames) = map.take_oldest_for_probe().unwrap();
        let mut cca = crate::congestion::Controller::new(1500);
        let rtt = RttEstimator::new(Duration::from_millis(2));
        let now =
            unsafe { s2n_quic_core::time::Timestamp::from_duration(Duration::from_millis(200)) };
        let cc_info = cca.on_packet_sent(now, 100, false, &rtt);
        map.insert(
            pn_new,
            Packet::new(
                frames,
                TransmissionInfo {
                    cc_info,
                    time_sent: now,
                    sent_bytes: 100,
                },
            ),
        );
        map.set_probed_to_and_take_bytes(pn_old, pn_new);

        // Simulate the probe PN being removed before the shell is processed.
        let range = s2n_quic_core::packet::number::PacketNumberRange::new(pn_new, pn_new);
        let _removed: Vec<_> = map.remove_range(range).collect();

        // remove_chain on the shell walks to pn_new which is gone → empty queue
        let removal = map.remove_chain(pn_old);
        assert!(
            removal.frames.is_empty(),
            "tail already removed; frames should be empty"
        );
    }

    #[test]
    fn remove_chain_multi_hop() {
        // Chain: pn1 (shell) → pn10 (shell) → pn20 (tail with frames)
        let mut map = Map::new(make_gauge());
        let pn1 = make_pn(1);
        let pn10 = make_pn(10);
        let pn20 = make_pn(20);

        let now =
            unsafe { s2n_quic_core::time::Timestamp::from_duration(Duration::from_millis(100)) };
        let mut cca = crate::congestion::Controller::new(1500);
        let rtt = RttEstimator::new(Duration::from_millis(2));

        map.insert(pn1, make_packet(fake_entry()));

        // First probe: pn1 → pn10
        let (_old1, frames1) = map.take_oldest_for_probe().unwrap();
        let cc1 = cca.on_packet_sent(now, 100, false, &rtt);
        map.insert(
            pn10,
            Packet::new(
                frames1,
                TransmissionInfo {
                    cc_info: cc1,
                    time_sent: now,
                    sent_bytes: 100,
                },
            ),
        );
        map.set_probed_to_and_take_bytes(pn1, pn10);

        // Second probe: pn10 → pn20
        let (_old10, frames10) = map.take_oldest_for_probe().unwrap();
        let cc2 = cca.on_packet_sent(now, 100, false, &rtt);
        map.insert(
            pn20,
            Packet::new(
                frames10,
                TransmissionInfo {
                    cc_info: cc2,
                    time_sent: now,
                    sent_bytes: 100,
                },
            ),
        );
        map.set_probed_to_and_take_bytes(pn10, pn20);

        // remove_chain from pn1 should walk pn1 → pn10 → pn20, remove all, return pn20's frames
        let removal = map.remove_chain(pn1);
        assert!(!removal.frames.is_empty(), "pn20 frames should be present");
        // All three entries removed
        assert!(!map.has_inflight());
    }

    // ── invariants ────────────────────────────────────────────────────────────

    #[test]
    fn invariants_passes_for_valid_packets() {
        let mut map = Map::new(make_gauge());
        map.insert(make_pn(1), make_packet(fake_entry()));
        map.insert(make_pn(2), make_packet(fake_entry()));
        // Should not panic
        map.invariants();
    }

    #[test]
    fn invariants_passes_for_valid_shell() {
        let mut map = Map::new(make_gauge());
        let pn1 = make_pn(1);
        let pn2 = make_pn(5);
        map.insert(pn1, make_packet(fake_entry()));
        map.insert(pn2, make_packet(fake_entry()));
        map.take_oldest_for_probe(); // makes pn1 a shell with empty frames
        map.set_probed_to_and_take_bytes(pn1, pn2); // now pn1 is a valid shell (probed_to is Some)
                                                    // Should not panic: pn1 has probed_to, pn2 has non-empty frames
        map.invariants();
    }

    /// Verifies that repeated probes without ACKs do NOT inflate bytes_in_flight.
    ///
    /// This is the core regression test for the strss-1-dcquic production issue:
    /// each probe must release the previous shell's bytes so that bytes_in_flight
    /// equals only the latest probe's size, not the sum of all historical probes.
    #[test]
    fn repeated_probes_do_not_inflate_bytes_in_flight() {
        let mut map = Map::new(make_gauge());
        let mut cca = crate::congestion::Controller::new(1500);
        let rtt = RttEstimator::new(Duration::from_millis(2));
        let now =
            unsafe { s2n_quic_core::time::Timestamp::from_duration(Duration::from_millis(100)) };

        const PACKET_SIZE: u16 = 100;
        const NUM_PROBES: usize = 50;

        // Initial packet — use make_packet's frame construction pattern
        let pn0 = make_pn(0);
        let initial_packet = make_packet(fake_entry());
        let cc_info = cca.on_packet_sent(now, PACKET_SIZE, false, &rtt);
        map.insert(
            pn0,
            Packet::new(
                initial_packet.frames,
                TransmissionInfo {
                    cc_info,
                    time_sent: now,
                    sent_bytes: PACKET_SIZE,
                },
            ),
        );
        assert_eq!(cca.bytes_in_flight(), PACKET_SIZE as u32);

        // Simulate N probes without any ACK arriving
        let mut prev_pn = pn0;
        for i in 1..=NUM_PROBES {
            let new_pn = make_pn((i * 10) as u64);

            // PTO fires: take frames from oldest non-shell
            let (old_pn, frames) = map.take_oldest_for_probe().unwrap();
            assert_eq!(old_pn, prev_pn);

            // Assemble new probe packet (calls on_packet_sent)
            let cc_info = cca.on_packet_sent(now, PACKET_SIZE, false, &rtt);
            map.insert(
                new_pn,
                Packet::new(
                    frames,
                    TransmissionInfo {
                        cc_info,
                        time_sent: now,
                        sent_bytes: PACKET_SIZE,
                    },
                ),
            );

            // Link shell → new probe and release shell bytes
            let discarded = map.set_probed_to_and_take_bytes(old_pn, new_pn);
            if discarded > 0 {
                cca.on_packet_discarded(discarded);
            }

            prev_pn = new_pn;
        }

        // After 50 probes, bytes_in_flight should still be just one packet's worth.
        // Without the fix (no discard), it would be 51 * 100 = 5100.
        assert_eq!(
            cca.bytes_in_flight(),
            PACKET_SIZE as u32,
            "bytes_in_flight should equal one packet after {} probes, \
             but got {} — shell chain is leaking bytes into the CCA",
            NUM_PROBES,
            cca.bytes_in_flight()
        );

        // sum_sent_bytes should match (only the live entry has transmission_info)
        assert_eq!(map.sum_sent_bytes(), PACKET_SIZE as u32);
    }
}
