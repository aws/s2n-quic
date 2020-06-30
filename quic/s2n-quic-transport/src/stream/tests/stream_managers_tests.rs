//! Tests for `StreamManager`

use super::*;
use crate::{
    connection::{ConnectionLimits, InternalConnectionId, InternalConnectionIdGenerator},
    contexts::{ConnectionApiCallContext, ConnectionContext, OnTransmitError, WriteContext},
    stream::{
        stream_impl::StreamConfig,
        stream_interests::{StreamInterestProvider, StreamInterests},
        AbstractStreamManager, StreamError, StreamEvents, StreamLimits, StreamManagerInterests,
        StreamTrait,
    },
    wakeup_queue::{WakeupHandle, WakeupQueue},
};
use alloc::collections::VecDeque;
use bytes::Bytes;
use core::task::{Context, Poll, Waker};
use futures_test::task::new_count_waker;
use s2n_quic_core::{
    ack_set::AckSet,
    application::ApplicationErrorCode,
    endpoint::EndpointType,
    frame::{
        stream::StreamRef, MaxData, MaxStreamData, ResetStream, StopSending, Stream as StreamFrame,
        StreamDataBlocked,
    },
    stream::{StreamId, StreamType},
    transport::parameters::{InitialFlowControlLimits, InitialStreamLimits},
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
        interests.frame_exchange.transmission = self.on_transmit_try_write_frames > 0;
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

    fn poll_pop<C: ConnectionContext>(
        &mut self,
        _connection_context: &C,
        _context: &Context,
    ) -> Poll<Result<Option<Bytes>, StreamError>> {
        self.poll_pop_count += 1;
        if self.api_call_requires_transmission {
            self.on_transmit_try_write_frames = 1;
        }
        if let Some(err) = self.next_api_error {
            return Poll::Ready(Err(err));
        };
        Poll::Ready(Ok(None))
    }

    fn stop_sending<C: ConnectionContext>(
        &mut self,
        _error_code: ApplicationErrorCode,
        _connection_context: &C,
    ) -> Result<(), StreamError> {
        self.stop_sending_count += 1;
        if self.api_call_requires_transmission {
            self.on_transmit_try_write_frames = 1;
        }
        if let Some(err) = self.next_api_error {
            return Err(err);
        };
        Ok(())
    }

    fn poll_push<C: ConnectionContext>(
        &mut self,
        _connection_context: &C,
        _data: Bytes,
        _context: &Context,
    ) -> Poll<Result<(), StreamError>> {
        self.poll_push_count += 1;
        if self.api_call_requires_transmission {
            self.on_transmit_try_write_frames = 1;
        }
        if let Some(err) = self.next_api_error {
            return Poll::Ready(Err(err));
        };
        Poll::Ready(Ok(()))
    }

    fn poll_finish<C: ConnectionContext>(
        &mut self,
        _connection_context: &C,
        _context: &Context,
    ) -> Poll<Result<(), StreamError>> {
        self.poll_finish_count += 1;
        if self.api_call_requires_transmission {
            self.on_transmit_try_write_frames = 1;
        }
        if let Some(err) = self.next_api_error {
            return Poll::Ready(Err(err));
        };
        Poll::Ready(Ok(()))
    }

    fn reset<C: ConnectionContext>(
        &mut self,
        _connection_context: &C,
        _error_code: ApplicationErrorCode,
    ) -> Result<(), StreamError> {
        self.reset_count += 1;
        if self.api_call_requires_transmission {
            self.on_transmit_try_write_frames = 1;
        }
        if let Some(err) = self.next_api_error {
            return Err(err);
        };
        Ok(())
    }
}

const EMPTY_STREAM_MANAGER_INTERESTS: StreamManagerInterests = StreamManagerInterests {
    transmission: false,
    finalization: false,
};

const TX_STREAM_MANAGER_INTEREST: StreamManagerInterests = StreamManagerInterests {
    transmission: true,
    finalization: false,
};

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

fn create_stream_manager(local_ep_type: EndpointType) -> AbstractStreamManager<MockStream> {
    let initial_local_limits = create_default_initial_flow_control_limits();
    let initial_peer_limits = create_default_initial_flow_control_limits();

    let mut limits = ConnectionLimits::default();
    limits.stream_limits = StreamLimits {
        max_send_buffer_size: 4096,
    };

    AbstractStreamManager::<MockStream>::new(
        &limits,
        local_ep_type,
        initial_local_limits,
        initial_peer_limits,
    )
}

#[test]
fn remote_messages_open_unopened_streams() {
    const STREAMS_TO_OPEN: usize = 8;

    for initiator_type in &[EndpointType::Server, EndpointType::Client] {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            for local_ep_type in &[EndpointType::Server, EndpointType::Client] {
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
    const STREAMS_TO_OPEN: usize = 8;

    for initiator_type in &[EndpointType::Server, EndpointType::Client] {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            for local_ep_type in &[EndpointType::Server, EndpointType::Client] {
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
                manager.close(StreamError::ConnectionClosed(ApplicationErrorCode::UNKNOWN));

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
    for local_ep_type in &[EndpointType::Client, EndpointType::Server] {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            let mut manager = create_stream_manager(*local_ep_type);

            for n in 0..8 {
                let stream_id: StreamId = manager.open(*stream_type).unwrap();
                assert!(manager.active_streams().contains(&stream_id));
                assert_eq!(
                    StreamId::nth(*local_ep_type, *stream_type, n).unwrap(),
                    stream_id
                );
            }

            // Check Stream ID exhaustion
            *manager
                .inner
                .next_stream_ids
                .get_mut(*local_ep_type, *stream_type) = None;

            assert_eq!(
                (Err(StreamError::StreamIdExhausted)),
                manager.open(*stream_type)
            );
        }
    }
}

#[test]
fn open_returns_error_after_close() {
    let mut manager = create_stream_manager(EndpointType::Server);

    manager.close(StreamError::ConnectionError);
    assert_eq!(
        Err(StreamError::ConnectionError),
        manager.open(StreamType::Bidirectional)
    )
}

#[test]
fn returns_finalization_interest_after_last_stream_is_drained() {
    let mut manager = create_stream_manager(EndpointType::Server);
    let (_wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();

    assert_eq!(0, manager.active_streams().len());
    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    assert_eq!(1, manager.active_streams().len());
    let stream_2 = manager.open(StreamType::Bidirectional).unwrap();
    assert_eq!(2, manager.active_streams().len());
    assert_eq!(false, manager.interests().finalization);

    manager.close(StreamError::ConnectionClosed(ApplicationErrorCode::UNKNOWN));
    assert_eq!(false, manager.interests().finalization);

    let error = ApplicationErrorCode::new(0).unwrap();

    // The first stream is immediately interested in finalization and should
    // therefore be collected
    manager.with_asserted_stream(stream_1, |stream| {
        stream.interests.finalization = true;
    });
    assert_eq!(1, manager.active_streams().len());
    assert_eq!(false, manager.interests().finalization);

    // The second stream is not yet interested in finalization
    assert!(manager
        .reset(
            stream_2,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            error
        )
        .is_ok());
    assert_eq!(1, manager.active_streams().len());
    assert_eq!(false, manager.interests().finalization);

    // Let the last stream return the finalization interest
    manager.with_asserted_stream(stream_2, |stream| {
        stream.interests.finalization = true;
    });
    assert_eq!(0, manager.active_streams().len());
    assert_eq!(true, manager.interests().finalization);
}

#[test]
fn remote_messages_which_target_locally_initiated_unopened_streams_error() {
    for initiator_type in &[EndpointType::Server, EndpointType::Client] {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            for local_ep_type in &[EndpointType::Server, EndpointType::Client] {
                let is_remote_initialized = *local_ep_type != *initiator_type;
                if is_remote_initialized {
                    continue;
                }

                for (already_open, target_stream_id) in &[(0, 0), (1, 1), (3, 5)] {
                    let mut manager = create_stream_manager(*local_ep_type);

                    for _ in 0..*already_open {
                        let stream_id = manager.open(*stream_type).unwrap();
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
                        Err(TransportError::new(
                            TransportError::STREAM_STATE_ERROR,
                            "Stream was not yet opened",
                            None
                        )),
                        manager.on_reset_stream(&reset_frame)
                    );
                }
            }
        }
    }
}

#[test]
fn max_data_replenishes_connection_flow_control_window() {
    let mut manager = create_stream_manager(EndpointType::Server);
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

#[test]
fn accept_returns_remotely_initiated_stream() {
    const STREAMS_TO_OPEN: usize = 8;

    for initiator_type in &[EndpointType::Server, EndpointType::Client] {
        for local_ep_type in &[EndpointType::Server, EndpointType::Client] {
            let is_remote_initialized = *local_ep_type != *initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
                let mut manager = create_stream_manager(*local_ep_type);

                for n in 0..STREAMS_TO_OPEN {
                    let stream_id = StreamId::nth(*initiator_type, *stream_type, n).unwrap();
                    let (accept_waker, accept_wake_counter) = new_count_waker();

                    // Stream is not yet available
                    assert_eq!(
                        Poll::Pending,
                        manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
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
                        Poll::Ready(Ok(stream_id)),
                        manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
                    );
                }

                // Try to accept multiple streams at once
                let stream_id_1 =
                    StreamId::nth(*initiator_type, *stream_type, STREAMS_TO_OPEN).unwrap();
                let stream_id_2 =
                    StreamId::nth(*initiator_type, *stream_type, STREAMS_TO_OPEN + 1).unwrap();
                let (accept_waker, accept_wake_counter) = new_count_waker();

                // Stream is not yet available
                assert_eq!(
                    Poll::Pending,
                    manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
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
                    Poll::Ready(Ok(stream_id_1)),
                    manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
                );
                assert_eq!(
                    Poll::Ready(Ok(stream_id_2)),
                    manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
                );
                assert_eq!(
                    Poll::Pending,
                    manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
                );

                // Check Stream ID exhaustion
                *manager.inner.accept_state.next_stream_mut(*stream_type) = None;

                assert_eq!(
                    Poll::Ready(Err(StreamError::StreamIdExhausted)),
                    manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
                );
            }
        }
    }
}

#[test]
fn accept_returns_opened_streams_even_if_stream_manager_was_closed() {
    const STREAMS_TO_OPEN: usize = 8;

    for initiator_type in &[EndpointType::Server, EndpointType::Client] {
        for local_ep_type in &[EndpointType::Server, EndpointType::Client] {
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
                manager.close(StreamError::ConnectionError);

                for n in 0..STREAMS_TO_OPEN {
                    let stream_id = StreamId::nth(*initiator_type, *stream_type, n).unwrap();
                    assert_eq!(
                        Poll::Ready(Ok(stream_id)),
                        manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
                    );
                }

                // Now the error should be visible
                assert_eq!(
                    Poll::Ready(Err(StreamError::ConnectionError)),
                    manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
                );
            }
        }
    }
}

#[test]
fn closing_stream_manager_wakes_blocked_accepts() {
    for initiator_type in &[EndpointType::Server, EndpointType::Client] {
        for local_ep_type in &[EndpointType::Server, EndpointType::Client] {
            let is_remote_initialized = *local_ep_type != *initiator_type;
            if !is_remote_initialized {
                continue;
            }

            for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
                let mut manager = create_stream_manager(*local_ep_type);
                let (accept_waker, accept_wake_counter) = new_count_waker();

                assert_eq!(
                    Poll::Pending,
                    manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
                );
                assert_eq!(accept_wake_counter, 0);

                // Close the StreamManager
                // This should wake up the accept call
                manager.close(StreamError::ConnectionError);
                assert_eq!(accept_wake_counter, 1);

                // Now the error should be visible
                assert_eq!(
                    Poll::Ready(Err(StreamError::ConnectionError)),
                    manager.poll_accept(*stream_type, &Context::from_waker(&accept_waker))
                );
            }
        }
    }
}

#[test]
fn add_and_remove_streams_from_on_connection_window_lists() {
    let mut manager = create_stream_manager(EndpointType::Server);
    // Consume all window
    let current_window =
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window());
    assert_eq!(
        current_window,
        manager
            .with_outgoing_connection_flow_controller(|ctrl| ctrl.acquire_window(current_window))
    );

    // Create some open Streams with interests
    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Unidirectional).unwrap();
    let _stream_3 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_4 = manager.open(StreamType::Unidirectional).unwrap();

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

    let mut manager = create_stream_manager(EndpointType::Server);
    // Consume all window
    let mut current_window =
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window());
    assert_eq!(
        current_window,
        manager
            .with_outgoing_connection_flow_controller(|ctrl| ctrl.acquire_window(current_window))
    );

    // Create some open Streams with interests
    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Unidirectional).unwrap();
    let stream_3 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_4 = manager.open(StreamType::Unidirectional).unwrap();

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
    let mut manager = create_stream_manager(EndpointType::Server);

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Unidirectional).unwrap();
    let _stream_3 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_4 = manager.open(StreamType::Unidirectional).unwrap();

    for stream_id in &[stream_2, stream_1, stream_4] {
        manager.with_asserted_stream(*stream_id, |stream| {
            stream.interests.frame_exchange.delivery_notifications = true;
        });
    }
    assert_eq!(
        [stream_2, stream_1, stream_4],
        *manager.streams_waiting_for_delivery_notifications()
    );

    manager.with_asserted_stream(stream_2, |stream| {
        stream.interests.frame_exchange.delivery_notifications = false;
    });
    assert_eq!(
        [stream_1, stream_4],
        *manager.streams_waiting_for_delivery_notifications()
    );

    manager.with_asserted_stream(stream_4, |stream| {
        stream.interests.frame_exchange.delivery_notifications = false;
    });
    assert_eq!(
        [stream_1],
        *manager.streams_waiting_for_delivery_notifications()
    );

    manager.with_asserted_stream(stream_1, |stream| {
        stream.interests.frame_exchange.delivery_notifications = false;
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
    let mut manager = create_stream_manager(EndpointType::Server);

    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Unidirectional).unwrap();
    let stream_3 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_4 = manager.open(StreamType::Unidirectional).unwrap();

    for stream_id in &[stream_2, stream_1, stream_4] {
        let read_waker = read_waker.clone();
        let write_waker = write_waker.clone();
        manager.with_asserted_stream(*stream_id, |stream| {
            stream.interests.frame_exchange.delivery_notifications = true;
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
        stream.interests.frame_exchange.delivery_notifications = false;
    });
    manager.with_asserted_stream(stream_2, |stream| {
        stream.interests.frame_exchange.delivery_notifications = false;
    });
    manager.with_asserted_stream(stream_3, |stream| {
        stream.interests.frame_exchange.delivery_notifications = true;
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.interests.frame_exchange.delivery_notifications = false;
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
    let mut manager = create_stream_manager(EndpointType::Server);

    // Consume all window
    let current_window =
        manager.with_outgoing_connection_flow_controller(|ctrl| ctrl.total_window());
    assert_eq!(
        current_window,
        manager
            .with_outgoing_connection_flow_controller(|ctrl| ctrl.acquire_window(current_window))
    );

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Unidirectional).unwrap();
    let stream_3 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_4 = manager.open(StreamType::Unidirectional).unwrap();

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
        stream.interests.frame_exchange.delivery_notifications = true;
        stream.on_transmit_try_write_frames = 1;
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_transmit_try_write_frames = 1;
        stream.write_waker_to_return = Some(write_waker);
    });

    manager.close(StreamError::ConnectionClosed(VarInt::from_u32(1).into()));
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
    let mut manager = create_stream_manager(EndpointType::Server);

    // Create some open Streams with interests
    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Unidirectional).unwrap();
    let _stream_3 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_4 = manager.open(StreamType::Unidirectional).unwrap();

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

    let mut manager = create_stream_manager(EndpointType::Server);
    let mut frame_buffer = OutgoingFrameBuffer::new();

    // Create some open Streams with interests
    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Unidirectional).unwrap();
    let stream_3 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_4 = manager.open(StreamType::Unidirectional).unwrap();

    manager.with_asserted_stream(stream_1, |stream| {
        stream.on_transmit_try_write_frames = 1;
    });
    manager.with_asserted_stream(stream_3, |stream| {
        stream.on_transmit_try_write_frames = 1;
    });
    manager.with_asserted_stream(stream_4, |stream| {
        stream.on_transmit_try_write_frames = 1;
    });
    assert_eq!(
        [stream_1, stream_3, stream_4],
        *manager.streams_waiting_for_transmission()
    );

    let connection_context = MockConnectionContext::new(EndpointType::Server);
    let mut write_context = MockWriteContext::new(
        &connection_context,
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
    );
    assert!(manager.on_transmit(&mut write_context).is_ok());

    for stream_id in &[stream_1, stream_3, stream_4] {
        assert_stream_write_state(&mut manager, *stream_id, 1, 0);
    }

    // All streams have written a frame
    assert_eq!(3, frame_buffer.len());
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
    let connection_context = MockConnectionContext::new(EndpointType::Server);
    let mut write_context = MockWriteContext::new(
        &connection_context,
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
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
    let connection_context = MockConnectionContext::new(EndpointType::Server);
    let mut write_context = MockWriteContext::new(
        &connection_context,
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
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
    let connection_context = MockConnectionContext::new(EndpointType::Server);
    let mut write_context = MockWriteContext::new(
        &connection_context,
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
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
    let connection_context = MockConnectionContext::new(EndpointType::Server);
    let mut write_context = MockWriteContext::new(
        &connection_context,
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
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
    let connection_context = MockConnectionContext::new(EndpointType::Server);
    let mut write_context = MockWriteContext::new(
        &connection_context,
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
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

fn invalid_stream_id(local_ep_type: EndpointType) -> StreamId {
    StreamId::nth(local_ep_type, StreamType::Bidirectional, 100_000).unwrap()
}

#[test]
fn forwards_on_data() {
    let mut manager = create_stream_manager(EndpointType::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Bidirectional).unwrap();

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
        stream.next_packet_error = Some(TransportError::new(
            TransportError::INTERNAL_ERROR,
            "",
            None,
        ));
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
    frame.stream_id = invalid_stream_id(EndpointType::Server).into();
    assert_is_transport_error(manager.on_data(&frame), TransportError::STREAM_STATE_ERROR);
}

#[test]
fn forwards_on_stream_data_blocked() {
    let mut manager = create_stream_manager(EndpointType::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Bidirectional).unwrap();

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
        stream.next_packet_error = Some(TransportError::new(
            TransportError::INTERNAL_ERROR,
            "",
            None,
        ));
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
    frame.stream_id = invalid_stream_id(EndpointType::Server).into();
    assert_is_transport_error(
        manager.on_stream_data_blocked(&frame),
        TransportError::STREAM_STATE_ERROR,
    );
}

#[test]
fn forwards_on_max_stream_data() {
    let mut manager = create_stream_manager(EndpointType::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Bidirectional).unwrap();

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
        stream.next_packet_error = Some(TransportError::new(
            TransportError::INTERNAL_ERROR,
            "",
            None,
        ));
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
    frame.stream_id = invalid_stream_id(EndpointType::Server).into();
    assert_is_transport_error(
        manager.on_max_stream_data(&frame),
        TransportError::STREAM_STATE_ERROR,
    );
}

#[test]
fn forwards_on_stop_sending() {
    let mut manager = create_stream_manager(EndpointType::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Bidirectional).unwrap();

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
        stream.next_packet_error = Some(TransportError::new(
            TransportError::INTERNAL_ERROR,
            "",
            None,
        ));
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
    frame.stream_id = invalid_stream_id(EndpointType::Server).into();
    assert_is_transport_error(
        manager.on_stop_sending(&frame),
        TransportError::STREAM_STATE_ERROR,
    );
}

#[test]
fn forwards_on_reset() {
    let mut manager = create_stream_manager(EndpointType::Server);
    let (read_waker, read_wake_counter) = new_count_waker();
    let (write_waker, write_wake_counter) = new_count_waker();

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let stream_2 = manager.open(StreamType::Bidirectional).unwrap();

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
        stream.next_packet_error = Some(TransportError::new(
            TransportError::INTERNAL_ERROR,
            "",
            None,
        ));
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
    frame.stream_id = invalid_stream_id(EndpointType::Server).into();
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
    let mut manager = create_stream_manager(EndpointType::Server);

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();

    let ctx = Context::from_waker(&waker);
    assert_eq!(
        Poll::Ready(Ok(None)),
        manager.poll_pop(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            &ctx
        )
    );

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.poll_pop_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert_eq!(EMPTY_STREAM_MANAGER_INTERESTS, manager.interests());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Poll::Ready(Err(StreamError::MaxStreamDataSizeExceeded)),
        manager.poll_pop(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            &ctx
        )
    );
    assert_eq!(TX_STREAM_MANAGER_INTEREST, manager.interests());
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Poll::Ready(Err(StreamError::InvalidStream)),
        manager.poll_pop(
            invalid_stream_id(EndpointType::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            &ctx
        )
    );
}

#[test]
fn forwards_stop_sending() {
    let (mut wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();
    let mut manager = create_stream_manager(EndpointType::Server);

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let error = ApplicationErrorCode::new(0x123_456).unwrap();

    assert_eq!(
        Ok(()),
        manager.stop_sending(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            error
        )
    );

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.stop_sending_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert_eq!(EMPTY_STREAM_MANAGER_INTERESTS, manager.interests());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Err(StreamError::MaxStreamDataSizeExceeded),
        manager.stop_sending(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            error
        )
    );
    assert_eq!(TX_STREAM_MANAGER_INTEREST, manager.interests());
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Err(StreamError::InvalidStream),
        manager.stop_sending(
            invalid_stream_id(EndpointType::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            error,
        )
    );
}

#[test]
fn forwards_poll_push() {
    let (mut wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();
    let (waker, _wake_counter) = new_count_waker();
    let mut manager = create_stream_manager(EndpointType::Server);

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let data = Bytes::from_static(b"1234");

    let ctx = Context::from_waker(&waker);
    assert_eq!(
        Poll::Ready(Ok(())),
        manager.poll_push(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            data.clone(),
            &ctx
        )
    );

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.poll_push_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert_eq!(EMPTY_STREAM_MANAGER_INTERESTS, manager.interests());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Poll::Ready(Err(StreamError::MaxStreamDataSizeExceeded)),
        manager.poll_push(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            data.clone(),
            &ctx
        )
    );
    assert_eq!(TX_STREAM_MANAGER_INTEREST, manager.interests());
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Poll::Ready(Err(StreamError::InvalidStream)),
        manager.poll_push(
            invalid_stream_id(EndpointType::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            data,
            &ctx
        )
    );
}

#[test]
fn forwards_poll_finish() {
    let (mut wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();
    let (waker, _wake_counter) = new_count_waker();
    let mut manager = create_stream_manager(EndpointType::Server);

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();

    let ctx = Context::from_waker(&waker);
    assert_eq!(
        Poll::Ready(Ok(())),
        manager.poll_finish(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            &ctx
        )
    );

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.poll_finish_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert_eq!(EMPTY_STREAM_MANAGER_INTERESTS, manager.interests());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Poll::Ready(Err(StreamError::MaxStreamDataSizeExceeded)),
        manager.poll_finish(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            &ctx
        )
    );
    assert_eq!(TX_STREAM_MANAGER_INTEREST, manager.interests());
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Poll::Ready(Err(StreamError::InvalidStream)),
        manager.poll_finish(
            invalid_stream_id(EndpointType::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            &ctx
        )
    );
}

#[test]
fn forwards_reset() {
    let (mut wakeup_queue, mut wakeup_handle) = create_wakeup_queue_and_handle();
    let mut manager = create_stream_manager(EndpointType::Server);

    let stream_1 = manager.open(StreamType::Bidirectional).unwrap();
    let error = ApplicationErrorCode::new(0x123_456).unwrap();

    assert_eq!(
        Ok(()),
        manager.reset(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            error
        )
    );

    // Check call count and error forwarding
    manager.with_asserted_stream(stream_1, |stream| {
        assert_eq!(stream.reset_count, 1);
        stream.next_api_error = Some(StreamError::MaxStreamDataSizeExceeded);
        stream.api_call_requires_transmission = true;
    });

    assert_eq!(EMPTY_STREAM_MANAGER_INTERESTS, manager.interests());
    assert_wakeups(&mut wakeup_queue, 0);
    assert_eq!(
        Err(StreamError::MaxStreamDataSizeExceeded),
        manager.reset(
            stream_1,
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            error
        )
    );
    assert_eq!(TX_STREAM_MANAGER_INTEREST, manager.interests());
    assert_wakeups(&mut wakeup_queue, 1);

    // Check invalid stream ID
    assert_eq!(
        Err(StreamError::InvalidStream),
        manager.reset(
            invalid_stream_id(EndpointType::Server),
            &mut ConnectionApiCallContext::from_wakeup_handle(&mut wakeup_handle),
            error
        )
    );
}
