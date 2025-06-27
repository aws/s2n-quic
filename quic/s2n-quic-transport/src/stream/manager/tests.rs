// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for `StreamManager`

use super::*;
use crate::{
    connection::{
        self, finalization::Provider, InternalConnectionId, InternalConnectionIdGenerator,
        Limits as ConnectionLimits,
    },
    contexts::{ConnectionApiCallContext, OnTransmitError, WriteContext},
    endpoint,
    recovery::{RttEstimator, DEFAULT_INITIAL_RTT},
    stream::{
        controller::MAX_STREAMS_SYNC_FRACTION,
        manager_api::Manager as _,
        stream_impl::StreamConfig,
        stream_interests::{StreamInterestProvider, StreamInterests},
        testing::*,
        AbstractStreamManager, StreamError, StreamEvents, StreamTrait,
    },
    sync::DEFAULT_SYNC_PERIOD,
    transmission,
    transmission::interest::Provider as TransmissionInterestProvider,
    wakeup_queue::{WakeupHandle, WakeupQueue},
};
use alloc::{collections::VecDeque, sync::Arc};
use bytes::Bytes;
use core::{
    task::{Context, Poll, Waker},
    time::Duration,
};
use futures_test::task::new_count_waker;
use s2n_quic_core::{
    ack::Set as AckSet,
    application::Error as ApplicationErrorCode,
    frame::{
        stream::StreamRef, DataBlocked, Frame, MaxData, MaxStreamData, MaxStreams, ResetStream,
        StopSending, Stream as StreamFrame, StreamDataBlocked, StreamsBlocked,
    },
    packet::number::{PacketNumberRange, PacketNumberSpace},
    stream::{ops, StreamId, StreamType},
    time::{
        clock::testing as time,
        timer::{self, Provider as _},
        Clock as _, Timestamp,
    },
    transport::{
        parameters::{InitialFlowControlLimits, InitialStreamLimits},
        Error as TransportError,
    },
    varint::VarInt,
};

#[derive(Debug)]
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
    update_blocked_sync_period_count: usize,
    on_timeout_count: usize,
    on_internal_reset_count: usize,
    on_transmit_try_write_frames: usize,
    on_transmit_count: usize,
    on_transmit_limit: Option<usize>,
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
    fn stream_interests(&self, interests: &mut StreamInterests) {
        interests.merge(&self.interests);

        interests.connection_flow_control_credits |=
            self.on_connection_window_available_retrieve_window > 0;

        if self.on_transmit_try_write_frames > 0 {
            interests.with_transmission(|query| {
                use transmission::interest::Query;

                if self.lost_data {
                    query.on_lost_data()
                } else {
                    query.on_new_data()
                }
            });
        }
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
            interests: StreamInterests {
                retained: true,
                ..Default::default()
            },
            on_connection_window_available_count: 0,
            on_connection_window_available_retrieve_window: 0,
            on_packet_ack_count: 0,
            on_packet_loss_count: 0,
            update_blocked_sync_period_count: 0,
            on_timeout_count: 0,
            on_internal_reset_count: 0,
            on_data_count: 0,
            on_reset_count: 0,
            on_stream_data_blocked_count: 0,
            on_stop_sending_count: 0,
            on_max_stream_data_count: 0,
            on_transmit_count: 0,
            on_transmit_try_write_frames: 0,
            on_transmit_limit: None,
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

    fn update_blocked_sync_period(&mut self, _blocked_sync_period: Duration) {
        self.update_blocked_sync_period_count += 1;
    }

    fn on_timeout(&mut self, _now: Timestamp) {
        self.on_timeout_count += 1;
    }

    fn on_internal_reset(&mut self, _error: StreamError, events: &mut StreamEvents) {
        self.on_internal_reset_count += 1;
        if self.set_finalize_on_internal_reset {
            self.interests.retained = false;
        }
        self.store_wakers(events);
    }

    fn on_flush(&mut self, error: StreamError, events: &mut StreamEvents) {
        // for testing purposes, it's the same as a reset
        self.on_internal_reset(error, events);
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        self.on_transmit_count += 1;
        let count = self
            .on_transmit_try_write_frames
            .min(self.on_transmit_limit.unwrap_or(usize::MAX));
        for _ in 0..count {
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

impl timer::Provider for MockStream {
    fn timers<Q: timer::Query>(&self, _query: &mut Q) -> timer::Result {
        Ok(())
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
        max_open_remote_bidirectional_streams: VarInt::from_u32(128),
        max_open_remote_unidirectional_streams: VarInt::from_u32(128),
    }
}

fn create_wakeup_queue_and_handle() -> (
    WakeupQueue<InternalConnectionId>,
    Arc<WakeupHandle<InternalConnectionId>>,
) {
    let wakeup_queue = WakeupQueue::new();
    let connection_id = InternalConnectionIdGenerator::new().generate_id();
    let wakeup_handle = Arc::new(wakeup_queue.create_wakeup_handle(connection_id));
    (wakeup_queue, wakeup_handle)
}

/// Asserts that a given number of wakeups had been enqueued
#[track_caller]
fn assert_wakeups(wakeup_queue: &mut WakeupQueue<InternalConnectionId>, expected_wakeups: usize) {
    let mut dequeued_wakeups = VecDeque::new();
    let (waker, _counter) = new_count_waker();
    wakeup_queue.poll_pending_wakeups(&mut dequeued_wakeups, &Context::from_waker(&waker));

    assert_eq!(expected_wakeups, dequeued_wakeups.len());
}

fn create_stream_manager(local_ep_type: endpoint::Type) -> AbstractStreamManager<MockStream> {
    let initial_local_limits = create_default_initial_flow_control_limits();
    let initial_peer_limits = create_default_initial_flow_control_limits();

    // set local limits high so that they are not a constraint
    let limits = ConnectionLimits::default()
        .with_max_open_local_bidirectional_streams(1000)
        .unwrap()
        .with_max_open_local_unidirectional_streams(1000)
        .unwrap();

    AbstractStreamManager::<MockStream>::new(
        &limits,
        local_ep_type,
        initial_local_limits,
        initial_peer_limits,
        DEFAULT_INITIAL_RTT,
    )
}

/// Tries to open a new stream and returns an error if we're at the limit
fn try_open(
    manager: &mut AbstractStreamManager<MockStream>,
    stream_type: StreamType,
) -> Result<StreamId, connection::Error> {
    let (accept_waker, _accept_wake_counter) = new_count_waker();
    let (_wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
    let mut token = connection::OpenToken::new();

    match manager.poll_open_local_stream(
        stream_type,
        &mut token,
        &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
        &Context::from_waker(&accept_waker),
    ) {
        Poll::Ready(res) => res,
        Poll::Pending => Err(connection::Error::unspecified()),
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
    for local_ep_type in [endpoint::Type::Client, endpoint::Type::Server] {
        for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
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

            assert_matches!(
                try_open(&mut manager, stream_type),
                Err(connection::Error::StreamIdExhausted { .. }),
            );
        }
    }
}

#[test]
fn open_returns_error_after_close() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    manager.close(connection::Error::unspecified());
    assert!(matches!(
        try_open(&mut manager, StreamType::Bidirectional),
        Err(connection::Error::Unspecified { .. })
    ));
}

#[test]
fn returns_finalization_interest_after_last_stream_is_drained() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    let (_wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();

    assert_eq!(0, manager.active_streams().len());
    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    assert_eq!(1, manager.active_streams().len());
    let stream_2 = try_open(&mut manager, StreamType::Unidirectional).unwrap();
    assert_eq!(2, manager.active_streams().len());
    assert!(manager.finalization_status().is_idle());

    let error = ApplicationErrorCode::new(0).unwrap();

    manager.close(connection::Error::application(error));
    assert!(manager.finalization_status().is_draining());

    // The first stream is immediately interested in finalization and should
    // therefore be collected
    manager.with_asserted_stream(stream_1, |stream| {
        stream.interests.retained = false;
    });
    assert_eq!(1, manager.active_streams().len());
    assert!(manager.finalization_status().is_draining());

    // The second stream is not yet interested in finalization
    assert!(manager
        .poll_request(
            stream_2,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().reset(error),
            None,
        )
        .is_ok());
    assert_eq!(1, manager.active_streams().len());
    assert!(manager.finalization_status().is_draining());

    // Let the last stream return the finalization interest
    manager.with_asserted_stream(stream_2, |stream| {
        stream.interests.retained = false;
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

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
//= type=test
//# MAX_STREAMS frames that do not increase the stream limit MUST be ignored.

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
//= type=test
//# MAX_STREAMS frames that do not increase the stream limit MUST be ignored.

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
//= type=test
//# Endpoints MUST NOT exceed the limit set by their peer.

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
//= type=test
//# An endpoint MUST NOT open more streams than permitted by the current
//# stream limit set by its peer.
#[test]
fn max_streams_replenishes_stream_control_capacity() {
    let mut manager = create_stream_manager(endpoint::Type::Server);
    let (waker, _counter) = new_count_waker();
    let (mut wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
    let mut token = connection::OpenToken::new();

    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
        let current_max_streams = manager.with_stream_controller(|ctrl| {
            ctrl.remote_initiated_max_streams_latest_value(stream_type)
        });

        // Open and close up to the current max streams limit to ensure we are blocked on the
        // peer's max streams limit and not the local concurrent stream limit.
        for i in 0..*current_max_streams {
            let stream_id = StreamId::nth(endpoint::Type::Server, stream_type, i).unwrap();
            assert!(manager
                .with_stream_controller(|ctrl| {
                    ctrl.poll_open_local_stream(
                        stream_id.stream_type(),
                        &mut token,
                        &Context::from_waker(&waker),
                    )
                })
                .is_ready());

            manager.with_stream_controller(|ctrl| ctrl.on_close_stream(stream_id));
        }

        assert!(manager
            .poll_open_local_stream(
                stream_type,
                &mut token,
                &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                &Context::from_waker(&waker)
            )
            .is_pending());

        // The stream controller would already have transmission interest so no additional wakeup is needed
        assert_wakeups(&mut wakeup_queue, 0);

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
                    |ctrl| ctrl.available_local_initiated_stream_capacity(stream_type)
                )
            );
        }

        assert!(manager
            .poll_open_local_stream(
                stream_type,
                &mut token,
                &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                &Context::from_waker(&waker)
            )
            .is_ready());

        // The stream controller would already have transmission interest so no additional wakeup is needed
        assert_wakeups(&mut wakeup_queue, 0);
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
//= type=test
//# An endpoint MUST NOT wait
//# to receive this signal before advertising additional credit, since
//# doing so will mean that the peer will be blocked for at least an
//# entire round trip
#[test]
fn peer_closing_streams_transmits_max_streams() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
        let current_max_streams = manager.with_stream_controller(|ctrl| {
            ctrl.remote_initiated_max_streams_latest_value(stream_type)
        });

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
            manager.get_transmission_interest()
        );

        // The peer closes MAX_STREAMS_SYNC_FRACTION of streams
        let streams_to_close = current_max_streams / MAX_STREAMS_SYNC_FRACTION;

        for i in 0..*streams_to_close {
            let stream_id = StreamId::nth(endpoint::Type::Client, stream_type, i).unwrap();
            manager.with_asserted_stream(stream_id, |stream| {
                stream.interests.retained = false;
            });
        }

        assert_eq!(
            transmission::Interest::NewData,
            manager.get_transmission_interest()
        );

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );
        let packet_number = write_context.packet_number();
        assert!(manager.on_transmit(&mut write_context).is_ok());

        let expected_frame = Frame::MaxStreams(MaxStreams {
            stream_type,
            maximum_streams: current_max_streams + streams_to_close,
        });

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );

        assert_eq!(
            transmission::Interest::None,
            manager.get_transmission_interest()
        );

        manager.on_packet_loss(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            transmission::Interest::LostData,
            manager.get_transmission_interest()
        );

        let packet_number = write_context.packet_number();
        assert!(manager.on_transmit(&mut write_context).is_ok());

        manager.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            transmission::Interest::None,
            manager.get_transmission_interest()
        );
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
//= type=test
//# An endpoint that is unable to open a new stream due to the peer's
//# limits SHOULD send a STREAMS_BLOCKED frame (Section 19.14).

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.14
//= type=test
//# A sender SHOULD send a STREAMS_BLOCKED frame (type=0x16 or 0x17) when
//# it wishes to open a stream but is unable to do so due to the maximum
//# stream limit set by its peer; see Section 19.11.
#[test]
fn send_streams_blocked_frame_when_blocked_by_peer() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
        let (waker, _) = new_count_waker();
        let (_wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
        let mut token = connection::OpenToken::new();

        let mut opened_streams = VarInt::from_u8(0);

        // Open streams until blocked
        while manager
            .poll_open_local_stream(
                stream_type,
                &mut token,
                &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                &Context::from_waker(&waker),
            )
            .is_ready()
        {
            opened_streams += 1;
        }

        assert_eq!(
            transmission::Interest::NewData,
            manager.get_transmission_interest(),
            "stream_type:{stream_type:?} opened_streams:{opened_streams}"
        );

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );
        let packet_number = write_context.packet_number();
        assert!(manager.on_transmit(&mut write_context).is_ok());

        let expected_frame = Frame::StreamsBlocked(StreamsBlocked {
            stream_type,
            stream_limit: opened_streams,
        });

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );

        assert_eq!(
            transmission::Interest::None,
            manager.get_transmission_interest()
        );

        manager.on_packet_loss(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            transmission::Interest::LostData,
            manager.get_transmission_interest()
        );

        let packet_number = write_context.packet_number();
        assert!(manager.on_transmit(&mut write_context).is_ok());

        manager.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            transmission::Interest::None,
            manager.get_transmission_interest()
        );

        let expected_transmission_backoff = 4;
        let expected_next_stream_blocked_time =
            write_context.current_time + DEFAULT_SYNC_PERIOD * expected_transmission_backoff;
        assert_eq!(
            Some(expected_next_stream_blocked_time),
            manager.next_expiration()
        );

        manager.on_timeout(expected_next_stream_blocked_time);

        // Another STREAM_BLOCKED frame should be sent
        assert_eq!(
            transmission::Interest::NewData,
            manager.get_transmission_interest()
        );

        // We get more credit from the peer so we should no longer send STREAM_BLOCKED
        assert!(manager
            .on_max_streams(&MaxStreams {
                stream_type,
                maximum_streams: VarInt::from_u32(200),
            })
            .is_ok());

        assert_eq!(
            transmission::Interest::None,
            manager.get_transmission_interest()
        );

        // Close currently open streams to not block on local limits
        for i in 0..*opened_streams {
            let stream_id = StreamId::nth(endpoint::Type::Server, stream_type, i).unwrap();
            manager.with_asserted_stream(stream_id, |stream| stream.interests.retained = false);
        }

        // Clear out the MAX_STREAMS frame
        assert!(manager.on_transmit(&mut write_context).is_ok());
        write_context.frame_buffer.clear();

        // Open streams until blocked
        while manager
            .poll_open_local_stream(
                stream_type,
                &mut token,
                &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                &Context::from_waker(&waker),
            )
            .is_ready()
        {
            opened_streams += 1;
        }

        // Another STREAM_BLOCKED frame should be sent with the updated MAX_STREAMS value
        assert_eq!(
            transmission::Interest::NewData,
            manager.get_transmission_interest()
        );

        assert!(manager.on_transmit(&mut write_context).is_ok());

        let expected_frame = Frame::StreamsBlocked(StreamsBlocked {
            stream_type,
            stream_limit: VarInt::from_u32(200),
        });

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );
    }
}

#[test]
fn streams_blocked_period() {
    let (_wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();

    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
        let block_func = |manager: &mut AbstractStreamManager<MockStream>| {
            let (waker, _) = new_count_waker();
            let mut token = connection::OpenToken::new();

            let mut opened_streams = VarInt::from_u8(0);

            // Open streams until blocked
            while manager
                .poll_open_local_stream(
                    stream_type,
                    &mut token,
                    &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                    &Context::from_waker(&waker),
                )
                .is_ready()
            {
                opened_streams += 1;
            }
        };

        assert_blocked_frame_based_on_pto(block_func);
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
//= type=test
//# To keep the
//# connection from closing, a sender that is flow control limited SHOULD
//# periodically send a STREAM_DATA_BLOCKED or DATA_BLOCKED frame when it
//# has no ack-eliciting packets in flight.
#[test]
fn data_blocked_period() {
    let blocked_func = |manager: &mut AbstractStreamManager<MockStream>| {
        let current_window =
            manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window());
        manager.with_outgoing_connection_flow_controller(|ctrl| {
            ctrl.acquire_window(current_window + 1)
        });
    };

    assert_blocked_frame_based_on_pto(blocked_func);
}

fn assert_blocked_frame_based_on_pto<F>(mut block_func: F)
where
    F: FnMut(&mut AbstractStreamManager<MockStream>),
{
    let mut manager = create_stream_manager(endpoint::Type::Server);

    block_func(&mut manager);

    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut write_context = MockWriteContext::new(
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );
    let packet_number = write_context.packet_number();
    assert!(manager.on_transmit(&mut write_context).is_ok());

    let rtt_estimator = RttEstimator::new(Duration::from_millis(100));
    manager.on_rtt_update(&rtt_estimator, time::now());
    manager.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

    let expected_transmission_backoff = 2;

    let expected_next_blocked_time = write_context.current_time
        + rtt_estimator.pto_period(
            expected_transmission_backoff,
            PacketNumberSpace::ApplicationData,
        );
    assert_eq!(Some(expected_next_blocked_time), manager.next_expiration());
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
//= type=test
//# A sender SHOULD send a
//# STREAM_DATA_BLOCKED or DATA_BLOCKED frame to indicate to the receiver
//# that it has data to write but is blocked by flow control limits.

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.12
//= type=test
//# A sender SHOULD send a DATA_BLOCKED frame (type=0x14) when it wishes
//# to send data, but is unable to do so due to connection-level flow
//# control; see Section 4.
#[test]
fn send_data_blocked_frame_when_blocked_by_connection_flow_limits() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    // Consume all window
    let current_window =
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window());
    assert_eq!(
        current_window,
        manager
            .with_outgoing_connection_flow_controller(|ctrl| ctrl.acquire_window(current_window))
    );

    // No DATA_BLOCKED is sent, since the window has been fully consumed, but not exceeded
    assert_eq!(
        transmission::Interest::None,
        manager.get_transmission_interest()
    );

    // Try acquiring one more byte to exceed the window
    assert_eq!(
        VarInt::from_u32(0),
        manager.with_outgoing_connection_flow_controller(
            |ctrl| ctrl.acquire_window(VarInt::from_u32(1))
        )
    );

    assert_eq!(
        transmission::Interest::NewData,
        manager.get_transmission_interest()
    );

    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut write_context = MockWriteContext::new(
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );
    let packet_number = write_context.packet_number();
    assert!(manager.on_transmit(&mut write_context).is_ok());

    let expected_frame = Frame::DataBlocked(DataBlocked {
        data_limit: current_window,
    });

    assert_eq!(
        expected_frame,
        write_context.frame_buffer.pop_front().unwrap().as_frame()
    );
    write_context.frame_buffer.clear();

    assert_eq!(
        transmission::Interest::None,
        manager.get_transmission_interest()
    );

    manager.on_packet_loss(&PacketNumberRange::new(packet_number, packet_number));

    assert_eq!(
        transmission::Interest::LostData,
        manager.get_transmission_interest()
    );

    let packet_number = write_context.packet_number();
    assert!(manager.on_transmit(&mut write_context).is_ok());
    write_context.frame_buffer.clear();

    manager.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

    assert_eq!(
        transmission::Interest::None,
        manager.get_transmission_interest()
    );

    let expected_transmission_backoff = 4;
    let expected_next_data_blocked_time =
        write_context.current_time + DEFAULT_SYNC_PERIOD * expected_transmission_backoff;
    assert_eq!(
        Some(expected_next_data_blocked_time),
        manager.next_expiration()
    );

    manager.on_timeout(expected_next_data_blocked_time);

    // Another DATA_BLOCKED frame should be sent
    assert_eq!(
        transmission::Interest::NewData,
        manager.get_transmission_interest()
    );

    // We get more credit from the peer so we should no longer send DATA_BLOCKED
    assert!(manager
        .on_max_data(MaxData {
            maximum_data: current_window + 1
        })
        .is_ok());

    assert_eq!(
        transmission::Interest::None,
        manager.get_transmission_interest()
    );

    // Exceed the window again
    assert_eq!(
        VarInt::from_u32(1),
        manager.with_outgoing_connection_flow_controller(
            |ctrl| ctrl.acquire_window(VarInt::from_u32(2))
        )
    );

    // Another DATA_BLOCKED frame should be sent with the updated MAX_DATA value
    assert_eq!(
        transmission::Interest::NewData,
        manager.get_transmission_interest()
    );

    assert!(manager.on_transmit(&mut write_context).is_ok());

    let expected_frame = Frame::DataBlocked(DataBlocked {
        data_limit: current_window + 1,
    });

    assert_eq!(
        expected_frame,
        write_context.frame_buffer.pop_front().unwrap().as_frame()
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
//= type=test
//# An endpoint that receives a frame with a stream ID exceeding the limit it has
//# sent MUST treat this as a connection error of type
//# STREAM_LIMIT_ERROR; see Section 11 for details on error handling.

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
//= type=test
//# An endpoint MUST terminate a connection
//# with an error of type STREAM_LIMIT_ERROR if a peer opens more streams
//# than was permitted.
#[test]
fn stream_limit_error_on_peer_open_stream_too_large() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
        let current_max_streams = manager.with_stream_controller(|ctrl| {
            ctrl.remote_initiated_max_streams_latest_value(stream_type)
        });
        // stream_id is 0-indexed
        let current_max_streams = current_max_streams.as_u64() - 1;

        let max_stream_id =
            StreamId::nth(endpoint::Type::Client, stream_type, current_max_streams).unwrap();

        assert!(manager
            .with_stream_controller(
                |ctrl| ctrl.on_open_remote_stream(StreamIter::new(max_stream_id, max_stream_id))
            )
            .is_ok());

        assert_eq!(
            Err(transport::Error::STREAM_LIMIT_ERROR),
            manager.with_stream_controller(|ctrl| {
                let open_id = max_stream_id.next_of_type().unwrap();
                ctrl.on_open_remote_stream(StreamIter::new(open_id, open_id))
            })
        );
    }
}

#[test]
fn blocked_on_local_concurrent_stream_limit() {
    for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
        let mut manager = create_stream_manager(endpoint::Type::Server);

        // The peer allows a large amount of streams to be opened
        assert!(manager
            .on_max_streams(&MaxStreams {
                stream_type,
                maximum_streams: VarInt::from_u32(100_000),
            })
            .is_ok());

        let available_outgoing_stream_capacity = manager.with_stream_controller(|ctrl| {
            ctrl.available_local_initiated_stream_capacity(stream_type)
        });

        assert!(available_outgoing_stream_capacity < VarInt::from_u32(100_000));

        let (waker, wake_counter) = new_count_waker();
        let (mut wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
        let mut token = connection::OpenToken::new();

        for _i in 0..*available_outgoing_stream_capacity {
            assert!(manager
                .poll_open_local_stream(
                    stream_type,
                    &mut token,
                    &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                    &Context::from_waker(&waker)
                )
                .is_ready());
        }

        assert_eq!(wake_counter, 0);

        // Cannot open any more streams
        assert!(manager
            .poll_open_local_stream(
                stream_type,
                &mut token,
                &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                &Context::from_waker(&waker)
            )
            .is_pending());

        if stream_type.is_bidirectional() {
            // if we have a bidirectional stream, then the controller should transmit an empty STREAM frame in order
            // to notify the peer of its existence.

            assert_wakeups(&mut wakeup_queue, 1);

            let mut frame_buffer = OutgoingFrameBuffer::new();
            let mut write_context = MockWriteContext::new(
                time::now(),
                &mut frame_buffer,
                transmission::Constraint::None,
                transmission::Mode::Normal,
                endpoint::Type::Server,
            );

            manager.on_transmit(&mut write_context).unwrap();

            let mut stream_frame = frame_buffer
                .pop_front()
                .expect("missing empty STREAM frame");

            match stream_frame.as_frame() {
                Frame::Stream(frame) => {
                    assert_eq!(
                        frame.stream_id,
                        StreamId::nth(
                            endpoint::Type::Server,
                            stream_type,
                            available_outgoing_stream_capacity.as_u64() - 1
                        )
                        .unwrap()
                        .as_varint()
                    );
                    assert_eq!(frame.offset, VarInt::default());
                    assert!(frame.data.is_empty());
                }
                frame => panic!("unexpected frame: {frame:?}"),
            }

            assert!(frame_buffer.is_empty());
        }

        // No STREAMS_BLOCKED frame should be transmitted since we are blocked on the local
        // limit not the peer's limit.
        assert!(manager.get_transmission_interest().is_none());

        // Close one stream
        manager.with_asserted_stream(
            StreamId::initial(endpoint::Type::Server, stream_type),
            |stream| {
                stream.interests.retained = false;
            },
        );

        // One more stream can be opened
        assert!(manager
            .poll_open_local_stream(
                stream_type,
                &mut token,
                &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                &Context::from_waker(&waker)
            )
            .is_ready());
        assert_eq!(wake_counter, 1);
        assert!(manager
            .poll_open_local_stream(
                stream_type,
                &mut token,
                &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                &Context::from_waker(&waker)
            )
            .is_pending());

        // Close the stream manager and verify the wake counter is incremented
        manager.close(connection::Error::application(1u8.into()));

        assert_eq!(wake_counter, 2);
    }
}

#[test]
fn asymmetric_stream_limits_remote_initiated() {
    let mut initial_local_limits = create_default_initial_flow_control_limits();
    let mut initial_peer_limits = create_default_initial_flow_control_limits();

    for local_limit in 0..101 {
        for peer_limit in 0..101 {
            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
                let limits = ConnectionLimits::default()
                    .with_max_open_local_bidirectional_streams(local_limit as u64)
                    .unwrap()
                    .with_max_open_local_unidirectional_streams(local_limit as u64)
                    .unwrap();

                initial_local_limits.max_open_remote_bidirectional_streams =
                    VarInt::from_u16(local_limit);
                initial_peer_limits.max_open_remote_bidirectional_streams =
                    VarInt::from_u16(peer_limit);
                initial_local_limits.max_open_remote_unidirectional_streams =
                    VarInt::from_u16(local_limit);
                initial_peer_limits.max_open_remote_unidirectional_streams =
                    VarInt::from_u16(peer_limit);

                let mut manager = AbstractStreamManager::<MockStream>::new(
                    &limits,
                    endpoint::Type::Server,
                    initial_local_limits,
                    initial_peer_limits,
                    DEFAULT_INITIAL_RTT,
                );

                // The peer opens streams up to the limit we have given them
                for i in 0..local_limit {
                    let stream_id =
                        StreamId::nth(endpoint::Type::Client, stream_type, i as u64).unwrap();
                    assert_eq!(
                        Ok(()),
                        manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], true))
                    );
                }

                // local capacity assertion
                let available_local_stream_capacity = manager.with_stream_controller(|cntl| {
                    cntl.available_local_initiated_stream_capacity(stream_type)
                });
                // Remote initiated streams do NOT use up the capacity for local initiated streams
                //
                // Local endpoint did not open any streams so it should have the capacity based
                // on peer and local limits.
                let available_limit = local_limit.min(peer_limit);
                assert_eq!(
                    VarInt::from_u16(available_limit),
                    available_local_stream_capacity,
                    "local_limit:{local_limit} peer_limit:{peer_limit} stream_type:{stream_type:?}"
                );

                // remote capacity assertion
                let available_remote_stream_capacity = manager.with_stream_controller(|cntl| {
                    cntl.available_remote_initiated_stream_capacity(stream_type)
                });
                // Remote initiated streams capacity should be all used up
                assert_eq!(
                    VarInt::from_u16(0),
                    available_remote_stream_capacity,
                    "local_limit:{local_limit} peer_limit:{peer_limit} stream_type:{stream_type:?}"
                );
            }
        }
    }
}

#[test]
fn asymmetric_stream_limits_local_initiated() {
    let mut initial_local_limits = create_default_initial_flow_control_limits();
    let mut initial_peer_limits = create_default_initial_flow_control_limits();
    let (accept_waker, _accept_wake_counter) = new_count_waker();
    let (_wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
    let mut token = connection::OpenToken::new();

    for local_limit in 0..101 {
        for peer_limit in 0..101 {
            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
                let limits = ConnectionLimits::default()
                    .with_max_open_local_bidirectional_streams(local_limit as u64)
                    .unwrap()
                    .with_max_open_local_unidirectional_streams(local_limit as u64)
                    .unwrap();

                initial_local_limits.max_open_remote_bidirectional_streams =
                    VarInt::from_u16(local_limit);
                initial_peer_limits.max_open_remote_bidirectional_streams =
                    VarInt::from_u16(peer_limit);
                initial_local_limits.max_open_remote_unidirectional_streams =
                    VarInt::from_u16(local_limit);
                initial_peer_limits.max_open_remote_unidirectional_streams =
                    VarInt::from_u16(peer_limit);

                let mut manager = AbstractStreamManager::<MockStream>::new(
                    &limits,
                    endpoint::Type::Server,
                    initial_local_limits,
                    initial_peer_limits,
                    DEFAULT_INITIAL_RTT,
                );

                // Local endpoint opens streams up to the limit
                let local_available_limit = local_limit.min(peer_limit);
                for _ in 0..local_available_limit {
                    let result = manager.poll_open_local_stream(
                        stream_type,
                        &mut token,
                        &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                        &Context::from_waker(&accept_waker),
                    );
                    assert!(
                        result.is_ready(),
                        "local_limit:{local_limit} peer_limit:{peer_limit} stream_type:{stream_type:?}"
                    );
                }

                // local capacity assertion
                let available_local_stream_capacity = manager.with_stream_controller(|cntl| {
                    cntl.available_local_initiated_stream_capacity(stream_type)
                });
                // Local initiated streams capacity should be all used up
                assert_eq!(
                    VarInt::from_u16(0),
                    available_local_stream_capacity,
                    "local_limit:{local_limit} peer_limit:{peer_limit} stream_type:{stream_type:?}"
                );

                // remote capacity assertion
                let available_remote_stream_capacity = manager.with_stream_controller(|cntl| {
                    cntl.available_remote_initiated_stream_capacity(stream_type)
                });
                // Local initiated streams do NOT use up the capacity for remote initiated streams
                //
                // Remote endpoint did not open any streams so it should have the capacity that
                // the local endpoint specified and sent in transport parameters
                assert_eq!(
                    VarInt::from_u16(local_limit),
                    available_remote_stream_capacity,
                    "local_limit:{local_limit} peer_limit:{peer_limit} stream_type:{stream_type:?}"
                );
            }
        }
    }
}

#[test]
fn rate_limited_stream_credits() {
    let mut initial_local_limits = create_default_initial_flow_control_limits();
    let mut initial_peer_limits = create_default_initial_flow_control_limits();
    let limit = 2u32;

    for stream_type in [StreamType::Unidirectional, StreamType::Bidirectional] {
        for timeout in [
            Duration::ZERO,
            Duration::from_millis(2),
            DEFAULT_INITIAL_RTT,
        ] {
            dbg!(stream_type, timeout);

            let mut clock = time::Clock::default();
            let limits = ConnectionLimits::default()
                .with_max_open_local_bidirectional_streams(limit as u64)
                .unwrap()
                .with_max_open_local_unidirectional_streams(limit as u64)
                .unwrap();

            initial_local_limits.max_open_remote_bidirectional_streams = VarInt::from_u32(limit);
            initial_peer_limits.max_open_remote_bidirectional_streams = VarInt::from_u32(limit);
            initial_local_limits.max_open_remote_unidirectional_streams = VarInt::from_u32(limit);
            initial_peer_limits.max_open_remote_unidirectional_streams = VarInt::from_u32(limit);

            let mut manager = AbstractStreamManager::<MockStream>::new(
                &limits,
                endpoint::Type::Server,
                initial_local_limits,
                initial_peer_limits,
                DEFAULT_INITIAL_RTT,
            );

            let mut stream_ids =
                (0..).map(|id| StreamId::nth(endpoint::Type::Client, stream_type, id).unwrap());

            // The peer opens streams up to the limit we have given them, once for the initial
            // limits, next for the first MAX_STREAMS frame
            for stream_id in (&mut stream_ids).take(limit as usize * 2) {
                assert_eq!(
                    Ok(()),
                    manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], true))
                );

                // have the application clean up the stream state
                manager.with_asserted_stream(stream_id, |stream| {
                    stream.interests.retained = false;
                });

                manager.on_timeout(clock.get_time());
            }

            // apply the configured timeout
            clock.inc_by(timeout);
            manager.on_timeout(clock.get_time());

            // if the peer didn't wait enough time, then an error should occur
            let expected = if timeout >= DEFAULT_INITIAL_RTT {
                Ok(())
            } else {
                Err(transport::Error::STREAM_LIMIT_ERROR)
            };

            // The peer tries to open one more beyond the limit
            let stream_id = stream_ids.next().unwrap();
            assert_eq!(
                expected,
                manager.on_data(&stream_data(stream_id, VarInt::from_u32(0), &[], true))
            );
        }
    }
}

#[test]
fn accept_returns_remotely_initiated_stream() {
    const STREAMS_TO_OPEN: u64 = 8;

    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client] {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client] {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
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

                assert_matches!(
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker)),
                    Poll::Ready(Err(connection::Error::StreamIdExhausted { .. })),
                );
            }
        }
    }
}

#[test]
fn accept_returns_opened_streams_of_any_type() {
    const STREAMS_TO_OPEN: u64 = 8;

    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client] {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client] {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }

            let mut manager = create_stream_manager(local_ep_type);
            let (accept_waker, _accept_wake_counter) = new_count_waker();

            let mut streams = std::collections::HashSet::new();

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
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
                        "accepted {stream_id:?} stream multiple times"
                    ),
                    other => panic!("unexpected result {other:?}"),
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
    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client] {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client] {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
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
    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client] {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client] {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }
            let mut manager = create_stream_manager(local_ep_type);
            let (accept_waker, accept_wake_counter) = new_count_waker();

            assert!(manager
                .poll_accept(None, &Context::from_waker(&accept_waker))
                .is_pending());

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
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
                manager.close(connection::Error::unspecified());

                for n in 0..STREAMS_TO_OPEN {
                    let stream_id = StreamId::nth(*initiator_type, *stream_type, n).unwrap();
                    assert_eq!(
                        Poll::Ready(Ok(Some(stream_id))),
                        manager
                            .poll_accept(Some(*stream_type), &Context::from_waker(&accept_waker))
                    );
                }

                // Now the error should be visible
                assert_matches!(
                    manager.poll_accept(Some(*stream_type), &Context::from_waker(&accept_waker)),
                    Poll::Ready(Err(connection::Error::Unspecified { .. })),
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
                manager.close(connection::Error::unspecified());
                assert_eq!(accept_wake_counter, 1);

                // Now the error should be visible
                assert!(matches!(
                    manager.poll_accept(Some(*stream_type), &Context::from_waker(&accept_waker)),
                    Poll::Ready(Err(connection::Error::Unspecified { .. })),
                ));
            }
        }
    }
}

#[test]
fn closing_stream_manager_without_error_returns_none() {
    for initiator_type in [endpoint::Type::Server, endpoint::Type::Client] {
        for local_ep_type in [endpoint::Type::Server, endpoint::Type::Client] {
            let is_remote_initialized = local_ep_type != initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
                let mut manager = create_stream_manager(local_ep_type);
                let (accept_waker, accept_wake_counter) = new_count_waker();

                assert_eq!(
                    Poll::Pending,
                    manager.poll_accept(Some(stream_type), &Context::from_waker(&accept_waker))
                );
                assert_eq!(accept_wake_counter, 0);

                // Close the StreamManager
                // This should wake up the accept call
                manager.close(connection::Error::closed(endpoint::Location::Local));
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
    assert!(manager
        .streams_waiting_for_connection_flow_control_credits()
        .is_empty());
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
    assert!(manager
        .streams_waiting_for_connection_flow_control_credits()
        .is_empty());

    // Let stream_3 grab more of the connection window than it requires
    manager.with_asserted_stream(stream_3, |stream| {
        stream.on_connection_window_available_retrieve_window = 120
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_connection_window_available_retrieve_window = 220
    });

    //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
    //= type=test
    //# A sender MUST ignore any MAX_STREAM_DATA or MAX_DATA frames that do
    //# not increase flow control limits.
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
    assert!(manager
        .streams_waiting_for_connection_flow_control_credits()
        .is_empty());
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
    assert!(manager
        .streams_waiting_for_delivery_notifications()
        .is_empty());
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

    manager.close(connection::Error::application(1u8.into()));
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
    assert!(manager
        .streams_waiting_for_delivery_notifications()
        .is_empty());
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
    assert!(manager.streams_waiting_for_transmission().is_empty());
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
    assert!(manager.streams_waiting_for_retransmission().is_empty());
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
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
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
    assert!(manager.streams_waiting_for_retransmission().is_empty());

    write_context.transmission_constraint = transmission::Constraint::None;

    assert!(manager.on_transmit(&mut write_context).is_ok());

    for stream_id in &[stream_1, stream_3, stream_4] {
        assert_stream_write_state(&mut manager, *stream_id, 1, 0);
    }

    // All 4 streams have written a frame plus there should also be an empty STREAM frame for the open notify
    assert_eq!(5, frame_buffer.len());
    frame_buffer.clear();
    assert!(manager.streams_waiting_for_transmission().is_empty());

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
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );

    assert_eq!(
        [stream_1, stream_2, stream_3, stream_4],
        *manager.streams_waiting_for_transmission()
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
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
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
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
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
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
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
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );

    assert_eq!(Ok(()), manager.on_transmit(&mut write_context));
    assert_stream_write_state(&mut manager, stream_1, 4, 0);
    assert_stream_write_state(&mut manager, stream_2, 2, 0);
    assert_stream_write_state(&mut manager, stream_3, 2, 0);
    assert_stream_write_state(&mut manager, stream_4, 4, 0);

    assert_eq!(10, frame_buffer.len());
    frame_buffer.clear();
    assert!(manager.streams_waiting_for_transmission().is_empty());
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
fn forwards_on_rtt_update() {
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let stream_2 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    manager.with_asserted_stream(stream_1, |stream| {
        stream.interests.stream_flow_control_credits = true;
    });

    let rtt_estimator = RttEstimator::new(Duration::from_millis(100));
    manager.on_rtt_update(&rtt_estimator, time::now());

    // Check call count
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.update_blocked_sync_period_count, 1);
    });
    manager.with_asserted_stream(stream_2, |stream| {
        assert_eq!(stream.update_blocked_sync_period_count, 0);
    });
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
    let (mut wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
    let (waker, _wake_counter) = new_count_waker();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    let ctx = Context::from_waker(&waker);
    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().receive(&mut [Bytes::new()]),
            Some(&ctx),
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.poll_pop_count, 1);
        stream.next_api_error = Some(StreamError::max_stream_data_size_exceeded());
        stream.api_call_requires_transmission = true;
    });

    assert_wakeups(&mut wakeup_queue, 0);
    assert_matches!(
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().receive(&mut [Bytes::new()]),
            Some(&ctx)
        ),
        Err(StreamError::MaxStreamDataSizeExceeded { .. }),
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.get_transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_matches!(
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().receive(&mut [Bytes::new()]),
            Some(&ctx)
        ),
        Err(StreamError::InvalidStream { .. }),
    );
}

#[test]
fn forwards_stop_sending() {
    let (mut wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let error = ApplicationErrorCode::new(0x12_3456).unwrap();

    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().stop_sending(error),
            None,
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.stop_sending_count, 1);
        stream.next_api_error = Some(StreamError::max_stream_data_size_exceeded());
        stream.api_call_requires_transmission = true;
    });

    assert_wakeups(&mut wakeup_queue, 0);
    assert_matches!(
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().stop_sending(error),
            None,
        ),
        Err(StreamError::MaxStreamDataSizeExceeded { .. }),
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.get_transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_matches!(
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().stop_sending(error),
            None,
        ),
        Err(StreamError::InvalidStream { .. }),
    );
}

#[test]
fn forwards_poll_push() {
    let (mut wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
    let (waker, _wake_counter) = new_count_waker();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let data = Bytes::from_static(b"1234");

    let ctx = Context::from_waker(&waker);
    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().send(&mut [data.clone()]),
            Some(&ctx)
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.poll_push_count, 1);
        stream.next_api_error = Some(StreamError::max_stream_data_size_exceeded());
        stream.api_call_requires_transmission = true;
    });

    assert_wakeups(&mut wakeup_queue, 0);
    assert_matches!(
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().send(&mut [data.clone()]),
            Some(&ctx)
        ),
        Err(StreamError::MaxStreamDataSizeExceeded { .. }),
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.get_transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_matches!(
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().send(&mut [data]),
            Some(&ctx)
        ),
        Err(StreamError::InvalidStream { .. }),
    );
}

#[test]
fn forwards_poll_finish() {
    let (mut wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
    let (waker, _wake_counter) = new_count_waker();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();

    let ctx = Context::from_waker(&waker);
    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().finish().flush(),
            Some(&ctx)
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.poll_finish_count, 1);
        stream.next_api_error = Some(StreamError::max_stream_data_size_exceeded());
        stream.api_call_requires_transmission = true;
    });

    assert_wakeups(&mut wakeup_queue, 0);
    assert_matches!(
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().finish().flush(),
            Some(&ctx)
        ),
        Err(StreamError::MaxStreamDataSizeExceeded { .. }),
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.get_transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_matches!(
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().finish().flush(),
            Some(&ctx)
        ),
        Err(StreamError::InvalidStream { .. }),
    );
}

#[test]
fn forwards_reset() {
    let (mut wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
    let mut manager = create_stream_manager(endpoint::Type::Server);

    let stream_1 = try_open(&mut manager, StreamType::Bidirectional).unwrap();
    let error = ApplicationErrorCode::new(0x12_3456).unwrap();

    assert!(manager
        .poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().reset(error),
            None,
        )
        .is_ok());

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.reset_count, 1);
        stream.next_api_error = Some(StreamError::max_stream_data_size_exceeded());
        stream.api_call_requires_transmission = true;
    });

    assert_wakeups(&mut wakeup_queue, 0);
    assert_matches!(
        manager.poll_request(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().reset(error),
            None,
        ),
        Err(StreamError::MaxStreamDataSizeExceeded { .. }),
    );
    assert_eq!(
        transmission::Interest::NewData,
        manager.get_transmission_interest()
    );
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_matches!(
        manager.poll_request(
            invalid_stream_id(endpoint::Type::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
            ops::Request::default().reset(error),
            None,
        ),
        Err(StreamError::InvalidStream { .. }),
    );
}

#[test]
fn stream_transmission_fairness_test() {
    for concurrent_streams in 2..=5 {
        dbg!(concurrent_streams);

        let mut manager = create_stream_manager(endpoint::Type::Server);

        // Create some open Streams with interests
        let mut streams: VecDeque<_> = (0..concurrent_streams)
            .map(|_| try_open(&mut manager, StreamType::Bidirectional).unwrap())
            .collect();

        // limit the number of frame transmissions per round
        for stream_id in &streams {
            manager.with_asserted_stream(*stream_id, |stream| {
                stream.on_transmit_try_write_frames = 100;
                stream.on_transmit_limit = Some(1);
            });
        }

        // make sure the order matches creation order
        assert_eq!(streams, manager.streams_waiting_for_transmission());

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );

        // Iterate over the list twice and transmit a single frame each time
        //
        // The stream at the front should be moved to the back
        for transmission in 0..(concurrent_streams * 2) {
            dbg!(transmission);
            let _ = manager.on_transmit(&mut write_context);
            write_context.frame_buffer.flush();
            write_context.frame_buffer.set_error_write_after_n_frames(1);

            assert_eq!(streams, manager.streams_waiting_for_transmission());

            streams.rotate_left(1);
        }
    }
}

#[test]
fn stream_batching_test() {
    for batch_size in 1..=10 {
        dbg!(batch_size);
        let limits = ConnectionLimits::default()
            .with_stream_batch_size(batch_size)
            .unwrap();

        let mut manager = AbstractStreamManager::<stream::StreamImpl>::new(
            &limits,
            endpoint::Type::Server,
            create_default_initial_flow_control_limits(),
            create_default_initial_flow_control_limits(),
            DEFAULT_INITIAL_RTT,
        );

        // Create some open Streams
        let mut stream_ids: VecDeque<StreamId> = (0..4)
            .map(|_| {
                let (accept_waker, _accept_wake_counter) = new_count_waker();
                let (_wakeup_queue, wakeup_handle) = create_wakeup_queue_and_handle();
                let mut token = connection::OpenToken::new();

                let result = match manager.poll_open_local_stream(
                    StreamType::Bidirectional,
                    &mut token,
                    &mut ConnectionApiCallContext::from_wakeup_handle(&wakeup_handle),
                    &Context::from_waker(&accept_waker),
                ) {
                    Poll::Ready(res) => res,
                    Poll::Pending => Err(connection::Error::unspecified()),
                };
                result.unwrap()
            })
            .collect();

        // Create a context that can only fit packets of size 50
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let max_packet_size = 50;
        frame_buffer.set_max_packet_size(Some(max_packet_size));
        let mut write_context = MockWriteContext::new(
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );

        const DATA_SIZE: usize = 2000;
        let array: [u8; DATA_SIZE] = [1; DATA_SIZE];

        // Set up each stream to have much more data to send than can fit in our test packet
        for stream_id in &stream_ids {
            manager
                .with_asserted_stream(*stream_id, |stream: &mut stream::StreamImpl| {
                    let data_to_send = bytes::Bytes::copy_from_slice(&array);
                    stream.poll_request(ops::Request::default().send(&mut [data_to_send]), None)
                })
                .unwrap();
        }
        // make sure the order matches creation order
        assert_eq!(stream_ids, manager.streams_waiting_for_transmission());

        // Send 40 packets. Each stream gets to be the first to fill up a packet "batch_size" times.
        // Then the stream gets sent to the back of the transmission list.
        for idx in 1..=40 {
            dbg!(idx);
            let _ = manager.on_transmit(&mut write_context);

            assert_eq!(stream_ids, manager.streams_waiting_for_transmission());

            write_context.frame_buffer.flush();

            if idx % batch_size == 0 {
                // The first stream gets sent to the back of the transmission list once we have sent "batch_size" packets
                stream_ids.rotate_left(1);
            }
        }
    }
}
