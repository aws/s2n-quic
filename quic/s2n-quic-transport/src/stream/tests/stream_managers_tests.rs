// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for `StreamManager`

use super::*;
use crate::{
    connection::{
        finalization::Provider, InternalConnectionId, InternalConnectionIdGenerator,
        Limits as ConnectionLimits,
    },
    contexts::{ConnectionApiCallContext, OnTransmitError, WriteContext},
    endpoint,
    recovery::RttEstimator,
    stream::{
        controller::MAX_STREAMS_SYNC_FRACTION,
        stream_impl::StreamConfig,
        stream_interests::{StreamInterestProvider, StreamInterests},
        AbstractStreamManager, StreamError, StreamEvents, StreamTrait,
    },
    sync::DEFAULT_SYNC_PERIOD,
    transmission,
    transmission::interest::Provider as TransmissionInterestProvider,
    wakeup_queue::{WakeupHandle, WakeupQueue},
};
use alloc::collections::VecDeque;
use bytes::Bytes;
use core::{
    task::{Context, Poll, Waker},
    time::Duration,
};
use futures_test::task::new_count_waker;
use s2n_quic_core::{
    ack::Set as AckSet,
    application::Error as ApplicationErrorCode,
    connection,
    frame::{
        stream::StreamRef, Frame, MaxData, MaxStreamData, MaxStreams, ResetStream, StopSending,
        Stream as StreamFrame, StreamDataBlocked, StreamsBlocked,
    },
    packet::number::{PacketNumberRange, PacketNumberSpace},
    stream::{ops, StreamId, StreamType},
    transport::{
        parameters::{InitialFlowControlLimits, InitialStreamLimits, MaxIdleTimeout},
        Error as TransportError,
    },
    varint::VarInt,
};

struct MockStream {
    config: StreamConfig,
    last_reset: Option<ResetStream>,
    last_on_stream_data_blocked: Option<StreamDataBlocked>,
    last_max_stream_data: Option<MaxStreamData>,
    last_stop_sending: Option<StopSending>,
    read_waker_to_return: Option<Waker>,
    write_waker_to_return: Option<Waker>,
    interests: StreamInterests,
    on_connection_window_available_count: usize,
    on_connection_window_available_retrieve_window: u64,
    on_packet_ack_count: usize,
    on_packet_loss_count: usize,
    on_internal_reset_count: usize,
    on_transmit_try_write_frames: usize,
    on_transmit_count: usize,
    on_data_count: usize,
    on_reset_count: usize,
    on_stream_data_blocked_count: usize,
    on_stop_sending_count: usize,
    on_max_stream_data_count: usize,
    lost_data: bool,
    set_finalize_on_internal_reset: bool,
    next_packet_error: Option<TransportError>,
    next_api_error: Option<StreamError>,
    api_call_requires_transmission: bool,
    poll_pop_count: usize,
    stop_sending_count: usize,
    poll_push_count: usize,
    poll_finish_count: usize,
    reset_count: usize,
}

impl MockStream {
    fn store_wakers(&mut self, events: &mut StreamEvents) {
        if let Some(waker) = self.read_waker_to_return.clone() {
            events.store_read_waker(waker);
        }
        if let Some(waker) = self.write_waker_to_return.clone() {
            events.store_write_waker(waker);
        }
    }
}

impl StreamInterestProvider for MockStream {
    fn interests(&self) -> StreamInterests {
        let mut interests = self.interests;
        interests.connection_flow_control_credits =
            self.on_connection_window_available_retrieve_window > 0;
        if self.on_transmit_try_write_frames > 0 {
            if self.lost_data {
                interests.transmission = transmission::Interest::LostData;
            } else {
                interests.transmission = transmission::Interest::NewData;
            }
        } else {
            interests.transmission = transmission::Interest::None;
        }
        interests
    }
}

impl StreamTrait for MockStream {
    fn new(config: StreamConfig) -> Self {
        Self {
            config,
            last_reset: None,
            last_on_stream_data_blocked: None,
            last_max_stream_data: None,
            last_stop_sending: None,
            interests: StreamInterests::default(),
            on_connection_window_available_count: 0,
            on_connection_window_available_retrieve_window: 0,
            on_packet_ack_count: 0,
            on_packet_loss_count: 0,
            on_internal_reset_count: 0,
            on_data_count: 0,
            on_reset_count: 0,
            on_stream_data_blocked_count: 0,
            on_stop_sending_count: 0,
            on_max_stream_data_count: 0,
            on_transmit_count: 0,
            on_transmit_try_write_frames: 0,
            lost_data: false,
            set_finalize_on_internal_reset: false,
            next_packet_error: None,
            next_api_error: None,
            api_call_requires_transmission: false,
            read_waker_to_return: None,
            write_waker_to_return: None,
            poll_pop_count: 0,
            stop_sending_count: 0,
            poll_push_count: 0,
            poll_finish_count: 0,
            reset_count: 0,
        }
    }

    fn stream_id(&self) -> StreamId {
        self.config.stream_id
    }

    fn on_data(
        &mut self,
        frame: &StreamRef,
        events: &mut StreamEvents,
    ) -> Result<(), TransportError> {
        assert_eq!(self.stream_id(), StreamId::from_varint(frame.stream_id));
        self.on_data_count += 1;
        self.store_wakers(events);
        if let Some(err) = self.next_packet_error {
            return Err(err);
        };
        Ok(())
    }

    fn on_stream_data_blocked(
        &mut self,
        frame: &StreamDataBlocked,
        events: &mut StreamEvents,
    ) -> Result<(), TransportError> {
        assert_eq!(self.stream_id(), StreamId::from_varint(frame.stream_id));
        self.last_on_stream_data_blocked = Some(*frame);
        self.on_stream_data_blocked_count += 1;
        self.store_wakers(events);
        if let Some(err) = self.next_packet_error {
            return Err(err);
        };
        Ok(())
    }

    fn on_reset(
        &mut self,
        frame: &ResetStream,
        events: &mut StreamEvents,
    ) -> Result<(), TransportError> {
        assert_eq!(self.stream_id(), StreamId::from_varint(frame.stream_id));
        self.on_reset_count += 1;
        self.last_reset = Some(*frame);
        self.store_wakers(events);
        if let Some(err) = self.next_packet_error {
            return Err(err);
        };
        Ok(())
    }

    fn on_max_stream_data(
        &mut self,
        frame: &MaxStreamData,
        events: &mut StreamEvents,
    ) -> Result<(), TransportError> {
        assert_eq!(self.stream_id(), StreamId::from_varint(frame.stream_id));
        self.on_max_stream_data_count += 1;
        self.last_max_stream_data = Some(*frame);
        self.store_wakers(events);
        if let Some(err) = self.next_packet_error {
            return Err(err);
        };
        Ok(())
    }

    fn on_stop_sending(
        &mut self,
        frame: &StopSending,
        events: &mut StreamEvents,
    ) -> Result<(), TransportError> {
        assert_eq!(self.stream_id(), StreamId::from_varint(frame.stream_id));
        self.last_stop_sending = Some(*frame);
        self.on_stop_sending_count += 1;
        self.store_wakers(events);
        if let Some(err) = self.next_packet_error {
            return Err(err);
        };
        Ok(())
    }

    fn on_packet_ack<A: AckSet>(&mut self, _ack_set: &A, events: &mut StreamEvents) {
        self.on_packet_ack_count += 1;
        self.store_wakers(events);
    }

    fn on_packet_loss<A: AckSet>(&mut self, _ack_set: &A, events: &mut StreamEvents) {
        self.on_packet_loss_count += 1;
        self.store_wakers(events);
    }

    fn on_internal_reset(&mut self, _error: StreamError, events: &mut StreamEvents) {
        self.on_internal_reset_count += 1;
        if self.set_finalize_on_internal_reset {
            self.interests.finalization = true;
        }
        self.store_wakers(events);
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        self.on_transmit_count += 1;
        while self.on_transmit_try_write_frames > 0 {
            // We write simple frames here, since the tests do not care about
            // the content but only the number of succeeded write calls.
            let _pn = context
                .write_frame(&MaxData {
                    maximum_data: VarInt::from_u32(0),
                })
                .ok_or(OnTransmitError::CouldNotWriteFrame)?;
            self.on_transmit_try_write_frames -= 1;
        }

        Ok(())
    }

    fn on_connection_window_available(&mut self) {
        self.on_connection_window_available_count += 1;
        let acquired_window = self
            .config
            .outgoing_connection_flow_controller
            .acquire_window(
                VarInt::new(self.on_connection_window_available_retrieve_window).unwrap(),
            );
        self.on_connection_window_available_retrieve_window -= Into::<u64>::into(acquired_window);
    }

    fn poll_request(
        &mut self,
        request: &mut ops::Request,
        _context: Option<&Context>,
    ) -> Result<ops::Response, StreamError> {
        let mut response = ops::Response::default();

        if let Some(tx) = request.tx.as_ref() {
            if tx.chunks.is_some() {
                self.poll_push_count += 1;
            }

            if tx.finish {
                self.poll_finish_count += 1;
            }

            if tx.reset.is_some() {
                self.reset_count += 1;
            }

            response.tx = Some(ops::tx::Response::default());
        }

        if let Some(rx) = request.rx.as_ref() {
            if rx.chunks.is_some() {
                self.poll_pop_count += 1;
            }

            if rx.stop_sending.is_some() {
                self.stop_sending_count += 1;
            }

            response.rx = Some(ops::rx::Response::default());
        }

        if self.api_call_requires_transmission {
            self.on_transmit_try_write_frames = 1;
        }

        if let Some(err) = self.next_api_error.take() {
            return Err(err);
        }

        Ok(response)
    }
}

fn create_default_initial_flow_control_limits() -> InitialFlowControlLimits {
    InitialFlowControlLimits {
        stream_limits: InitialStreamLimits {
            max_data_bidi_local: VarInt::from_u32(4096),
            max_data_bidi_remote: VarInt::from_u32(4096),
            max_data_uni: VarInt::from_u32(4096),
        },
        max_data: VarInt::from_u32(64 * 1024),
        max_streams_bidi: VarInt::from_u32(128),
        max_streams_uni: VarInt::from_u32(128),
    }
}

fn create_wakeup_queue_and_handle() -> (
    WakeupQueue<InternalConnectionId>,
    WakeupHandle<InternalConnectionId>,
) {
    let wakeup_queue = WakeupQueue::new();
    let connection_id = InternalConnectionIdGenerator::new().generate_id();
    let wakeup_handle = wakeup_queue.create_wakeup_handle(connection_id);
    (wakeup_queue, wakeup_handle)
}

/// Asserts that a given number of wakeups had been enqueued
fn assert_wakeups(wakeup_queue: &mut WakeupQueue<InternalConnectionId>, expected_wakeups: usize) {
    let dequeued_wakeups = VecDeque::new();
    let (waker, _counter) = new_count_waker();
    let dequeued_wakeups =
        wakeup_queue.poll_pending_wakeups(dequeued_wakeups, &Context::from_waker(&waker));

    assert_eq!(expected_wakeups, dequeued_wakeups.len());
}

fn create_stream_manager(local_ep_type: endpoint::Type) -> AbstractStreamManager<MockStream> {
    let initial_local_limits = create_default_initial_flow_control_limits();
    let initial_peer_limits = create_default_initial_flow_control_limits();

    let limits = ConnectionLimits::default()
        .with_max_send_buffer_size(4096)
        .unwrap()
        .with_max_open_local_unidirectional_streams(256)
        .unwrap();

    AbstractStreamManager::<MockStream>::new(
        &limits,
        local_ep_type,
        initial_local_limits,
        initial_peer_limits,
    )
}

/// Tries to open a new stream and returns an error if we're at the limit
fn try_open(
    manager: &mut AbstractStreamManager<MockStream>,
    stream_type: StreamType,
) -> Result<StreamId, connection::Error> {
    let (accept_waker, _accept_wake_counter) = new_count_waker();

    match manager.poll_open(stream_type, &Context::from_waker(&accept_waker)) {
        Poll::Ready(res) => res,
        Poll::Pending => Err(connection::Error::Unspecified),
    }
}

#[test]
fn remote_messages_open_unopened_streams() {
    const STREAMS_TO_OPEN: u64 = 8;

    for initiator_type in &[endpoint::Type::Server, endpoint::Type::Client] {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            for local_ep_type in &[endpoint::Type::Server, endpoint::Type::Client] {
                let is_remote_initialized = *local_ep_type != *initiator_type;
                if !is_remote_initialized {
                    continue;
                }

                let highest_stream_id =
                    StreamId::nth(*initiator_type, *stream_type, STREAMS_TO_OPEN - 1).unwrap();
                let reset_frame = ResetStream {
                    stream_id: highest_stream_id.into(),
                    application_error_code: VarInt::from_u32(0x2233_4455),
                    final_size: VarInt::from_u32(2000),
                };

                let mut manager = create_stream_manager(*local_ep_type);
                assert!(manager.on_reset_stream(&reset_frame).is_ok());

                // All other Streams are opened (but not reset)
                for n in 0..STREAMS_TO_OPEN - 1 {
                    manager.with_asserted_stream(
                        StreamId::nth(*initiator_type, *stream_type, n).unwrap(),
                        |stream| {
                            assert!(stream.last_reset.is_none());
                        },
                    )
                }

                // The highest stream received the reset frame
                manager.with_asserted_stream(highest_stream_id, |stream| {
                    assert_eq!(Some(reset_frame), stream.last_reset);
                })
            }
        }
    }
}

#[test]
fn remote_streams_do_not_open_if_manager_is_closed() {
    const STREAMS_TO_OPEN: u64 = 8;

    for initiator_type in &[endpoint::Type::Server, endpoint::Type::Client] {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            for local_ep_type in &[endpoint::Type::Server, endpoint::Type::Client] {
                let is_remote_initialized = *local_ep_type != *initiator_type;
                if !is_remote_initialized {
                    continue;
                }

                let highest_stream_id =
                    StreamId::nth(*initiator_type, *stream_type, STREAMS_TO_OPEN - 1).unwrap();
                let reset_frame = ResetStream {
                    stream_id: highest_stream_id.into(),
                    application_error_code: VarInt::from_u32(0x2233_4455),
                    final_size: VarInt::from_u32(2000),
                };

                let mut manager = create_stream_manager(*local_ep_type);
                manager.close(TransportError::NO_ERROR.into());

                assert_is_transport_error(
                    manager.on_reset_stream(&reset_frame),
                    TransportError::NO_ERROR,
                );

                // No stream is open
                assert_eq!(0, manager.active_streams().len());
            }
        }
    }
}

#[test]
fn opens_locally_initiated_streams() {
    for local_ep_type in [endpoint::Type::Client, endpoint::Type::Server]
        .iter()
        .copied()
    {
        for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
            .iter()
            .copied()
        {
            let mut manager = create_stream_manager(local_ep_type);

            for n in 0..8 {
                let stream_id: StreamId = try_open(&mut manager, stream_type).unwrap();
                assert!(manager.active_streams().contains(&stream_id));
                assert_eq!(
                    StreamId::nth(local_ep_type, stream_type, n).unwrap(),
                    stream_id
                );
            }

            // Check Stream ID exhaustion
            *manager
                .inner
                .next_stream_ids
                .get_mut(local_ep_type, stream_type) = None;

            assert_eq!(
                Err(connection::Error::StreamIdExhausted),
                try_open(&mut manager, stream_type)
            );
        }
    }
}

#[test]
fn open_returns_error_after_close() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    manager.close(connection::Error::Unspecified);
    assert_eq!(
        Err(connection::Error::Unspecified),
        try_open(&mut manager, StreamType::Bidirectional)
    )
}

#[test]
fn returns_finalization_interest_after_last_stream_is_drained() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    let (_wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();

    assert_eq!(0, manager.active_streams().len());
    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    assert_eq!(1, manager.active_streams().len());
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    assert_eq!(2, manager.active_streams().len());
    assert!(manager.finalization_status().is_idle());

    let error = ApplicationErrorCode::new(0).unwrap();

    manager.close(connection::Error::Application {
        error,
        initiator: endpoint::Location::Local,
    });
    assert!(manager.finalization_status().is_draining());

    // The first stream is immediately interested in finalization and should
    // therefore be collected
    manager.with_asserted_stream(stream_1, |stream| {
        stream.interests.finalization = true;
    });
    assert_eq!(1, manager.active_streams().len());
    assert!(manager.finalization_status().is_draining());

    // The second stream is not yet interested in finalization
    assert!(manager
        .poll_request(
            stream_2,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().reset(error),
            None,
        )
        .is_ok());
    assert_eq!(1, manager.active_streams().len());
    assert!(manager.finalization_status().is_draining());

    // Let the last stream return the finalization interest
    manager.with_asserted_stream(stream_2, |stream| {
        stream.interests.finalization = true;
    });
    assert_eq!(0, manager.active_streams().len());
    assert!(manager.finalization_status().is_final());
}

#[test]
fn remote_messages_which_target_locally_initiated_unopened_streams_error() {
    for initiator_type in &[endpoint::Type::Server, endpoint::Type::Client] {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            for local_ep_type in &[endpoint::Type::Server, endpoint::Type::Client] {
                let is_remote_initialized = *local_ep_type != *initiator_type;
                if is_remote_initialized {
                    continue;
                }

                for (already_open, target_stream_id) in &[(0, 0), (1, 1), (3, 5)] {
                    let mut manager = create_stream_manager(*local_ep_type);

                    for _ in 0..*already_open {
                        let stream_id = try_open(&mut manager, *stream_type).unwrap();
                        // Check that the Stream is really open
                        assert!(manager.active_streams().contains(&stream_id));
                    }

                    let target_stream_id =
                        StreamId::nth(*initiator_type, *stream_type, *target_stream_id).unwrap();

                    let reset_frame = ResetStream {
                        stream_id: target_stream_id.into(),
                        application_error_code: VarInt::from_u32(0x2233_4455),
                        final_size: VarInt::from_u32(2000),
                    };

                    // Check that the target stream is not open
                    assert!(!manager.active_streams().contains(&target_stream_id));

                    assert_eq!(
                        Err(TransportError::STREAM_STATE_ERROR
                            .with_reason("Stream was not yet opened")),
                        manager.on_reset_stream(&reset_frame)
                    );
                }
            }
        }
    }
}

#[test]
fn max_data_replenishes_connection_flow_control_window() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    let current_window =
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window());

    for window in &[current_window + 1, current_window + 100] {
        assert!(manager
            .on_max_data(MaxData {
                maximum_data: *window,
            })
            .is_ok());
        assert_eq!(
            *window,
            manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window())
        );
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
//= type=test
//# A receiver MUST
//# ignore any MAX_STREAMS frame that does not increase the stream limit.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.11
//= type=test
//# MAX_STREAMS frames that do not increase the stream limit MUST be
//# ignored.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
//= type=test
//# Endpoints MUST NOT exceed the limit set by their peer.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.11
//= type=test
//# An endpoint MUST NOT open more streams than permitted by the current
//# stream limit set by its peer.
#[test]
fn max_streams_replenishes_stream_control_capacity() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
        .iter()
        .copied()
    {
        let current_max_streams =
            manager.with_stream_controller(|ctrl| ctrl.max_streams_latest_value(stream_type));

        // Open and close up to the current max streams limit to ensure we are blocked on the
        // peer's max streams limit and not the local concurrent stream limit.
        for i in 0..*current_max_streams {
            let stream_id = StreamId::nth(endpoint::Type::Server, stream_type, i).unwrap();
            manager.with_stream_controller(|ctrl| ctrl.on_open_stream(stream_id));
            manager.with_stream_controller(|ctrl| ctrl.on_close_stream(stream_id));
        }

        let (waker, _counter) = new_count_waker();
        assert!(manager
            .poll_open(stream_type, &Context::from_waker(&waker))
            .is_pending());

        for additional_streams in &[VarInt::from_u8(0), VarInt::from_u8(1), VarInt::from_u8(10)] {
            assert!(manager
                .on_max_streams(&MaxStreams {
                    stream_type,
                    maximum_streams: current_max_streams + *additional_streams,
                })
                .is_ok());
            assert_eq!(
                *additional_streams,
                manager.with_stream_controller(
                    |ctrl| ctrl.available_outgoing_stream_capacity(stream_type)
                )
            );
        }

        assert!(manager
            .poll_open(stream_type, &Context::from_waker(&waker))
            .is_ready());
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
//= type=test
//# An endpoint MUST NOT wait
//# to receive this signal before advertising additional credit, since
//# doing so will mean that the peer will be blocked for at least an
//# entire round trip
#[test]
fn peer_closing_streams_transmits_max_streams() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
        .iter()
        .copied()
    {
        let current_max_streams =
            manager.with_stream_controller(|ctrl| ctrl.max_streams_latest_value(stream_type));

        // The peer opens up to the current max streams limit
        for i in 0..*current_max_streams {
            let stream_id = StreamId::nth(endpoint::Type::Client, stream_type, i).unwrap();
            assert_eq!(
                Ok(()),
                manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], false))
            );
        }

        assert_eq!(
            transmission::Interest::None,
            manager.transmission_interest()
        );

        // The peer closes MAX_STREAMS_SYNC_FRACTION of streams
        let streams_to_close = current_max_streams / MAX_STREAMS_SYNC_FRACTION;

        for i in 0..*streams_to_close {
            let stream_id = StreamId::nth(endpoint::Type::Client, stream_type, i).unwrap();
            manager.with_asserted_stream(stream_id, |stream| {
                stream.interests.finalization = true;
            });
        }

        assert_eq!(
            transmission::Interest::NewData,
            manager.transmission_interest()
        );

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            endpoint::Type::Server,
        );
        let packet_number = write_context.packet_number();
        assert!(manager.on_transmit(&mut write_context).is_ok());

        let expected_frame = Frame::MaxStreams {
            0: MaxStreams {
                stream_type,
                maximum_streams: current_max_streams + streams_to_close,
            },
        };

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );

        assert_eq!(
            transmission::Interest::None,
            manager.transmission_interest()
        );

        manager.on_packet_loss(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            transmission::Interest::LostData,
            manager.transmission_interest()
        );

        let packet_number = write_context.packet_number();
        assert!(manager.on_transmit(&mut write_context).is_ok());

        manager.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            transmission::Interest::None,
            manager.transmission_interest()
        );
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
//= type=test
//# An endpoint that is unable to open a new stream due to the peer's
//# limits SHOULD send a STREAMS_BLOCKED frame (Section 19.14).

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.14
//= type=test
//# A sender SHOULD send a STREAMS_BLOCKED frame (type=0x16 or 0x17) when
//# it wishes to open a stream, but is unable to due to the maximum
//# stream limit set by its peer; see Section 19.11.
#[test]
fn send_streams_blocked_frame_when_blocked_by_peer() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
        .iter()
        .copied()
    {
        let (waker, _) = new_count_waker();

        let mut opened_streams = VarInt::from_u8(0);

        // Open streams until blocked
        while manager
            .poll_open(stream_type, &Context::from_waker(&waker))
            .is_ready()
        {
            opened_streams += 1;
        }

        assert_eq!(
            transmission::Interest::NewData,
            manager.transmission_interest()
        );

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            endpoint::Type::Server,
        );
        let packet_number = write_context.packet_number();
        assert!(manager.on_transmit(&mut write_context).is_ok());

        let expected_frame = Frame::StreamsBlocked {
            0: StreamsBlocked {
                stream_type,
                stream_limit: opened_streams,
            },
        };

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );

        assert_eq!(
            transmission::Interest::None,
            manager.transmission_interest()
        );

        manager.on_packet_loss(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            transmission::Interest::LostData,
            manager.transmission_interest()
        );

        let packet_number = write_context.packet_number();
        assert!(manager.on_transmit(&mut write_context).is_ok());

        manager.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            transmission::Interest::None,
            manager.transmission_interest()
        );

        let expected_next_stream_blocked_time = write_context.current_time + DEFAULT_SYNC_PERIOD;
        assert_eq!(
            Some(expected_next_stream_blocked_time),
            manager.timers().next().copied()
        );

        manager.on_timeout(expected_next_stream_blocked_time);

        // Another STREAM_BLOCKED frame should be sent
        assert_eq!(
            transmission::Interest::NewData,
            manager.transmission_interest()
        );

        // We get more credit from the peer so we should no longer send STREAM_BLOCKED
        assert!(manager
            .on_max_streams(&MaxStreams {
                stream_type,
                maximum_streams: VarInt::from_u32(100_000),
            })
            .is_ok());

        assert_eq!(
            transmission::Interest::None,
            manager.transmission_interest()
        );
    }
}

#[test]
fn send_streams_blocked_period_based_on_rtt() {
    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
        .iter()
        .copied()
    {
        let mut manager = create_stream_manager(endpoint::Type::Server);
        let (waker, _) = new_count_waker();

        let mut opened_streams = VarInt::from_u8(0);

        // Open streams until blocked
        while manager
            .poll_open(stream_type, &Context::from_waker(&waker))
            .is_ready()
        {
            opened_streams += 1;
        }

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            endpoint::Type::Server,
        );
        let packet_number = write_context.packet_number();
        assert!(manager.on_transmit(&mut write_context).is_ok());

        manager.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

        let mut rtt_estimator = RttEstimator::new(Duration::from_millis(100));
        manager.on_rtt_update(&rtt_estimator);

        let expected_next_stream_blocked_time = write_context.current_time
            + MaxIdleTimeout::RECOMMENDED.as_duration().unwrap()
            - rtt_estimator.smoothed_rtt();
        assert_eq!(
            Some(expected_next_stream_blocked_time),
            manager.timers().next().copied()
        );

        // Update the RTT estimate with a large value to ensure 3 * PTO is larger than the max idle timeout
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_secs(20),
            write_context.current_time,
            true,
            PacketNumberSpace::ApplicationData,
        );
        manager.on_rtt_update(&rtt_estimator);

        let expected_blocked_sync_period = 3 * rtt_estimator
            .pto_period(1, PacketNumberSpace::ApplicationData)
            - rtt_estimator.smoothed_rtt();

        assert_eq!(
            Some(write_context.current_time + expected_blocked_sync_period),
            manager.timers().next().copied()
        );

        // Update the RTT estimate with a small value to test when the blocked period decreases in length
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(100),
            write_context.current_time,
            true,
            PacketNumberSpace::ApplicationData,
        );
        manager.on_rtt_update(&rtt_estimator);

        let expected_blocked_sync_period = 3 * rtt_estimator
            .pto_period(1, PacketNumberSpace::ApplicationData)
            - rtt_estimator.smoothed_rtt();
        assert_eq!(
            Some(write_context.current_time + expected_blocked_sync_period),
            manager.timers().next().copied()
        );
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
//= type=test
//# An endpoint
//# that receives a frame with a stream ID exceeding the limit it has
//# sent MUST treat this as a connection error of type STREAM_LIMIT_ERROR
//# (Section 11).

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.11
//= type=test
//# An endpoint MUST terminate a connection
//# with a STREAM_LIMIT_ERROR error if a peer opens more streams than was
//# permitted.
#[test]
fn stream_limit_error_on_peer_open_stream_too_large() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
        .iter()
        .copied()
    {
        let current_max_streams =
            manager.with_stream_controller(|ctrl| ctrl.max_streams_latest_value(stream_type));

        let max_stream_id = StreamId::nth(
            endpoint::Type::Client,
            stream_type,
            current_max_streams.as_u64(),
        )
        .unwrap();

        assert!(manager
            .with_stream_controller(|ctrl| ctrl.on_remote_open_stream(max_stream_id))
            .is_ok());

        assert_eq!(
            Err(transport::Error::STREAM_LIMIT_ERROR),
            manager.with_stream_controller(
                |ctrl| ctrl.on_remote_open_stream(max_stream_id.next_of_type().unwrap())
            )
        );
    }
}

#[test]
fn blocked_on_local_concurrent_stream_limit() {
    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
        .iter()
        .copied()
    {
        let mut manager = create_stream_manager(endpoint::Type::Server);

        // The peer allows a large amount of streams to be opened
        assert!(manager
            .on_max_streams(&MaxStreams {
                stream_type,
                maximum_streams: VarInt::from_u32(100_000),
            })
            .is_ok());

        let available_outgoing_stream_capacity = manager
            .with_stream_controller(|ctrl| ctrl.available_outgoing_stream_capacity(stream_type));

        assert!(available_outgoing_stream_capacity < VarInt::from_u32(100_000));

        let (waker, wake_counter) = new_count_waker();

        for _i in 0..*available_outgoing_stream_capacity {
            assert!(manager
                .poll_open(stream_type, &Context::from_waker(&waker))
                .is_ready());
        }

        assert_eq!(wake_counter, 0);

        // Cannot open any more streams
        assert!(manager
            .poll_open(stream_type, &Context::from_waker(&waker))
            .is_pending());

        // No STREAMS_BLOCKED frame should be transmitted since we are blocked on the local
        // limit not the peer's limit.
        assert!(manager.transmission_interest().is_none());

        // Close one stream
        manager.with_asserted_stream(
            StreamId::initial(endpoint::Type::Server, stream_type),
            |stream| {
                stream.interests.finalization = true;
            },
        );

        // One more stream can be opened
        assert!(manager
            .poll_open(stream_type, &Context::from_waker(&waker))
            .is_ready());
        assert_eq!(wake_counter, 1);
        assert!(manager
            .poll_open(stream_type, &Context::from_waker(&waker))
            .is_pending());

        // Close the stream manager and verify the wake counter is incremented
        manager.close(connection::Error::Application {
            error: ApplicationErrorCode::new(1).unwrap(),
            initiator: endpoint::Location::Local,
        });

        assert_eq!(wake_counter, 2);
    }
}

#[test]
fn asymmetric_stream_limits() {
    let mut initial_local_limits = create_default_initial_flow_control_limits();
    let mut initial_peer_limits = create_default_initial_flow_control_limits();

    let limits = ConnectionLimits::default()
        .with_max_send_buffer_size(4096)
        .unwrap()
        .with_max_open_local_unidirectional_streams(256)
        .unwrap();

    for (local_limit, peer_limit) in &[(100, 5), (5, 100)] {
        for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
            .iter()
            .copied()
        {
            initial_local_limits.max_streams_bidi = VarInt::from_u8(*local_limit);
            initial_peer_limits.max_streams_bidi = VarInt::from_u8(*peer_limit);
            initial_local_limits.max_streams_uni = VarInt::from_u8(*local_limit);
            initial_peer_limits.max_streams_uni = VarInt::from_u8(*peer_limit);

            let mut manager = AbstractStreamManager::<MockStream>::new(
                &limits,
                endpoint::Type::Server,
                initial_local_limits,
                initial_peer_limits,
            );

            // The peer opens streams up to the limit we have given them
            for i in 0..*local_limit {
                let stream_id =
                    StreamId::nth(endpoint::Type::Client, stream_type, i as u64).unwrap();
                assert_eq!(
                    Ok(()),
                    manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], false))
                );
            }

            let available_outgoing_stream_capacity = manager.with_stream_controller(|cntl| {
                cntl.available_outgoing_stream_capacity(stream_type)
            });

            if stream_type.is_bidirectional() {
                // The peer opening bidirectional streams uses up the capacity for locally opening streams
                assert_eq!(VarInt::from_u8(0), available_outgoing_stream_capacity);
            } else {
                // The peer opening incoming streams does not affect the capacity for opening outgoing streams
                assert_eq!(
                    VarInt::from_u8(*peer_limit),
                    available_outgoing_stream_capacity
                );
            }
        }
    }
}

#[test]
fn accept_returns_remotely_initiated_stream() {
    const STREAMS_TO_OPEN: u64 = 8;

    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client]
        .iter()
        .copied()
    {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client]
            .iter()
            .copied()
        {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
                .iter()
                .copied()
            {
                let mut manager = create_stream_manager(local_ep_type);

                for n in 0..STREAMS_TO_OPEN {
                    let stream_id = StreamId::nth(initiator_type, stream_type, n).unwrap();
                    let (accept_waker, accept_wake_counter) = new_count_waker();

                    // Stream is not yet available
                    assert_eq!(
                        Poll::Pending,
                        manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                    );
                    assert_eq!(accept_wake_counter, 0);

                    // Open the Stream via a data frame
                    assert_eq!(
                        Ok(()),
                        manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], false))
                    );
                    // This must wake up the accept
                    assert_eq!(accept_wake_counter, 1);

                    // Stream is now available
                    assert_eq!(
                        Poll::Ready(Ok(Some(stream_id))),
                        manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                    );
                }

                // Try to accept multiple streams at once
                let stream_id_1 =
                    StreamId::nth(initiator_type, stream_type, STREAMS_TO_OPEN).unwrap();
                let stream_id_2 =
                    StreamId::nth(initiator_type, stream_type, STREAMS_TO_OPEN + 1).unwrap();
                let (accept_waker, accept_wake_counter) = new_count_waker();

                // Stream is not yet available
                assert_eq!(
                    Poll::Pending,
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                );
                assert_eq!(accept_wake_counter, 0);

                // Open 2 streams via a data frame
                assert_eq!(
                    Ok(()),
                    manager.on_data(&stream_data(stream_id_2, VarInt::from_u32(0), &[], false))
                );
                assert_eq!(accept_wake_counter, 1);

                // Streams are now available
                assert_eq!(
                    Poll::Ready(Ok(Some(stream_id_1))),
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                );
                assert_eq!(
                    Poll::Ready(Ok(Some(stream_id_2))),
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                );
                assert_eq!(
                    Poll::Pending,
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                );

                // Check Stream ID exhaustion
                *manager.inner.accept_state.next_stream_mut(stream_type) = None;

                assert_eq!(
                    Poll::Ready(Err(connection::Error::StreamIdExhausted)),
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                );
            }
        }
    }
}

#[test]
fn accept_returns_opened_streams_of_any_type() {
    const STREAMS_TO_OPEN: u64 = 8;

    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client]
        .iter()
        .copied()
    {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client]
            .iter()
            .copied()
        {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }

            let mut manager = create_stream_manager(local_ep_type);
            let (accept_waker, _accept_wake_counter) = new_count_waker();

            let mut streams = std::collections::HashSet::new();

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
                .iter()
                .copied()
            {
                // there are 2 different types of streams that can be created by this type of endpoint
                for n in 0..(STREAMS_TO_OPEN / 2) {
                    let stream_id = StreamId::nth(initiator_type, stream_type, n).unwrap();

                    // Open the Stream via a data frame
                    assert_eq!(
                        Ok(()),
                        manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], false))
                    );

                    streams.insert(stream_id);
                }
            }

            for _ in 0..STREAMS_TO_OPEN {
                match manager.poll_accept(None, &Context::from_waker(&accept_waker)) {
                    Poll::Ready(Ok(Some(stream_id))) => assert!(
                        streams.remove(&stream_id),
                        "accepted {:?} stream multiple times",
                        stream_id
                    ),
                    other => panic!("unexpected result {:?}", other),
                }
            }

            assert!(
                manager
                    .poll_accept(None, &Context::from_waker(&accept_waker))
                    .is_pending(),
                "all streams should have been accepted"
            );

            assert!(streams.is_empty(), "not all streams were accepted");
        }
    }
}

#[test]
fn accept_notifies_of_any_type() {
    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client]
        .iter()
        .copied()
    {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client]
            .iter()
            .copied()
        {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
                .iter()
                .copied()
            {
                let mut manager = create_stream_manager(local_ep_type);
                let (accept_waker, accept_wake_counter) = new_count_waker();

                assert!(
                    manager
                        .poll_accept(None, &Context::from_waker(&accept_waker))
                        .is_pending(),
                    "all streams should have been accepted"
                );

                let stream_id = StreamId::initial(initiator_type, stream_type);

                // Open the Stream via a data frame
                assert_eq!(
                    Ok(()),
                    manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], false))
                );

                // This must wake up the accept
                assert_eq!(accept_wake_counter, 1);

                // Stream is now available
                assert_eq!(
                    Poll::Ready(Ok(Some(stream_id))),
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                );
            }
        }
    }
}

#[test]
fn accept_notifies_on_both_types() {
    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client]
        .iter()
        .copied()
    {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client]
            .iter()
            .copied()
        {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }
            let mut manager = create_stream_manager(local_ep_type);
            let (accept_waker, accept_wake_counter) = new_count_waker();

            assert!(manager
                .poll_accept(None, &Context::from_waker(&accept_waker))
                .is_pending());

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
                .iter()
                .copied()
            {
                let stream_id = StreamId::initial(initiator_type, stream_type);

                // Open the Stream via a data frame
                assert_eq!(
                    Ok(()),
                    manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], false))
                );
            }

            // This must wake up the accept
            assert_eq!(accept_wake_counter, 2);
        }
    }
}

#[test]
fn accept_returns_opened_streams_even_if_stream_manager_was_closed() {
    const STREAMS_TO_OPEN: u64 = 8;

    for initiator_type in &[endpoint::Type::Server, endpoint::Type::Client] {
        for local_ep_type in &[endpoint::Type::Server, endpoint::Type::Client] {
            let is_remote_initialized = *local_ep_type != *initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
                let mut manager = create_stream_manager(*local_ep_type);
                let (accept_waker, _accept_wake_counter) = new_count_waker();

                for n in 0..STREAMS_TO_OPEN {
                    let stream_id = StreamId::nth(*initiator_type, *stream_type, n).unwrap();

                    // Open the Stream via a data frame
                    assert_eq!(
                        Ok(()),
                        manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], false))
                    );
                }

                // Close the StreamManager
                // This should still allow us to accept Streams
                manager.close(connection::Error::Unspecified);

                for n in 0..STREAMS_TO_OPEN {
                    let stream_id = StreamId::nth(*initiator_type, *stream_type, n).unwrap();
                    assert_eq!(
                        Poll::Ready(Ok(Some(stream_id))),
                        manager
                            .poll_accept(Some(*stream_type), &Context::from_waker(&accept_waker))
                    );
                }

                // Now the error should be visible
                assert_eq!(
                    Poll::Ready(Err(connection::Error::Unspecified)),
                    manager.poll_accept(Some(*stream_type), &Context::from_waker(&accept_waker))
                );
            }
        }
    }
}

#[test]
fn closing_stream_manager_wakes_blocked_accepts() {
    for initiator_type in &[endpoint::Type::Server, endpoint::Type::Client] {
        for local_ep_type in &[endpoint::Type::Server, endpoint::Type::Client] {
            let is_remote_initialized = *local_ep_type != *initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
                let mut manager = create_stream_manager(*local_ep_type);
                let (accept_waker, accept_wake_counter) = new_count_waker();

                assert_eq!(
                    Poll::Pending,
                    manager.poll_accept(Some(*stream_type), &Context::from_waker(&accept_waker))
                );
                assert_eq!(accept_wake_counter, 0);

                // Close the StreamManager
                // This should wake up the accept call
                manager.close(connection::Error::Unspecified);
                assert_eq!(accept_wake_counter, 1);

                // Now the error should be visible
                assert_eq!(
                    Poll::Ready(Err(connection::Error::Unspecified)),
                    manager.poll_accept(Some(*stream_type), &Context::from_waker(&accept_waker))
                );
            }
        }
    }
}

#[test]
fn closing_stream_manager_without_error_returns_none() {
    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client]
        .iter()
        .copied()
    {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client]
            .iter()
            .copied()
        {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional]
                .iter()
                .copied()
            {
                let mut manager = create_stream_manager(local_ep_type);
                let (accept_waker, accept_wake_counter) = new_count_waker();

                assert_eq!(
                    Poll::Pending,
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                );
                assert_eq!(accept_wake_counter, 0);

                // Close the StreamManager
                // This should wake up the accept call
                manager.close(connection::Error::Closed {
                    initiator: endpoint::Location::Local,
                });
                assert_eq!(accept_wake_counter, 1);

                // Now the stream should return None
                assert_eq!(
                    Poll::Ready(Ok(None)),
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                );
            }
        }
    }
}

#[test]
fn add_and_remove_streams_from_on_connection_window_lists() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    // Consume all window
    let current_window =
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window());
    assert_eq!(
        current_window,
        manager
            .with_outgoing_connection_flow_controller(|ctrl| ctrl.acquire_window(current_window))
    );

    // Create some open Streams with interests
    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    let _stream_3 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_4 = try_open(&mut manager, StreamType::Unidirectional).unwrap();

    for stream_id in &[stream_2, stream_1, stream_4] {
        manager.with_asserted_stream(*stream_id, |stream| {
            stream.on_connection_window_available_retrieve_window = 1;
        });
    }

    assert_eq!(
        [stream_2, stream_1, stream_4],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );

    manager.with_asserted_stream(stream_2, |stream| {
        stream.on_connection_window_available_retrieve_window = 0;
    });
    assert_eq!(
        [stream_1, stream_4],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );

    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_connection_window_available_retrieve_window = 0;
    });
    assert_eq!(
        [stream_1],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );

    manager.with_asserted_stream(stream_1, |stream| {
        stream.on_connection_window_available_retrieve_window = 0;
    });
    assert_eq!(
        true,
        manager
            .streams_waiting_for_connection_flow_control_credits()
            .is_empty()
    );
}

#[test]
fn max_data_causes_on_connection_window_available_to_be_called_on_streams() {
    fn assert_connection_window_state(
        manager: &mut AbstractStreamManager<MockStream>,
        stream_id: StreamId,
        expected_on_connection_window_available_call_count: usize,
        expected_on_connection_window_available_retrieve_window: u64,
    ) {
        manager.with_asserted_stream(stream_id, |stream| {
            assert_eq!(
                expected_on_connection_window_available_call_count,
                stream.on_connection_window_available_count
            );
            assert_eq!(
                expected_on_connection_window_available_retrieve_window,
                stream.on_connection_window_available_retrieve_window
            );
        });
    }

    let mut manager = create_stream_manager(endpoint::Type::Server);
    // Consume all window
    let mut current_window =
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window());
    assert_eq!(
        current_window,
        manager
            .with_outgoing_connection_flow_controller(|ctrl| ctrl.acquire_window(current_window))
    );

    // Create some open Streams with interests
    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    let stream_3 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_4 = try_open(&mut manager, StreamType::Unidirectional).unwrap();

    manager.with_asserted_stream(stream_1, |stream| {
        stream.on_connection_window_available_retrieve_window = 1;
    });
    manager.with_asserted_stream(stream_3, |stream| {
        stream.on_connection_window_available_retrieve_window = 1;
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_connection_window_available_retrieve_window = 1;
    });
    assert_eq!(
        [stream_1, stream_3, stream_4],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );

    current_window += 100;
    assert!(manager
        .on_max_data(MaxData {
            maximum_data: current_window,
        })
        .is_ok());
    assert_eq!(
        current_window,
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window())
    );
    assert_eq!(
        VarInt::from_u32(97),
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.available_window())
    );

    for stream_id in &[stream_1, stream_3, stream_4] {
        assert_connection_window_state(&mut manager, *stream_id, 1, 0);
    }
    assert_connection_window_state(&mut manager, stream_2, 0, 0);
    assert_eq!(
        true,
        manager
            .streams_waiting_for_connection_flow_control_credits()
            .is_empty()
    );

    // Let stream_3 grab more of the connection window than it requires
    manager.with_asserted_stream(stream_3, |stream| {
        stream.on_connection_window_available_retrieve_window = 120
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_connection_window_available_retrieve_window = 220
    });

    assert!(manager
        .on_max_data(MaxData {
            maximum_data: current_window,
        })
        .is_ok());

    assert_eq!(
        VarInt::from_u32(0),
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.available_window())
    );

    assert_connection_window_state(&mut manager, stream_3, 2, 23);
    assert_connection_window_state(&mut manager, stream_4, 1, 220);

    // Stream 3 and 4 are still waiting
    assert_eq!(
        [stream_3, stream_4],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );

    current_window += 23;
    assert!(manager
        .on_max_data(MaxData {
            maximum_data: current_window,
        })
        .is_ok());

    assert_connection_window_state(&mut manager, stream_3, 3, 0);
    assert_connection_window_state(&mut manager, stream_4, 1, 220);

    // Stream 4 is still waiting
    assert_eq!(
        [stream_4],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );

    // Setup Stream 1 again
    manager.with_asserted_stream(stream_1, |stream| {
        stream.on_connection_window_available_retrieve_window = 200;
    });
    assert_eq!(
        [stream_4, stream_1],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );

    current_window += 230;
    assert!(manager
        .on_max_data(MaxData {
            maximum_data: current_window,
        })
        .is_ok());

    assert_connection_window_state(&mut manager, stream_1, 2, 190);

    // Stream 1 is still waiting
    assert_eq!(
        [stream_1],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );

    // Do not increase the window, but still query `on_max_data`:
    assert!(manager
        .on_max_data(MaxData {
            maximum_data: current_window,
        })
        .is_ok());
    assert_eq!(
        [stream_1],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );

    current_window += 200;
    assert!(manager
        .on_max_data(MaxData {
            maximum_data: current_window,
        })
        .is_ok());

    assert_connection_window_state(&mut manager, stream_1, 3, 0);

    // All done
    assert_eq!(
        true,
        manager
            .streams_waiting_for_connection_flow_control_credits()
            .is_empty()
    );
}

#[test]
fn add_and_remove_streams_from_delivery_notification_window_lists() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    let _stream_3 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_4 = try_open(&mut manager, StreamType::Unidirectional).unwrap();

    for stream_id in &[stream_2, stream_1, stream_4] {
        manager.with_asserted_stream(*stream_id, |stream| {
            stream.interests.delivery_notifications = true;
        });
    }
    assert_eq!(
        [stream_2, stream_1, stream_4],
        *manager.streams_waiting_for_delivery_notifications()
    );

    manager.with_asserted_stream(stream_2, |stream| {
        stream.interests.delivery_notifications = false;
    });
    assert_eq!(
        [stream_1, stream_4],
        *manager.streams_waiting_for_delivery_notifications()
    );

    manager.with_asserted_stream(stream_4, |stream| {
        stream.interests.delivery_notifications = false;
    });
    assert_eq!(
        [stream_1],
        *manager.streams_waiting_for_delivery_notifications()
    );

    manager.with_asserted_stream(stream_1, |stream| {
        stream.interests.delivery_notifications = false;
    });
    assert_eq!(
        true,
        manager
            .streams_waiting_for_delivery_notifications()
            .is_empty()
    );
}

#[test]
fn on_packet_ack_and_loss_is_forwarded_to_interested_streams() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    let stream_3 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_4 = try_open(&mut manager, StreamType::Unidirectional).unwrap();

    for stream_id in &[stream_2, stream_1, stream_4] {
        let read_waker = read_waker.clone();
        let write_waker = write_waker.clone();
        manager.with_asserted_stream(*stream_id, |stream| {
            stream.interests.delivery_notifications = true;
            stream.read_waker_to_return = Some(read_waker);
            stream.write_waker_to_return = Some(write_waker);
        });
    }

    assert_eq!(
        [stream_2, stream_1, stream_4],
        *manager.streams_waiting_for_delivery_notifications()
    );

    manager.on_packet_ack(&pn(1));
    manager.on_packet_loss(&pn(2));
    manager.on_packet_loss(&pn(3));

    assert_eq!(read_wake_counter, 3 * 3);
    assert_eq!(write_wake_counter, 3 * 3);

    for stream_id in &[stream_2, stream_1, stream_4] {
        manager.with_asserted_stream(*stream_id, |stream| {
            assert_eq!(1, stream.on_packet_ack_count);
            assert_eq!(2, stream.on_packet_loss_count);
        });
    }
    manager.with_asserted_stream(stream_3, |stream| {
        assert_eq!(0, stream.on_packet_ack_count);
        assert_eq!(0, stream.on_packet_loss_count);
    });

    manager.with_asserted_stream(stream_1, |stream| {
        stream.interests.delivery_notifications = false;
    });
    manager.with_asserted_stream(stream_2, |stream| {
        stream.interests.delivery_notifications = false;
    });
    manager.with_asserted_stream(stream_3, |stream| {
        stream.interests.delivery_notifications = true;
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.interests.delivery_notifications = false;
    });

    manager.on_packet_ack(&pn(4));
    manager.on_packet_ack(&pn(5));
    manager.on_packet_loss(&pn(6));

    for stream_id in &[stream_2, stream_1, stream_4] {
        manager.with_asserted_stream(*stream_id, |stream| {
            assert_eq!(1, stream.on_packet_ack_count);
            assert_eq!(2, stream.on_packet_loss_count);
        });
    }

    manager.with_asserted_stream(stream_3, |stream| {
        assert_eq!(2, stream.on_packet_ack_count);
        assert_eq!(1, stream.on_packet_loss_count);
    });
}

#[test]
fn close_is_forwarded_to_all_streams() {
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    // Consume all window
    let current_window =
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window());
    assert_eq!(
        current_window,
        manager
            .with_outgoing_connection_flow_controller(|ctrl| ctrl.acquire_window(current_window))
    );

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    let stream_3 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_4 = try_open(&mut manager, StreamType::Unidirectional).unwrap();

    let read_waker_clone = read_waker.clone();
    manager.with_asserted_stream(stream_1, |stream| {
        stream.set_finalize_on_internal_reset = true;
        stream.on_connection_window_available_retrieve_window = 1000;
        stream.read_waker_to_return = Some(read_waker_clone);
    });
    manager.with_asserted_stream(stream_2, |stream| {
        stream.on_connection_window_available_retrieve_window = 1000;
        stream.read_waker_to_return = Some(read_waker);
    });
    manager.with_asserted_stream(stream_3, |stream| {
        stream.set_finalize_on_internal_reset = true;
        stream.interests.delivery_notifications = true;
        stream.on_transmit_try_write_frames = 1;
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_transmit_try_write_frames = 1;
        stream.write_waker_to_return = Some(write_waker);
    });

    manager.close(connection::Error::Application {
        error: ApplicationErrorCode::new(1).unwrap(),
        initiator: endpoint::Location::Local,
    });
    assert_eq!([stream_2, stream_4], *manager.active_streams());
    assert_eq!(read_wake_counter, 2);
    assert_eq!(write_wake_counter, 1);

    for stream_id in &[stream_2, stream_4] {
        manager.with_asserted_stream(*stream_id, |stream| {
            assert_eq!(1, stream.on_internal_reset_count);
        });
    }

    assert_eq!(
        [stream_2],
        *manager.streams_waiting_for_connection_flow_control_credits()
    );
    assert_eq!(
        true,
        manager
            .streams_waiting_for_delivery_notifications()
            .is_empty()
    );
    assert_eq!([stream_4], *manager.streams_waiting_for_transmission());
}

#[test]
fn add_and_remove_streams_from_transmission_lists() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    // Create some open Streams with interests
    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    let _stream_3 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_4 = try_open(&mut manager, StreamType::Unidirectional).unwrap();

    for stream_id in &[stream_2, stream_1, stream_4] {
        manager.with_asserted_stream(*stream_id, |stream| {
            stream.on_transmit_try_write_frames = 1;
        });
    }

    assert_eq!(
        [stream_2, stream_1, stream_4],
        *manager.streams_waiting_for_transmission()
    );

    manager.with_asserted_stream(stream_2, |stream| {
        stream.on_transmit_try_write_frames = 0;
    });
    assert_eq!(
        [stream_1, stream_4],
        *manager.streams_waiting_for_transmission()
    );

    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_transmit_try_write_frames = 0;
    });
    assert_eq!([stream_1], *manager.streams_waiting_for_transmission());

    manager.with_asserted_stream(stream_1, |stream| {
        stream.on_transmit_try_write_frames = 0;
    });
    assert_eq!(true, manager.streams_waiting_for_transmission().is_empty());
}

#[test]
fn add_and_remove_streams_from_retransmission_lists() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    // Create some open Streams with interests
    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    let _stream_3 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_4 = try_open(&mut manager, StreamType::Unidirectional).unwrap();

    for stream_id in &[stream_2, stream_1, stream_4] {
        manager.with_asserted_stream(*stream_id, |stream| {
            stream.lost_data = true;
            stream.on_transmit_try_write_frames = 1;
        });
    }

    assert_eq!(
        [stream_2, stream_1, stream_4],
        *manager.streams_waiting_for_retransmission()
    );

    manager.with_asserted_stream(stream_2, |stream| {
        stream.on_transmit_try_write_frames = 0;
    });
    assert_eq!(
        [stream_1, stream_4],
        *manager.streams_waiting_for_retransmission()
    );

    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_transmit_try_write_frames = 0;
    });
    assert_eq!([stream_1], *manager.streams_waiting_for_retransmission());

    manager.with_asserted_stream(stream_1, |stream| {
        stream.on_transmit_try_write_frames = 0;
    });
    assert_eq!(
        true,
        manager.streams_waiting_for_retransmission().is_empty()
    );
}

#[test]
fn on_transmit_queries_streams_for_data() {
    fn assert_stream_write_state(
        manager: &mut AbstractStreamManager<MockStream>,
        stream_id: StreamId,
        expected_on_transmit_count: usize,
        expected_on_transmit_try_write: usize,
    ) {
        manager.with_asserted_stream(stream_id, |stream| {
            assert_eq!(expected_on_transmit_count, stream.on_transmit_count);
            assert_eq!(
                expected_on_transmit_try_write,
                stream.on_transmit_try_write_frames
            );
        });
    }

    let mut manager = create_stream_manager(endpoint::Type::Server);
    let mut frame_buffer = OutgoingFrameBuffer::new();

    // Create some open Streams with interests
    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    let stream_3 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_4 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    let stream_5 = try_open(&mut manager, StreamType::Unidirectional).unwrap();

    manager.with_asserted_stream(stream_1, |stream| {
        stream.on_transmit_try_write_frames = 1;
    });
    manager.with_asserted_stream(stream_3, |stream| {
        stream.on_transmit_try_write_frames = 1;
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_transmit_try_write_frames = 1;
    });
    manager.with_asserted_stream(stream_5, |stream| {
        stream.on_transmit_try_write_frames = 1;
        stream.lost_data = true;
    });
    assert_eq!(
        [stream_1, stream_3, stream_4],
        *manager.streams_waiting_for_transmission()
    );
    assert_eq!([stream_5], *manager.streams_waiting_for_retransmission());

    let mut write_context = MockWriteContext::new(
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        endpoint::Type::Server,
    );

    write_context.transmission_constraint = transmission::Constraint::CongestionLimited;

    assert!(manager.on_transmit(&mut write_context).is_ok());
    assert!(
        write_context.frame_buffer.is_empty(),
        "no frames are written when congestion limited"
    );

    write_context.transmission_constraint = transmission::Constraint::RetransmissionOnly;

    assert!(manager.on_transmit(&mut write_context).is_ok());

    // Only lost data may be written when constrained to retransmission only
    assert_stream_write_state(&mut manager, stream_5, 1, 0);
    assert_eq!(1, write_context.frame_buffer.len());
    assert_eq!(
        true,
        manager.streams_waiting_for_retransmission().is_empty()
    );

    write_context.transmission_constraint = transmission::Constraint::None;

    assert!(manager.on_transmit(&mut write_context).is_ok());

    for stream_id in &[stream_1, stream_3, stream_4] {
        assert_stream_write_state(&mut manager, *stream_id, 1, 0);
    }

    // All streams have written a frame
    assert_eq!(4, frame_buffer.len());
    frame_buffer.clear();
    assert_eq!(true, manager.streams_waiting_for_transmission().is_empty());

    manager.with_asserted_stream(stream_1, |stream| {
        stream.on_transmit_try_write_frames = 10;
    });
    manager.with_asserted_stream(stream_2, |stream| {
        stream.on_transmit_try_write_frames = 10;
    });
    manager.with_asserted_stream(stream_3, |stream| {
        stream.on_transmit_try_write_frames = 10;
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_transmit_try_write_frames = 10;
    });

    frame_buffer.set_error_write_after_n_frames(15);
    let mut write_context = MockWriteContext::new(
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        endpoint::Type::Server,
    );

    assert_eq!(
        Err(OnTransmitError::CouldNotWriteFrame),
        manager.on_transmit(&mut write_context)
    );
    assert_stream_write_state(&mut manager, stream_1, 2, 0);
    assert_stream_write_state(&mut manager, stream_2, 1, 5);
    assert_stream_write_state(&mut manager, stream_3, 1, 10);
    assert_stream_write_state(&mut manager, stream_4, 1, 10);

    assert_eq!(15, frame_buffer.len());
    frame_buffer.clear();
    assert_eq!(
        [stream_2, stream_3, stream_4],
        *manager.streams_waiting_for_transmission()
    );

    // Query stream_1 for data again. It should however be asked at the end
    manager.with_asserted_stream(stream_1, |stream| {
        stream.on_transmit_try_write_frames = 10;
    });
    assert_eq!(
        [stream_2, stream_3, stream_4, stream_1],
        *manager.streams_waiting_for_transmission()
    );

    frame_buffer.set_error_write_after_n_frames(15);
    let mut write_context = MockWriteContext::new(
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        endpoint::Type::Server,
    );

    assert_eq!(
        Err(OnTransmitError::CouldNotWriteFrame),
        manager.on_transmit(&mut write_context)
    );
    assert_stream_write_state(&mut manager, stream_1, 2, 10);
    assert_stream_write_state(&mut manager, stream_2, 2, 0);
    assert_stream_write_state(&mut manager, stream_3, 2, 0);
    assert_stream_write_state(&mut manager, stream_4, 2, 10);

    assert_eq!(15, frame_buffer.len());
    frame_buffer.clear();
    assert_eq!(
        [stream_4, stream_1],
        *manager.streams_waiting_for_transmission()
    );

    frame_buffer.set_error_write_after_n_frames(5);
    let mut write_context = MockWriteContext::new(
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        endpoint::Type::Server,
    );

    assert_eq!(
        Err(OnTransmitError::CouldNotWriteFrame),
        manager.on_transmit(&mut write_context)
    );
    assert_stream_write_state(&mut manager, stream_1, 2, 10);
    assert_stream_write_state(&mut manager, stream_2, 2, 0);
    assert_stream_write_state(&mut manager, stream_3, 2, 0);
    assert_stream_write_state(&mut manager, stream_4, 3, 5);

    assert_eq!(5, frame_buffer.len());
    frame_buffer.clear();
    assert_eq!(
        [stream_4, stream_1],
        *manager.streams_waiting_for_transmission()
    );

    frame_buffer.set_error_write_after_n_frames(5);
    let mut write_context = MockWriteContext::new(
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        endpoint::Type::Server,
    );

    assert_eq!(
        Err(OnTransmitError::CouldNotWriteFrame),
        manager.on_transmit(&mut write_context)
    );
    assert_stream_write_state(&mut manager, stream_1, 3, 10);
    assert_stream_write_state(&mut manager, stream_2, 2, 0);
    assert_stream_write_state(&mut manager, stream_3, 2, 0);
    assert_stream_write_state(&mut manager, stream_4, 4, 0);

    assert_eq!(5, frame_buffer.len());
    frame_buffer.clear();
    assert_eq!([stream_1], *manager.streams_waiting_for_transmission());

    frame_buffer.set_error_write_after_n_frames(11);
    let mut write_context = MockWriteContext::new(
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        endpoint::Type::Server,
    );

    assert_eq!(Ok(()), manager.on_transmit(&mut write_context));
    assert_stream_write_state(&mut manager, stream_1, 4, 0);
    assert_stream_write_state(&mut manager, stream_2, 2, 0);
    assert_stream_write_state(&mut manager, stream_3, 2, 0);
    assert_stream_write_state(&mut manager, stream_4, 4, 0);

    assert_eq!(10, frame_buffer.len());
    frame_buffer.clear();
    assert_eq!(true, manager.streams_waiting_for_transmission().is_empty());
}

fn invalid_stream_id(local_ep_type: endpoint::Type) -> StreamId {
    StreamId::nth(local_ep_type, StreamType::Bidirectional, 100_000).unwrap()
}

#[test]
fn forwards_on_data() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    manager.with_asserted_stream(stream_1, |stream| {
        stream.read_waker_to_return = Some(read_waker);
    });

    let mut frame = StreamFrame {
        stream_id: stream_1.into(),
        offset: VarInt::from_u32(0),
        is_last_frame: false,
        is_fin: false,
        data: &[][..],
    };

    assert_eq!(Ok(()), manager.on_data(&frame));
    assert_eq!(read_wake_counter, 1);
    assert_eq!(write_wake_counter, 0);

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_data_count, 1);
        stream.write_waker_to_return = Some(write_waker);
        stream.next_packet_error = Some(TransportError::INTERNAL_ERROR);
    });

    assert_is_transport_error(manager.on_data(&frame), TransportError::INTERNAL_ERROR);
    assert_eq!(read_wake_counter, 3);
    assert_eq!(write_wake_counter, 2);

    // Check that the Streams are reset due to the error
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });
    manager.with_asserted_stream(stream_2, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });

    // Check invalid stream ID
    frame.stream_id = invalid_stream_id(endpoint::Type::Server).into();
    assert_is_transport_error(manager.on_data(&frame), TransportError::STREAM_STATE_ERROR);
}

#[test]
fn forwards_on_stream_data_blocked() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    manager.with_asserted_stream(stream_1, |stream| {
        stream.read_waker_to_return = Some(read_waker);
    });

    let mut frame = StreamDataBlocked {
        stream_id: stream_1.into(),
        stream_data_limit: VarInt::from_u32(1123),
    };

    assert_eq!(Ok(()), manager.on_stream_data_blocked(&frame));
    assert_eq!(read_wake_counter, 1);
    assert_eq!(write_wake_counter, 0);

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_stream_data_blocked_count, 1);
        assert_eq!(Some(frame), stream.last_on_stream_data_blocked);
        stream.next_packet_error = Some(TransportError::INTERNAL_ERROR);
        stream.write_waker_to_return = Some(write_waker);
    });

    assert_is_transport_error(
        manager.on_stream_data_blocked(&frame),
        TransportError::INTERNAL_ERROR,
    );
    assert_eq!(read_wake_counter, 3);
    assert_eq!(write_wake_counter, 2);

    // Check that the Streams are reset due to the error
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });
    manager.with_asserted_stream(stream_2, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });

    // Check invalid stream ID
    frame.stream_id = invalid_stream_id(endpoint::Type::Server).into();
    assert_is_transport_error(
        manager.on_stream_data_blocked(&frame),
        TransportError::STREAM_STATE_ERROR,
    );
}

#[test]
fn forwards_on_max_stream_data() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    manager.with_asserted_stream(stream_1, |stream| {
        stream.read_waker_to_return = Some(read_waker);
    });

    let mut frame = MaxStreamData {
        stream_id: stream_1.into(),
        maximum_stream_data: VarInt::from_u32(1694),
    };

    assert_eq!(Ok(()), manager.on_max_stream_data(&frame));
    assert_eq!(read_wake_counter, 1);
    assert_eq!(write_wake_counter, 0);

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_max_stream_data_count, 1);
        assert_eq!(Some(frame), stream.last_max_stream_data);
        stream.next_packet_error = Some(TransportError::INTERNAL_ERROR);
        stream.write_waker_to_return = Some(write_waker);
    });

    assert_is_transport_error(
        manager.on_max_stream_data(&frame),
        TransportError::INTERNAL_ERROR,
    );
    assert_eq!(read_wake_counter, 3);
    assert_eq!(write_wake_counter, 2);

    // Check that the Streams are reset due to the error
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });
    manager.with_asserted_stream(stream_2, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });

    // Check invalid stream ID
    frame.stream_id = invalid_stream_id(endpoint::Type::Server).into();
    assert_is_transport_error(
        manager.on_max_stream_data(&frame),
        TransportError::STREAM_STATE_ERROR,
    );
}

#[test]
fn forwards_on_stop_sending() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    manager.with_asserted_stream(stream_1, |stream| {
        stream.read_waker_to_return = Some(read_waker);
    });

    let mut frame = StopSending {
        stream_id: stream_1.into(),
        application_error_code: VarInt::from_u32(9_988_776),
    };

    assert_eq!(Ok(()), manager.on_stop_sending(&frame));
    assert_eq!(read_wake_counter, 1);
    assert_eq!(write_wake_counter, 0);

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_stop_sending_count, 1);
        assert_eq!(Some(frame), stream.last_stop_sending);
        stream.next_packet_error = Some(TransportError::INTERNAL_ERROR);
        stream.write_waker_to_return = Some(write_waker);
    });

    assert_is_transport_error(
        manager.on_stop_sending(&frame),
        TransportError::INTERNAL_ERROR,
    );
    assert_eq!(read_wake_counter, 3);
    assert_eq!(write_wake_counter, 2);

    // Check that the Streams are reset due to the error
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });
    manager.with_asserted_stream(stream_2, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });

    // Check invalid stream ID
    frame.stream_id = invalid_stream_id(endpoint::Type::Server).into();
    assert_is_transport_error(
        manager.on_stop_sending(&frame),
        TransportError::STREAM_STATE_ERROR,
    );
}

#[test]
fn forwards_on_reset() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    manager.with_asserted_stream(stream_1, |stream| {
        stream.read_waker_to_return = Some(read_waker);
    });

    let mut frame = ResetStream {
        stream_id: stream_1.into(),
        application_error_code: VarInt::from_u32(3_928_776),
        final_size: VarInt::from_u32(44_222_111),
    };

    assert_eq!(Ok(()), manager.on_reset_stream(&frame));
    assert_eq!(read_wake_counter, 1);
    assert_eq!(write_wake_counter, 0);

    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_reset_count, 1);
        assert_eq!(Some(frame), stream.last_reset);
        stream.write_waker_to_return = Some(write_waker);
        stream.read_waker_to_return = None;
        stream.next_packet_error = Some(TransportError::INTERNAL_ERROR);
    });

    assert_is_transport_error(
        manager.on_reset_stream(&frame),
        TransportError::INTERNAL_ERROR,
    );
    assert_eq!(read_wake_counter, 1);
    assert_eq!(write_wake_counter, 2);

    // Check that the Streams are reset due to the error
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });
    manager.with_asserted_stream(stream_2, |stream| {
        assert_eq!(stream.on_internal_reset_count, 1);
    });

    // Check invalid stream ID
    frame.stream_id = invalid_stream_id(endpoint::Type::Server).into();
    assert_is_transport_error(
        manager.on_reset_stream(&frame),
        TransportError::STREAM_STATE_ERROR,
    );
    assert_eq!(read_wake_counter, 1);
    assert_eq!(write_wake_counter, 2);
}

#[test]
fn forwards_poll_pop() {
    let (mut wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();
    let (waker, _wake_counter) = new_count_waker();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    let ctx = Context::from_waker(&waker);
    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().receive(&mut [Bytes::new()]),
            Some(&ctx),
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.poll_pop_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert!(manager.transmission_interest().is_none());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Err(StreamError::MaxStreamDataSizeExceeded),
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().receive(&mut [Bytes::new()]),
            Some(&ctx)
        )
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Err(StreamError::InvalidStream),
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().receive(&mut [Bytes::new()]),
            Some(&ctx)
        )
    );
}

#[test]
fn forwards_stop_sending() {
    let (mut wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let error = ApplicationErrorCode::new(0x12_3456).unwrap();

    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().stop_sending(error),
            None,
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.stop_sending_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert!(manager.transmission_interest().is_none());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Err(StreamError::MaxStreamDataSizeExceeded),
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().stop_sending(error),
            None,
        )
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Err(StreamError::InvalidStream),
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().stop_sending(error),
            None,
        )
    );
}

#[test]
fn forwards_poll_push() {
    let (mut wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();
    let (waker, _wake_counter) = new_count_waker();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let data = Bytes::from_static(b"1234");

    let ctx = Context::from_waker(&waker);
    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().send(&mut [data.clone()]),
            Some(&ctx)
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.poll_push_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert!(manager.transmission_interest().is_none());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Err(StreamError::MaxStreamDataSizeExceeded),
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().send(&mut [data.clone()]),
            Some(&ctx)
        )
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Err(StreamError::InvalidStream),
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().send(&mut [data]),
            Some(&ctx)
        )
    );
}

#[test]
fn forwards_poll_finish() {
    let (mut wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();
    let (waker, _wake_counter) = new_count_waker();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    let ctx = Context::from_waker(&waker);
    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().finish().flush(),
            Some(&ctx)
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.poll_finish_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert!(manager.transmission_interest().is_none());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Err(StreamError::MaxStreamDataSizeExceeded),
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().finish().flush(),
            Some(&ctx)
        )
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Err(StreamError::InvalidStream),
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().finish().flush(),
            Some(&ctx)
        )
    );
}

#[test]
fn forwards_reset() {
    let (mut wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let error = ApplicationErrorCode::new(0x12_3456).unwrap();

    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().reset(error),
            None,
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.reset_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert!(manager.transmission_interest().is_none());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Err(StreamError::MaxStreamDataSizeExceeded),
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().reset(error),
            None,
        )
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Err(StreamError::InvalidStream),
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            ops::Request::default().reset(error),
            None,
        )
    );
}
