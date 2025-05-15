// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    buffer::{Buffer, Viewer},
    FinState, FrameWriter, OutgoingDataFlowController, State,
};
use crate::contexts::{OnTransmitError, WriteContext};
use core::num::NonZeroU16;
use s2n_quic_core::{
    ack,
    interval_set::{Interval, IntervalSet},
    packet::number::{Map as PacketNumberMap, PacketNumber, PacketNumberRange},
    varint::VarInt,
};

#[derive(Debug)]
pub struct Transmissions<FlowController, Writer> {
    /// Tracking information for all data in transmission
    in_flight: Set,
    //= https://www.rfc-editor.org/rfc/rfc9000#section-4.4
    //# Both endpoints MUST maintain flow control state
    //# for the stream in the unterminated direction until that direction
    //# enters a terminal state.
    /// The flow controller which is used to determine whether data chunks can
    /// be sent.
    pub flow_controller: FlowController,
    /// Serializes chunks into frames and writes the frames
    writer: Writer,
}

impl<FlowController: OutgoingDataFlowController, Writer: FrameWriter>
    Transmissions<FlowController, Writer>
{
    #[inline]
    pub fn new(flow_controller: FlowController) -> Self {
        Self {
            in_flight: Default::default(),
            flow_controller,
            writer: Default::default(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.in_flight.is_empty()
    }

    #[inline]
    pub fn on_ack_signal<Set: ack::Set, F: FnMut(Interval<VarInt>)>(
        &mut self,
        ack_set: &Set,
        mut on_range: F,
    ) -> bool {
        let mut changed = false;

        let range = ack_set.as_range();

        for range in self.in_flight.remove_range(range) {
            on_range(range);
            changed = true;
        }

        changed
    }

    #[inline]
    pub fn transmit_set<W: WriteContext>(
        &mut self,
        buffer: &Buffer,
        set: &mut IntervalSet<VarInt>,
        state: &mut State,
        writer_context: Writer::Context,
        context: &mut W,
    ) -> Result<bool, OnTransmitError> {
        // make sure we've got something to transmit
        if set.is_empty() {
            return Ok(false);
        }

        let mut viewer = buffer.viewer();

        let mut has_transmitted = false;
        while let Some(mut interval) = set.pop_min() {
            match self.transmit_interval(&mut viewer, interval, state, writer_context, context) {
                Ok(transmitted) => {
                    has_transmitted = true;
                    let len = transmitted.len();
                    if len != interval.len() {
                        // only a part of the range was written so push back what wasn't
                        interval =
                            (interval.start_inclusive() + len..=interval.end_inclusive()).into();
                        debug_assert!(interval.is_valid());
                        set.insert_front(interval).unwrap();
                        return Ok(has_transmitted);
                    }
                }
                Err(err) => {
                    // if the interval failed to transmit it, put it back
                    set.insert_front(interval).unwrap();
                    return Err(err);
                }
            }
        }

        Ok(has_transmitted)
    }

    #[inline]
    pub fn transmit_interval<W: WriteContext>(
        &mut self,
        viewer: &mut Viewer,
        mut interval: Interval<VarInt>,
        state: &mut State,
        writer_context: Writer::Context,
        context: &mut W,
    ) -> Result<Interval<VarInt>, OnTransmitError> {
        // First trim the range to the current packet buffer
        let capacity = context.remaining_capacity();
        // Bound the capacity to u16::MAX - UDP payloads can't be larger anyway
        let capacity = capacity.min(u16::MAX as _);
        let mut interval_len = interval.len();

        // ensure the current packet buffer meets our minimum requirements
        if capacity == 0
            || interval_len >= Writer::MIN_WRITE_SIZE && capacity < Writer::MIN_WRITE_SIZE
            || !self.in_flight.has_capacity()
        {
            return Err(OnTransmitError::CouldNotAcquireEnoughSpace);
        }

        if capacity < interval_len {
            interval.set_len(capacity);
            interval_len = capacity;
        }

        let window_len = {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-2.2
            //# An endpoint MUST NOT send data on any stream without ensuring that it
            //# is within the flow control limits set by its peer.

            //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
            //# Senders MUST NOT send data in excess of either limit.
            self.flow_controller
                .acquire_flow_control_window(interval.end_exclusive())
                .checked_sub(interval.start_inclusive())
                .ok_or(OnTransmitError::CouldNotAcquireEnoughSpace)?
                .as_u64()
        };

        // ensure the window is non-zero
        if window_len == 0 {
            return Err(OnTransmitError::CouldNotAcquireEnoughSpace);
        }

        if window_len < interval_len as u64 {
            interval.set_len(window_len as usize);
        }

        let packet_number = context.packet_number();
        let mut view = viewer.next_view(interval, matches!(state, State::Finishing(_)));

        self.writer
            .write_chunk(
                interval.start_inclusive(),
                &mut view,
                writer_context,
                context,
            )
            .map_err(|_| OnTransmitError::CouldNotAcquireEnoughSpace)?;

        let len = view.len();
        debug_assert_ne!(len, 0u64, "cannot transmit an empty payload");

        interval.set_len(len.as_u64() as usize);

        debug_assert!(interval.is_valid());

        self.in_flight
            .insert(packet_number, interval.start_inclusive(), len);

        // Piggyback a fin transmission if we can
        if Writer::WRITES_FIN && view.is_fin() {
            if let Some(state) = state.fin_state_mut() {
                state.on_transmit(packet_number);
            }
        }

        Ok(interval)
    }

    #[inline]
    pub fn transmit_fin<W: WriteContext>(
        &mut self,
        buffer: &Buffer,
        state: &mut State,
        writer_context: Writer::Context,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        // make sure we're not blocked before transmitting the fin bit
        if self.flow_controller.is_blocked() {
            return Err(OnTransmitError::CouldNotWriteFrame);
        }

        if let Some(state) = state.fin_state_mut() {
            if matches!(state, FinState::Pending | FinState::Lost) {
                let packet_number = context.packet_number();

                self.writer
                    .write_fin(buffer.total_len(), writer_context, context)
                    .map_err(|_| OnTransmitError::CouldNotAcquireEnoughSpace)?;

                state.on_transmit(packet_number);
            }
        }

        Ok(())
    }

    /// Remove all inflight transmissions
    #[inline]
    pub fn clear(&mut self) {
        self.in_flight.clear();
    }

    /// Remove all inflight transmissions and finish the flow controller
    #[inline]
    pub fn finish(&mut self) {
        self.clear();
        self.flow_controller.finish();
    }

    /// Get the inflight inclusive PacketNumberRange
    #[inline]
    pub fn get_inflight_range(&self) -> PacketNumberRange {
        self.in_flight.packets.get_range()
    }
}

/// Describes a chunk of bytes which has to be transmitted to the peer
#[derive(Copy, Clone, Debug)]
struct Transmission {
    /// The range of data that was sent in this transmission
    offset: VarInt,
    /// The length of data that was sent in the transmission
    len: u16,
    /// An optional next transmission in the same packet
    next: Option<TransmissionId>,
}

impl Transmission {
    #[inline]
    pub fn range(&self) -> Interval<VarInt> {
        (self.offset..self.offset + VarInt::from_u16(self.len)).into()
    }
}

#[derive(Debug, Default)]
struct Set {
    /// The packets that are currently in flight
    packets: PacketNumberMap<Transmission>,
    /// A slab of transmission ranges
    ///
    /// Because a packet number can have more than one transmission range,
    /// we need to store any extras outside of the map
    overflow: TransmissionSlab,
}

impl Set {
    #[inline]
    pub fn insert(&mut self, packet_number: PacketNumber, offset: VarInt, len: VarInt) {
        debug_assert!(len <= u16::MAX as u64);

        let transmission = Transmission {
            offset,
            len: len.as_u64() as _,
            next: None,
        };

        let transmissions = &mut self.overflow;

        self.packets
            .insert_or_update(packet_number, transmission, |prev| {
                // if we already have a entry for this packet number then chain the transmissions
                // together
                let idx = transmissions.insert(transmission);

                if let Some(prev) = prev.next {
                    transmissions.chain(prev, idx);
                } else {
                    prev.next = Some(idx);
                }
            });
    }

    #[inline]
    pub fn remove_range(&mut self, range: PacketNumberRange) -> SetRemoveIter {
        SetRemoveIter {
            inner: self.packets.remove_range(range),
            next: None,
            transmissions: &mut self.overflow,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    #[inline]
    pub fn has_capacity(&self) -> bool {
        self.overflow.has_capacity()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.packets.clear();
        self.overflow.clear();
    }
}

struct SetRemoveIter<'a> {
    inner: s2n_quic_core::packet::number::map::RemoveIter<'a, Transmission>,
    next: Option<TransmissionId>,
    transmissions: &'a mut TransmissionSlab,
}

impl Iterator for SetRemoveIter<'_> {
    type Item = Interval<VarInt>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(idx) = self.next.take() {
            let transmission = self.transmissions.remove(idx);
            self.next = transmission.next;
            return Some(transmission.range());
        }

        let (_, transmission) = self.inner.next()?;
        self.next = transmission.next;
        Some(transmission.range())
    }
}

#[derive(Debug, Default)]
struct TransmissionSlab {
    entries: Vec<TransmissionSlabEntry>,
    len: u16,
    next_free: u16,
}

/// An index into the transmission slab
///
/// We use a NonZeroU16 so it's the same size as `Option<TransmissionId>`
#[derive(Clone, Copy, Debug)]
struct TransmissionId(NonZeroU16);

#[derive(Debug)]
struct TransmissionSlabEntry {
    transmission: Transmission,
    next_free: u16,

    #[cfg(debug_assertions)]
    /// Tracks if the entry is occupied or not
    ///
    /// This is only needed with debug_assertions since the next_free logic actually
    /// takes care of this for us. This is all assuming the caller doesn't ever double-free
    /// any entries.
    ///
    /// This field exists to make sure these invariants stay true while testing.
    occupied: bool,
}

impl TransmissionSlab {
    #[inline]
    fn insert(&mut self, transmission: Transmission) -> TransmissionId {
        debug_assert!(self.has_capacity());
        let id = self.next_free;
        let index = id as usize;
        self.len += 1;

        let new_entry = TransmissionSlabEntry {
            transmission,
            next_free: 0,

            #[cfg(debug_assertions)]
            occupied: true,
        };

        if let Some(entry) = self.entries.get_mut(index) {
            #[cfg(debug_assertions)]
            assert!(!entry.occupied);

            self.next_free = entry.next_free;
            *entry = new_entry;
        } else {
            self.entries.push(new_entry);
            self.next_free += 1;
        }

        TransmissionId(NonZeroU16::new(1 + id).unwrap())
    }

    #[inline]
    fn remove(&mut self, index: TransmissionId) -> Transmission {
        let index = index.0.get() - 1;
        let entry = &mut self.entries[index as usize];

        #[cfg(debug_assertions)]
        {
            entry.occupied = false;
        }

        entry.next_free = self.next_free;
        self.next_free = index;
        self.len -= 1;
        entry.transmission
    }

    #[inline]
    fn chain(&mut self, prev: TransmissionId, next: TransmissionId) {
        let prev_entry = self.get_mut(prev);

        #[cfg(debug_assertions)]
        assert!(prev_entry.occupied);

        let next_entry = prev_entry.transmission.next.replace(next);

        #[cfg(debug_assertions)]
        assert!(self.get_mut(next).occupied);

        self.get_mut(next).transmission.next = next_entry;
    }

    #[inline]
    fn get_mut(&mut self, idx: TransmissionId) -> &mut TransmissionSlabEntry {
        let index = (idx.0.get() - 1) as usize;

        #[cfg(debug_assertions)]
        assert!(self.entries[index].occupied);

        &mut self.entries[index]
    }

    #[inline]
    fn has_capacity(&self) -> bool {
        // we need to be able to store the correct index so it's 1 less than the max
        self.len < u16::MAX - 1
    }

    #[inline]
    fn clear(&mut self) {
        self.entries.clear();
        self.len = 0;
        self.next_free = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::{check, generator::*};

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Operation {
        Insert,
        Remove(usize),
        Clear,
    }

    #[test]
    fn transmission_slab_test() {
        check!().with_type::<Vec<Operation>>().for_each(|ops| {
            let mut subject = TransmissionSlab::default();
            let mut oracle = Vec::new();
            let mut offset = VarInt::from_u8(0);
            // keeps track of the peak len of the oracle so we can make sure it matches the slab
            // entries.len
            let mut peak_len = 0;

            for op in ops {
                match op {
                    Operation::Insert => {
                        let id = subject.insert(Transmission {
                            offset,
                            len: 1,
                            next: None,
                        });
                        oracle.push((offset, id));
                        offset += 1;
                        peak_len = peak_len.max(oracle.len());

                        assert_eq!(subject.entries.len(), peak_len);
                        assert_eq!(subject.len as usize, oracle.len());
                    }
                    Operation::Remove(index) => {
                        if oracle.is_empty() {
                            continue;
                        }

                        let index = index % oracle.len();
                        let (offset, id) = oracle.remove(index);
                        let transmission = subject.remove(id);
                        assert_eq!(offset, transmission.offset);

                        assert_eq!(subject.entries.len(), peak_len);
                        assert_eq!(subject.len as usize, oracle.len());
                    }
                    Operation::Clear => {
                        subject.clear();
                        oracle.clear();
                        peak_len = 0;
                    }
                }
            }
        });
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn size_test() {
        use core::mem::size_of;
        use insta::assert_debug_snapshot;

        assert_debug_snapshot!(
            "transmission_entry_size",
            size_of::<TransmissionSlabEntry>()
        );
        assert_eq!(
            size_of::<TransmissionId>(),
            size_of::<Option<TransmissionId>>()
        );
    }
}
