use super::{
    buffer::{Buffer, Viewer},
    FinState, FrameWriter, OutgoingDataFlowController, State,
};
use crate::{
    contexts::{OnTransmitError, WriteContext},
    interval_set::{Interval, IntervalSet},
};
use core::convert::TryInto;
use s2n_quic_core::{ack, packet::number::PacketNumber, varint::VarInt};

#[derive(Debug)]
pub struct Transmissions<FlowController, Writer> {
    /// Tracking information for all data in transmission
    in_flight: Set,
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.4
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
    pub fn new(flow_controller: FlowController) -> Self {
        Self {
            in_flight: Default::default(),
            flow_controller,
            writer: Default::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.in_flight.is_empty()
    }

    pub fn on_ack_signal<Set: ack::Set, F: FnMut(Interval<VarInt>)>(
        &mut self,
        ack_set: &Set,
        mut on_range: F,
    ) -> bool {
        let mut changed = false;

        self.in_flight.retain(|packet_number, range| {
            if ack_set.contains(packet_number) {
                on_range(range);

                changed = true;

                // Remove from in flight chunks
                return false;
            }

            true
        });

        changed
    }

    pub fn transmit_set<W: WriteContext>(
        &mut self,
        buffer: &Buffer,
        set: &mut IntervalSet<VarInt>,
        state: &mut State,
        writer_context: Writer::Context,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        // make sure we've got something to transmit
        if set.is_empty() {
            return Ok(());
        }

        let mut viewer = buffer.viewer();

        while let Some(mut interval) = set.pop_min() {
            match self.transmit_interval(&mut viewer, interval, state, writer_context, context) {
                Ok(transmitted) => {
                    let len = transmitted.len();
                    if len != interval.len() {
                        // only a part of the range was written so push back what wasn't
                        interval.start += len;
                        debug_assert!(interval.is_valid());
                        set.insert_front(interval).unwrap();
                        return Ok(());
                    }
                }
                Err(err) => {
                    // if the interval failed to transmit it, put it back
                    set.insert_front(interval).unwrap();
                    return Err(err);
                }
            }
        }

        Ok(())
    }

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
        let mut interval_len = interval.len();

        // ensure the current packet buffer meets our minimum requirements
        if capacity == 0
            || interval_len >= Writer::MIN_WRITE_SIZE && capacity < Writer::MIN_WRITE_SIZE
        {
            return Err(OnTransmitError::CoundNotAcquireEnoughSpace);
        }

        if capacity < interval_len {
            interval.set_len(capacity);
            interval_len = capacity;
        }

        let window_len = {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#2.2
            //# An endpoint MUST NOT send data on any stream without ensuring that it
            //# is within the flow control limits set by its peer.

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.1
            //# Senders MUST NOT send data in excess of either limit.
            self.flow_controller
                .acquire_flow_control_window(interval.end_exclusive())
                .checked_sub(interval.start)
                .ok_or(OnTransmitError::CoundNotAcquireEnoughSpace)?
                .try_into()
                .unwrap_or(0usize)
        };

        // ensure the window is non-zero
        if window_len == 0 {
            return Err(OnTransmitError::CoundNotAcquireEnoughSpace);
        }

        if window_len < interval_len {
            interval.set_len(window_len);
        }

        let packet_number = context.packet_number();
        let mut view = viewer.next_view(interval, matches!(state, State::Finishing(_)));

        self.writer
            .write_chunk(interval.start, &mut view, writer_context, context)
            .map_err(|_| OnTransmitError::CoundNotAcquireEnoughSpace)?;

        let len = view.len();
        debug_assert_ne!(len, 0u64, "cannot transmit an empty payload");

        interval.set_len(len.as_u64() as usize);

        debug_assert!(interval.is_valid());

        self.in_flight.insert(packet_number, interval.start, len);

        // Piggyback a fin transmission if we can
        if Writer::WRITES_FIN && view.is_fin() {
            if let Some(state) = state.fin_state_mut() {
                state.on_transmit(packet_number);
            }
        }

        Ok(interval)
    }

    pub fn transmit_fin<W: WriteContext>(
        &mut self,
        buffer: &Buffer,
        state: &mut State,
        writer_context: Writer::Context,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        if let Some(state) = state.fin_state_mut() {
            if matches!(state, FinState::Pending | FinState::Lost) {
                let packet_number = context.packet_number();

                self.writer
                    .write_fin(buffer.total_len(), writer_context, context)
                    .map_err(|_| OnTransmitError::CoundNotAcquireEnoughSpace)?;

                state.on_transmit(packet_number);
            }
        }
        Ok(())
    }

    pub fn finish(&mut self) {
        self.in_flight.clear();
        self.flow_controller.finish();
    }
}

/// Describes a chunk of bytes which has to be transmitted to the peer
#[derive(Clone, Debug, PartialEq)]
struct Transmission {
    /// The range of data that was sent in this transmission
    offset: VarInt,
    len: u32,
    /// the packet number of the transmission
    packet_number: Option<PacketNumber>,
    next_free: u32,
}

impl Transmission {
    pub fn range(&self) -> Interval<VarInt> {
        (self.offset..self.offset + VarInt::from_u32(self.len)).into()
    }
}

#[derive(Debug, Default)]
struct Set {
    entries: Vec<Transmission>,
    next_free: u32,
    len: u32,
}

impl Set {
    pub fn insert(&mut self, packet_number: PacketNumber, offset: VarInt, len: VarInt) {
        let len = len.as_u64() as u32;
        let index = self.next_free as usize;
        self.len += 1;

        if let Some(entry) = self.entries.get_mut(index) {
            debug_assert!(
                entry.packet_number.is_none(),
                "cannot replace an existing entry"
            );

            self.next_free = entry.next_free;
            entry.offset = offset;
            entry.len = len;
            entry.packet_number = Some(packet_number);
        } else {
            self.entries.push(Transmission {
                offset,
                len,
                next_free: 0,
                packet_number: Some(packet_number),
            });
            self.next_free += 1;
        }
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(PacketNumber, Interval<VarInt>) -> bool,
    {
        for (index, entry) in self.entries.iter_mut().enumerate() {
            if let Some(packet_number) = entry.packet_number {
                if !f(packet_number, entry.range()) {
                    entry.next_free = self.next_free;
                    // mark this entry as "empty"
                    entry.packet_number = None;
                    self.next_free = index as u32;
                    self.len -= 1;
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.next_free = 0;
        self.len = 0;
    }
}
