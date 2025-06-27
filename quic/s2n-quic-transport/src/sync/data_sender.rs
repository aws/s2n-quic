// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::{OnTransmitError, WriteContext},
    transmission,
};
use bytes::Bytes;
use core::convert::TryInto;
use s2n_quic_core::{ack, interval_set::IntervalSet, packet::number::PacketNumber, varint::VarInt};

mod buffer;
mod traits;
mod transmissions;
pub mod writer;

pub use buffer::View;
use s2n_quic_core::stream::StreamError;
pub use traits::*;

/// Enumerates states of the [`DataSender`]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum State {
    /// Outgoing data is accepted and transmitted
    Sending,
    /// The finish procedure has been initiated. New outgoing data is no longer
    /// accepted. The stream will continue to transmit data until all outgoing
    /// data has been transmitted and acknowledged successfully.
    /// In that case the `FinishAcknowledged` state will be entered.
    Finishing(FinState),
    /// All outgoing data including the FIN flag had been acknowledged.
    /// The Stream is thereby finalized.
    Finished,
    /// Sending data was cancelled due to a Stream RESET.
    Cancelled(StreamError),
}

impl State {
    fn fin_state_mut(&mut self) -> Option<&mut FinState> {
        if let Self::Finishing(state) = self {
            Some(state)
        } else {
            None
        }
    }

    fn can_transmit_fin(&self, constraint: transmission::Constraint, is_blocked: bool) -> bool {
        match self {
            // lost frames are not blocked by flow control since we've already acquired those
            // credits on the initial transmission
            Self::Finishing(FinState::Lost) => constraint.can_retransmit(),
            Self::Finishing(FinState::Pending) => !is_blocked && constraint.can_transmit(),
            _ => false,
        }
    }

    fn is_inflight(&self) -> bool {
        matches!(self, Self::Finishing(FinState::InFlight(_)))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FinState {
    Pending,
    InFlight(PacketNumber),
    Lost,
    Acknowledged,
}

impl FinState {
    pub fn is_acknowledged(self) -> bool {
        matches!(self, Self::Acknowledged)
    }

    /// This method gets called when a packet delivery got acknowledged
    fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        if let Self::InFlight(packet) = self {
            if ack_set.contains(*packet) {
                *self = Self::Acknowledged;
            }
        }
    }

    /// This method gets called when a packet loss is reported
    ///
    /// Returns `true` if the fin bit was lost
    fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) -> bool {
        if let Self::InFlight(packet) = self {
            if ack_set.contains(*packet) {
                *self = Self::Lost;
                return true;
            }
        }

        false
    }

    fn on_transmit(&mut self, packet: PacketNumber) {
        if matches!(self, Self::Pending | Self::Lost) {
            *self = Self::InFlight(packet);
        }
    }
}

/// Manages the transmission of all `Stream` and `Crypto` data frames towards
/// the peer as long as the `Stream` has not been reset or closed.
#[derive(Debug)]
pub struct DataSender<FlowController, ChunkToFrameWriter> {
    /// The data that needs to get transmitted
    buffer: buffer::Buffer,
    /// The current transmissions for the sender
    transmissions: transmissions::Transmissions<FlowController, ChunkToFrameWriter>,
    /// The offset of data waiting to be transmitted for the first time
    transmission_offset: VarInt,
    /// All of the intervals in the buffer that are waiting to be transmitted, ACKed or lost
    pending: IntervalSet<VarInt>,
    /// All of the intervals that have been declared lost
    lost: IntervalSet<VarInt>,
    /// The maximum amount of bytes that are buffered within the sending stream.
    /// This capacity will not be exceeded - even if the remote provides us a
    /// bigger flow control window.
    max_buffer_capacity: VarInt,
    /// Whether the size of the send stream is known and a FIN flag is already
    /// enqueued.
    state: State,
}

impl<FlowController: OutgoingDataFlowController, Writer: FrameWriter>
    DataSender<FlowController, Writer>
{
    /// Creates a new `DataSender` instance.
    ///
    /// `initial_window` denotes the amount of data we are allowed to send to the
    /// peer as known through transport parameters.
    /// `maximum_buffer_capacity` is the maximum amount of data the queue will
    /// hold. If users try to enqueue more data, it will be rejected in order to
    /// provide back-pressure on the `Stream`.
    pub fn new(flow_controller: FlowController, max_buffer_capacity: u32) -> Self {
        Self {
            buffer: Default::default(),
            transmissions: transmissions::Transmissions::new(flow_controller),
            transmission_offset: VarInt::from_u32(0),
            pending: IntervalSet::new(),
            lost: IntervalSet::new(),
            max_buffer_capacity: VarInt::from_u32(max_buffer_capacity),
            state: State::Sending,
        }
    }

    /// Declares all inflight packets as lost.
    pub fn on_all_lost(&mut self) {
        self.on_packet_loss(&self.transmissions.get_inflight_range());
    }

    /// Creates a new `DataSender` instance in its final
    /// [`DataSenderState::FinishAcknowledged`] state.
    pub fn new_finished(flow_controller: FlowController, max_buffer_capacity: u32) -> Self {
        let mut result = Self::new(flow_controller, max_buffer_capacity);
        result.state = State::Finished;
        result
    }

    /// Returns the flow controller for this `DataSender`
    pub fn flow_controller(&self) -> &FlowController {
        &self.transmissions.flow_controller
    }

    /// Returns the flow controller for this `DataSender`
    pub fn flow_controller_mut(&mut self) -> &mut FlowController {
        &mut self.transmissions.flow_controller
    }

    /// Stops sending out outgoing data.
    ///
    /// This is a one-way operation - sending can not be resumed.
    ///
    /// Calling the method removes all pending outgoing data as well as
    /// all tracking information from the buffer.
    pub fn stop_sending(&mut self, error: StreamError) {
        if self.state == State::Finished {
            return;
        }

        self.state = State::Cancelled(error);
        self.buffer.clear();
        self.pending.clear();
        self.lost.clear();
        self.transmissions.finish();
        self.transmission_offset = VarInt::from_u8(0);
        self.check_integrity();
    }

    /// Returns the amount of bytes that have ever been enqueued for writing on
    /// this Stream. This equals the offset of the highest enqueued byte + 1.
    pub fn total_enqueued_len(&self) -> VarInt {
        self.buffer.total_len()
    }

    /// Returns true if the data sender doesn't have any data enqueued for sending
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the state of the sender
    pub fn state(&self) -> State {
        self.state
    }

    /// Returns `true` if the delivery is currently in progress.
    pub fn is_inflight(&self) -> bool {
        !self.transmissions.is_empty() || self.state.is_inflight()
    }

    /// Overwrites the amount of total received and acknowledged bytes.
    ///
    /// This method is only used for testing purposes, in order to simulate a
    /// large number of already received bytes. The value is normally updated
    /// as an implementation detail!
    #[cfg(test)]
    pub fn set_total_acknowledged_len(&mut self, total_acknowledged: VarInt) {
        assert_eq!(
            VarInt::from_u8(0),
            self.total_enqueued_len(),
            "set_total_acknowledged_len can only be called on a new stream"
        );
        self.buffer.set_offset(total_acknowledged);
    }

    /// Returns the amount of data that can be additionally buffered for sending
    ///
    /// This depends on the configured maximum buffer size.
    /// We do not utilize the window size that the peer provides us in order to
    /// avoid excessive buffering in case the peer would provide a very big window.
    pub fn available_buffer_space(&self) -> usize {
        self.max_buffer_capacity
            .saturating_sub(self.buffer.enqueued_len())
            .try_into()
            .unwrap_or(usize::MAX)
    }

    /// Enqueues the data for transmission.
    ///
    /// It is only allowed to enqueue bytes if they do not overflow the maximum
    /// allowed Stream window (of the maximum VarInt size). This is not checked
    /// inside this method. The already enqueued bytes can be retrieved by
    /// calling [`total_enqueued_len()`].
    pub fn push(&mut self, data: Bytes) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.5
        //# An endpoint MUST NOT send data on a stream at or beyond the final
        //# size.
        debug_assert_eq!(
            self.state,
            State::Sending,
            "Data transmission is not allowed after finish() was called"
        );
        debug_assert!(
            data.len() <= u32::MAX as usize,
            "Maximum data size exceeded"
        );

        if data.is_empty() {
            return;
        }

        self.pending
            .insert(self.buffer.push(data))
            .expect("pending should not have a limit");

        self.check_integrity();
    }

    /// Starts the finalization process of a `Stream` by enqueuing a `FIN` frame.
    pub fn finish(&mut self) {
        if self.state != State::Sending {
            return;
        }

        if Writer::WRITES_FIN {
            self.state = State::Finishing(FinState::Pending);
        } else {
            self.state = State::Finishing(FinState::Acknowledged);
        }

        self.check_integrity();
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        // If we do not get acknowledgements for any in flight data don't try
        // to release buffer chunks

        let pending = &mut self.pending;

        let any_acked = self.transmissions.on_ack_signal(ack_set, |range| {
            pending
                .remove(range)
                .expect("output should not have a limit");
        });

        if Writer::WRITES_FIN {
            if let Some(fin_state) = self.state.fin_state_mut() {
                fin_state.on_packet_ack(ack_set);
            }
        }

        if any_acked {
            // Remove any newly acked intervals from lost
            self.lost
                .intersection(pending)
                .expect("lost has no interval limit");
            if let Some(first) = self.pending.min_value() {
                self.buffer.release(first);
            } else {
                // the pending list was completely cleared
                self.buffer.release_all();
                // We don't need to track transmissions for already acked ranges
                self.transmissions.clear();
            }
        }

        // If the FIN was enqueued, and all outgoing data had been transmitted,
        // then we have finalized the stream.
        if matches!(self.state, State::Finishing(FinState::Acknowledged)) && self.is_idle() {
            self.state = State::Finished;
            self.flow_controller_mut().finish();
            self.buffer.release_all();
        }

        self.check_integrity();
    }

    fn is_idle(&self) -> bool {
        self.transmissions.is_empty() && self.pending.is_empty() && self.lost.is_empty()
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        let lost = &mut self.lost;

        let mut any_lost = self.transmissions.on_ack_signal(ack_set, |range| {
            lost.insert(range).expect("output should not have a limit");
        });

        if Writer::WRITES_FIN {
            if let Some(fin_state) = self.state.fin_state_mut() {
                any_lost |= fin_state.on_packet_loss(ack_set);
            }
        }

        // Since a packet needs to get retransmitted, we are
        // no longer blocked on waiting for flow control windows
        if any_lost {
            self.flow_controller_mut().clear_blocked();
            // Remove any lost intervals that had already been sent
            // and acked in a different packet
            self.lost
                .intersection(&self.pending)
                .expect("lost has no interval limit");
        }

        self.check_integrity();
    }

    /// Queries the component for any outgoing frames that need to get sent
    #[inline]
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        writer_context: Writer::Context,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        let initial_capacity = context.remaining_capacity();

        match self.on_transmit_impl(writer_context, context) {
            // only return an error if we didn't write anything
            Err(_) if context.remaining_capacity() < initial_capacity => Ok(()),
            other => other,
        }
    }

    fn on_transmit_impl<W: WriteContext>(
        &mut self,
        writer_context: Writer::Context,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        let constraint = context.transmission_constraint();

        let mut transmitted_lost = false;
        // try to retransmit any lost ranges first
        if constraint.can_retransmit() {
            transmitted_lost = self.transmissions.transmit_set(
                &self.buffer,
                &mut self.lost,
                &mut self.state,
                writer_context,
                context,
            )?;
        }

        let is_blocked = self.flow_controller().is_blocked();

        // try to transmit the enqueued ranges
        let total_len = self.buffer.total_len();

        let starting_transmission_offset = self.transmission_offset;

        if !is_blocked && constraint.can_transmit() && self.transmission_offset < total_len {
            let mut viewer = self.buffer.viewer();
            self.transmission_offset = self
                .transmissions
                .transmit_interval(
                    &mut viewer,
                    (self.transmission_offset..total_len).into(),
                    &mut self.state,
                    writer_context,
                    context,
                )?
                .end_exclusive();
        }

        if Writer::WRITES_FIN && self.state.can_transmit_fin(constraint, is_blocked) {
            self.transmissions.transmit_fin(
                &self.buffer,
                &mut self.state,
                writer_context,
                context,
            )?;
        }

        // If the current transmission is a loss recovery probe, we can include some already
        // transmitted, unacknowledged data in the probe packet since there is a higher likelihood
        // this data has been lost. If lost data has already been written to the packet, we
        // skip this feature as an optimization to avoid having to filter out already written
        // lost data. Since it is unlikely there is lost data requiring retransmission at the
        // same time as a probe transmission is being sent, this optimization does not have
        // much impact on the effectiveness of this feature.
        let retransmit_unacked_data_in_probe = Writer::RETRANSMIT_IN_PROBE
            && context.transmission_mode().is_loss_recovery_probing()
            && !transmitted_lost;

        if retransmit_unacked_data_in_probe {
            let mut viewer = self.buffer.viewer();

            for interval in self.pending.intervals() {
                if interval.start_inclusive() >= starting_transmission_offset {
                    // Don't write data we've already written to this packet
                    break;
                }

                let interval_end = interval.end_exclusive().min(starting_transmission_offset);

                self.transmissions.transmit_interval(
                    &mut viewer,
                    (interval.start_inclusive()..interval_end).into(),
                    &mut self.state,
                    writer_context,
                    context,
                )?;
            }
        }

        self.check_integrity();

        Ok(())
    }

    #[inline]
    fn check_integrity(&self) {
        if cfg!(debug_assertions) {
            // TODO: assert!(self.lost.is_subset(&self.pending));
            if self.pending.is_empty() {
                assert!(self.lost.is_empty());
                assert!(self.transmissions.is_empty());
                assert_eq!(self.buffer.head(), self.buffer.total_len());
                assert_eq!(
                    self.transmission_offset,
                    self.buffer.total_len(),
                    "transmission offset should equal buffer length when pending is empty"
                );
            }

            if !self.pending.is_empty() {
                assert!(
                    !self.lost.is_empty() || self.transmission_offset < self.buffer.total_len() || !self.transmissions.is_empty(),
                    "pending: {:?}, lost: {:?}, enqueued: {:?}, total_len: {:?}, transmissions: {:?}",
                    self.pending,
                    self.lost,
                    self.transmission_offset,
                    self.buffer.total_len(),
                    self.transmissions.is_empty()
                );
            }

            if let Some(start) = self.pending.min_value() {
                assert_eq!(self.buffer.head(), start);
            }

            if self.flow_controller().is_blocked() {
                use transmission::interest::{Interest, Provider};
                assert_ne!(self.get_transmission_interest(), Interest::NewData);
            }
        }
    }
}

impl<F: OutgoingDataFlowController, W: FrameWriter> transmission::interest::Provider
    for DataSender<F, W>
{
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        if W::WRITES_FIN {
            match self.state {
                State::Finishing(FinState::Lost) => {
                    return query.on_lost_data();
                }
                State::Finishing(FinState::Pending) if !self.flow_controller().is_blocked() => {
                    query.on_new_data()?;
                }
                _ => {}
            }
        };

        if !self.lost.is_empty() {
            query.on_lost_data()?;
        } else if self.transmission_offset < self.buffer.total_len()
            && !self.flow_controller().is_blocked()
        {
            query.on_new_data()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
        transmission::{self, interest::Provider as _},
    };
    use bolero::{check, generator::*};
    use s2n_quic_core::{endpoint, frame, stream::testing as stream, time::clock::testing as time};
    use std::collections::HashSet;

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Event {
        Push(#[generator(1..)] u16),
        Finish,
        Transmit(u16, transmission::Constraint),
        IncFlowControl(u16),
        Ack(usize),
        Loss(usize),
    }

    #[derive(Debug, Default)]
    struct TestFlowController {
        max_offset: VarInt,
        is_blocked: bool,
    }

    impl OutgoingDataFlowController for TestFlowController {
        fn acquire_flow_control_window(&mut self, end_offset: VarInt) -> VarInt {
            if end_offset > self.max_offset {
                self.is_blocked = true;
                self.max_offset
            } else {
                end_offset
            }
        }

        fn is_blocked(&self) -> bool {
            self.is_blocked
        }

        fn clear_blocked(&mut self) {
            self.is_blocked = false;
        }

        fn finish(&mut self) {}
    }

    fn check_model(events: &[Event], id: &VarInt) -> OutgoingFrameBuffer {
        let mut send_data = stream::Data::new(u64::MAX);
        let mut sender: DataSender<_, writer::Stream> =
            DataSender::new(TestFlowController::default(), u32::MAX);
        let mut total_len = 0;
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext {
            current_time: time::now(),
            frame_buffer: &mut frame_buffer,
            transmission_constraint: transmission::Constraint::None,
            transmission_mode: transmission::Mode::Normal,
            endpoint: endpoint::Type::Server,
        };
        let mut lost = HashSet::new();
        let mut pending = HashSet::new();
        let mut acked = HashSet::new();
        let mut is_finished = false;

        for event in events.iter().copied() {
            match event {
                Event::Push(len) if !is_finished => {
                    let mut chunks = [Bytes::new(), Bytes::new()];
                    let count = send_data
                        .send(len as usize, &mut chunks)
                        .expect("stream should not end early");
                    for chunk in chunks.iter_mut().take(count) {
                        total_len += chunk.len() as u64;
                        sender.push(core::mem::replace(chunk, Bytes::new()));
                    }
                }
                Event::Finish if total_len > 0 => {
                    sender.finish();
                    is_finished = true;
                }
                Event::Transmit(capacity, constraint) => {
                    let interest = sender.get_transmission_interest();

                    let prev_len = context.frame_buffer.len();
                    context.transmission_constraint = constraint;

                    context
                        .frame_buffer
                        .set_max_packet_size(Some(capacity as usize));

                    let _ = sender.on_transmit(*id, &mut context);
                    context.frame_buffer.flush();

                    if interest.is_none() {
                        assert_eq!(
                            context.frame_buffer.len(),
                            prev_len,
                            "frames should only transmit with interest"
                        );
                    }

                    let packets = context
                        .frame_buffer
                        .frames
                        .iter()
                        .skip(prev_len)
                        .map(|frame| frame.packet_nr);
                    pending.extend(packets);
                }
                Event::Ack(index) if !context.frame_buffer.is_empty() => {
                    let index = index % context.frame_buffer.len();
                    let packet = context.frame_buffer.frames[index].packet_nr;
                    sender.on_packet_ack(&packet);
                    pending.remove(&packet);
                    acked.insert(packet);
                }
                Event::Loss(index) if !context.frame_buffer.is_empty() => {
                    let index = index % context.frame_buffer.len();
                    let packet = context.frame_buffer.frames[index].packet_nr;
                    lost.insert(packet);
                    sender.on_packet_loss(&packet);
                }
                Event::IncFlowControl(amount) => {
                    let flow_controller = sender.flow_controller_mut();
                    flow_controller.max_offset = flow_controller
                        .max_offset
                        .saturating_add(VarInt::from_u16(amount));
                }
                _ => {}
            }
        }

        // make sure the stream was finished
        sender.finish();

        for packet in pending {
            sender.on_packet_ack(&packet);
            acked.insert(packet);
        }

        context.frame_buffer.set_max_packet_size(Some(usize::MAX));
        context.transmission_constraint = transmission::Constraint::None;
        sender.flow_controller_mut().clear_blocked();
        sender.flow_controller_mut().max_offset = VarInt::MAX;

        while sender.has_transmission_interest() {
            let prev_len = context.frame_buffer.len();
            let _ = sender.on_transmit(*id, &mut context);
            context.frame_buffer.flush();
            let packets = context
                .frame_buffer
                .frames
                .iter()
                .skip(prev_len)
                .map(|frame| frame.packet_nr);

            let mut did_transmit = false;

            for packet in packets {
                sender.on_packet_ack(&packet);
                acked.insert(packet);
                did_transmit = true;
            }

            assert!(
                did_transmit,
                "transmission_interest was expressed but sender did not transmit: {sender:#?}"
            );
        }

        assert!(
            !frame_buffer.is_empty(),
            "the test should transmit at least one frame: {sender:#?}",
        );

        let receiver = stream::Data::new(total_len);
        let mut received_ranges = IntervalSet::new();
        let mut transmitted_fin = false;

        for frame in &mut frame_buffer.frames {
            if acked.contains(&frame.packet_nr) {
                if let frame::Frame::Stream(frame) = frame.as_frame() {
                    let offset = frame.offset.as_u64();
                    let len = frame.data.len() as u64;
                    if len > 0 {
                        receiver.receive_at(offset, &[frame.data.as_less_safe_slice()]);
                        received_ranges.insert(offset..offset + len).unwrap();
                    }
                    transmitted_fin |= frame.is_fin;
                } else {
                    panic!("invalid frame");
                }
            }
        }

        if total_len != 0 {
            assert_eq!(
                received_ranges.interval_len(),
                1,
                "not all data was transmitted",
            );

            assert_eq!(received_ranges.max_value(), Some(total_len - 1));
        } else {
            assert!(received_ranges.is_empty(), "{sender:#?}");
        }

        assert!(transmitted_fin);

        frame_buffer
    }

    #[test]
    fn model() {
        check!()
            .with_generator((produce(), produce::<Vec<Event>>().with().len(1usize..128)))
            .for_each(|(id, events)| {
                check_model(events, id);
            });
    }
}
