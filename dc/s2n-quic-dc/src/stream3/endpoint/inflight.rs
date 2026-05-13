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
pub(crate) struct Packet {
    /// All frames packed into this packet.
    ///
    /// When the packet is ACKed, each frame's completion notification fires. When the
    /// packet is declared lost, each frame is individually evaluated for retransmission.
    /// When this packet is a "shell" (probed to a newer PN), the list will be empty
    /// because the frames have been moved to the probe entry.
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
