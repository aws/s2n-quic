// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Packet number map for tracking sent packets in the frame aggregation model.
//!
//! Each packet number maps to a PacketEntry containing a list of Frames and shared
//! transmission metadata. When a packet is ACKed, all constituent frames get their
//! completion notifications. When a packet is lost, frames are individually evaluated
//! for retransmission (checking TTL and should_transmit).

use crate::{congestion, counter::QueueGauge, intrusive_queue::Queue, stream3::frame::Frame};
use s2n_quic_core::{
    packet::number::{Map as Inner, PacketNumber, PacketNumberRange},
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
    pub fn new(inflight_gauge: QueueGauge) -> Self {
        Self {
            inner: Default::default(),
            inflight_gauge,
        }
    }

    pub fn insert(&mut self, pn: PacketNumber, entry: Packet) {
        self.inflight_gauge.enqueue(1);
        self.inner.insert(pn, entry);
    }

    /// Remove a range of ACKed packet numbers.
    ///
    /// Returns an iterator of (PacketNumber, Packet) for further processing
    /// (completion notifications, CCA updates).
    pub fn remove_range(
        &mut self,
        range: PacketNumberRange,
    ) -> impl Iterator<Item = (VarInt, Packet)> + '_ {
        RemoveRange {
            inner: self.inner.remove_range(range),
            gauge: &self.inflight_gauge,
        }
    }

    pub fn has_inflight(&self) -> bool {
        self.inner.iter().next().is_some()
    }

    /// Return a mutable reference to the packet at `pn`, if present.
    pub fn get_mut(&mut self, pn: PacketNumber) -> Option<&mut Packet> {
        self.inner.get_mut(pn)
    }

    /// Find the oldest inflight packet number that has data frames available for probing.
    ///
    /// Returns `None` if all inflight entries are shells or if the map is empty.
    pub fn oldest_non_shell_pn(&self) -> Option<PacketNumber> {
        self.inner
            .iter()
            .find(|(_, p)| !p.frames.is_empty())
            .map(|(pn, _)| pn)
    }

    /// Take the frames from the oldest non-shell inflight entry for a PTO probe.
    ///
    /// The entry remains in the map with an empty `frames` list and its
    /// `TransmissionInfo` intact. The caller must then call [`set_probed_to`] to
    /// finalise the shell pointer.
    ///
    /// [`set_probed_to`]: Self::set_probed_to
    pub fn take_oldest_for_probe(&mut self) -> Option<(PacketNumber, Queue<Frame>)> {
        let old_pn = self.oldest_non_shell_pn()?;
        let packet = self.inner.get_mut(old_pn)?;
        let frames = core::mem::take(&mut packet.frames);
        Some((old_pn, frames))
    }

    /// Verify structural invariants of the inflight map.
    ///
    /// Each stored packet must either have a `probed_to` link (shell) **or** contain
    /// non-empty, all-ack-eliciting frames. A packet with only ACK frames and no
    /// `probed_to` could trigger an ACK loop.
    ///
    /// The O(N × F) loop over all frames is only compiled in test builds. Cheaper
    /// per-entry checks can be added outside the `#[cfg(test)]` guard in the future.
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

    /// Set the `probed_to` forward pointer on an existing inflight entry.
    ///
    /// Called after a probe segment is successfully encoded: the `old_pn` entry
    /// becomes a shell pointing to `new_pn` (the probe's packet number).
    pub fn set_probed_to(&mut self, old_pn: PacketNumber, new_pn: PacketNumber) {
        if let Some(packet) = self.inner.get_mut(old_pn) {
            debug_assert!(
                packet.frames.is_empty(),
                "set_probed_to: old entry still has frames; \
                 take_oldest_for_probe should have taken them before calling set_probed_to"
            );
            packet.probed_to = Some(new_pn);
        }
    }

    /// Follow the `probed_to` chain starting at `pn` and take the frames from the tail.
    ///
    /// Used in ACK processing when a shell is ACKed: the frames to complete live at
    /// the tail of the probe chain. The tail entry's `frames` are emptied but the
    /// entry itself remains in the map with its `TransmissionInfo` intact for later
    /// loss detection or ACK completion.
    ///
    /// Returns `(tail_pn, frames)`. The frames queue may be empty if the tail entry
    /// was already ACKed and removed in the same ACK range (both shell and probe PN
    /// acknowledged simultaneously).
    pub fn take_chain_tail_frames(&mut self, mut pn: PacketNumber) -> (PacketNumber, Queue<Frame>) {
        // Walk the chain to the tail (first entry with no probed_to link).
        loop {
            match self.inner.get(pn).and_then(|p| p.probed_to) {
                Some(next_pn) => pn = next_pn,
                None => break,
            }
        }
        let frames = self
            .inner
            .get_mut(pn)
            .map(|p| core::mem::take(&mut p.frames))
            .unwrap_or_default();
        (pn, frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        byte_vec::ByteVec,
        packet::datagram::QueuePair,
        path::secret::map::Entry as PathSecretEntry,
        stream3::frame::{Frame, Header, TransmissionStatus, DEFAULT_TTL},
    };
    use core::time::Duration;
    use s2n_quic_core::{packet::number::PacketNumberSpace, recovery::RttEstimator, varint::VarInt};
    use std::sync::Arc;

    fn make_gauge() -> QueueGauge {
        let registry = crate::counter::Registry::new();
        registry.register_queue_gauge("test.inflight")
    }

    fn make_pn(n: u64) -> PacketNumber {
        PacketNumberSpace::Initial.new_packet_number(VarInt::new(n).unwrap())
    }

    fn fake_entry() -> Arc<PathSecretEntry> {
        PathSecretEntry::fake("127.0.0.1:9999".parse().unwrap(), None)
    }

    /// Create a Packet containing one FlowData (ack-eliciting) frame.
    fn make_packet(entry: Arc<PathSecretEntry>) -> Packet {
        let mut frames = Queue::new();
        let mut payload = ByteVec::new();
        payload.push_back(bytes::Bytes::from_static(b"x"));
        let frame = Frame {
            header: Header::FlowData {
                queue_pair: QueuePair {
                    source_queue_id: VarInt::from_u8(1),
                    dest_queue_id: VarInt::from_u8(2),
                },
                stream_id: VarInt::from_u8(1),
                offset: VarInt::ZERO,
                is_fin: false,
            },
            source_sender_id: VarInt::MAX,
            payload,
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };
        frames.push_back(frame.into());

        let mut cca = crate::congestion::Controller::new(1500);
        let rtt = RttEstimator::new(Duration::from_millis(2));
        let now =
            unsafe { s2n_quic_core::time::Timestamp::from_duration(Duration::from_millis(100)) };
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
        map.set_probed_to(pn1, pn2); // link shell → pn2

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
        map.set_probed_to(pn1, pn2);
        map.take_oldest_for_probe(); // empties pn2's frames
        // pn2 has no probed_to yet — take_oldest_for_probe should still return None
        // because frames are empty

        assert!(map.oldest_non_shell_pn().is_none());
        assert!(map.take_oldest_for_probe().is_none());
    }

    // ── set_probed_to / take_chain_tail_frames ────────────────────────────────

    #[test]
    fn set_probed_to_and_take_chain_tail_single_hop() {
        let mut map = Map::new(make_gauge());
        let pn_old = make_pn(1);
        let pn_new = make_pn(10);
        map.insert(pn_old, make_packet(fake_entry()));

        // Simulate probe assembly
        let (_old, frames) = map.take_oldest_for_probe().unwrap();
        // Insert probe packet containing the taken frames
        let mut cca = crate::congestion::Controller::new(1500);
        let rtt = RttEstimator::new(Duration::from_millis(2));
        let now =
            unsafe { s2n_quic_core::time::Timestamp::from_duration(Duration::from_millis(200)) };
        let cc_info = cca.on_packet_sent(now, 100, false, &rtt);
        let probe_packet = Packet::new(
            frames,
            TransmissionInfo {
                cc_info,
                time_sent: now,
                sent_bytes: 100,
            },
        );
        map.insert(pn_new, probe_packet);
        map.set_probed_to(pn_old, pn_new);

        // ACK the shell: chain walk should go shell → probe and return probe frames
        let (tail_pn, tail_frames) = map.take_chain_tail_frames(pn_old);
        assert_eq!(tail_pn, pn_new, "chain tail should be the probe PN");
        assert!(!tail_frames.is_empty(), "tail frames should be non-empty");
    }

    #[test]
    fn take_chain_tail_frames_already_removed_tail_returns_empty() {
        // Shell → probe, but probe was already ACKed and removed from the map.
        // take_chain_tail_frames should return an empty queue gracefully.
        let mut map = Map::new(make_gauge());
        let pn_old = make_pn(1);
        let pn_new = make_pn(10);
        map.insert(pn_old, make_packet(fake_entry()));

        let (_old, frames) = map.take_oldest_for_probe().unwrap();
        // Simulate inserting probe then ACKing both in the same ACK range
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
        map.set_probed_to(pn_old, pn_new);

        // ACK range covers pn_new first (remove_range processes in order).
        // We simulate the probe PN being removed before the shell is processed.
        let range = s2n_quic_core::packet::number::PacketNumberRange::new(pn_new, pn_new);
        let _removed: Vec<_> = map.remove_range(range).collect();

        // Now take_chain_tail_frames on the shell should return an empty queue
        let (_tail_pn, tail_frames) = map.take_chain_tail_frames(pn_old);
        assert!(
            tail_frames.is_empty(),
            "tail already removed; frames should be empty"
        );
    }

    #[test]
    fn take_chain_tail_frames_multi_hop() {
        // Chain: pn1 (shell) → pn10 (shell) → pn20 (probe, non-shell with frames)
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
        map.set_probed_to(pn1, pn10);

        // Second probe: pn10 → pn20
        let (_old10, frames10) = map.take_oldest_for_probe().unwrap(); // takes pn10's frames
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
        map.set_probed_to(pn10, pn20);

        // Chain: pn1 → pn10 → pn20. Walking from pn1 should reach pn20.
        let (tail_pn, tail_frames) = map.take_chain_tail_frames(pn1);
        assert_eq!(tail_pn, pn20, "chain tail should be pn20");
        assert!(!tail_frames.is_empty(), "pn20 frames should be present");
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
        map.set_probed_to(pn1, pn2); // now pn1 is a valid shell (probed_to is Some)
        // Should not panic: pn1 has probed_to, pn2 has non-empty frames
        map.invariants();
    }
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
