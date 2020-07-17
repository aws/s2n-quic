use super::*;
use crate::{
    frame_exchange_interests::FrameExchangeInterestProvider,
    stream::{stream_interests::StreamInterestProvider, StreamEvents, StreamTrait},
};
use s2n_quic_core::{
    application::ApplicationErrorCode,
    connection::ConnectionError,
    endpoint::EndpointType,
    frame::{Frame, MaxData, MaxStreamData, ResetStream, StopSending},
    stream::StreamType,
    varint::VarInt,
};

#[test]
fn locally_initiated_unidirectional_stream_can_not_be_read() {
    for local_endpoint_type in &[EndpointType::Client, EndpointType::Server] {
        let mut test_env_config: TestEnvironmentConfig = Default::default();
        test_env_config.stream_id =
            StreamId::initial(*local_endpoint_type, StreamType::Unidirectional);
        test_env_config.local_endpoint_type = *local_endpoint_type;
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        test_env.assert_end_of_stream();
    }
}

#[test]
fn bidirectional_and_remotely_initiated_unidirectional_streams_can_be_read() {
    for local_endpoint_type in &[EndpointType::Client, EndpointType::Server] {
        for stream_type in &[StreamType::Unidirectional, StreamType::Bidirectional] {
            for initiator in &[EndpointType::Client, EndpointType::Server] {
                // Skip locally initiated unidirectional stream
                if *stream_type == StreamType::Unidirectional && *initiator == *local_endpoint_type
                {
                    continue;
                }

                let stream_id = StreamId::initial(*initiator, *stream_type);
                let mut test_env_config: TestEnvironmentConfig = Default::default();
                test_env_config.stream_id = stream_id;
                test_env_config.local_endpoint_type = *local_endpoint_type;
                let mut test_env = setup_stream_test_env_with_config(test_env_config);
                test_env.assert_no_read_data();
            }
        }
    }
}

/// Sets up a Test environment for Streams where only the receiving half of
/// the Stream is open
fn setup_receive_only_test_env() -> TestEnvironment {
    let mut test_env_config: TestEnvironmentConfig = Default::default();
    test_env_config.stream_id = StreamId::initial(EndpointType::Client, StreamType::Unidirectional);
    test_env_config.local_endpoint_type = EndpointType::Server;
    setup_stream_test_env_with_config(test_env_config)
}

/// Returns a test environment configuration suitable for testing the
/// connection flow control window
fn conn_flow_control_test_env_config() -> TestEnvironmentConfig {
    let mut test_env_config: TestEnvironmentConfig = Default::default();
    test_env_config.stream_id = StreamId::initial(EndpointType::Client, StreamType::Unidirectional);
    test_env_config.local_endpoint_type = EndpointType::Server;
    // Increase the stream window to error on the connection window
    test_env_config.initial_receive_window = core::u32::MAX.into();
    test_env_config.desired_flow_control_window = core::u32::MAX;
    test_env_config.initial_connection_receive_window_size = 10 * 1024;
    test_env_config.desired_connection_flow_control_window = 10 * 1024;
    test_env_config
}

#[test]
fn receive_more_data_unblocks_reader() {
    let mut test_env = setup_receive_only_test_env();
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u8(0),
                &[0, 1, 2, 3],
                false
            ),
            &mut events
        )
        .is_ok());
    assert_eq!(0, events.waker_count());

    test_env.assert_receive_data(&[0, 1, 2, 3]);
    assert_eq!(test_env.wake_counter, 0);

    test_env.assert_no_read_data();
    assert_eq!(test_env.wake_counter, 0);

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u8(4),
                &[4, 5, 6],
                false
            ),
            &mut events
        )
        .is_ok());
    assert_eq!(1, events.waker_count());
    assert_eq!(test_env.wake_counter, 0);
    events.wake_all();
    assert_eq!(test_env.wake_counter, 1);

    test_env.assert_receive_data(&[4, 5, 6]);

    test_env.assert_no_read_data();
    assert_eq!(test_env.wake_counter, 1);

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(test_env.stream.stream_id, VarInt::from_u8(7), &[7], true),
            &mut events
        )
        .is_ok());
    assert_eq!(1, events.waker_count());
    assert_eq!(test_env.wake_counter, 1);
    events.wake_all();
    assert_eq!(test_env.wake_counter, 2);

    test_env.assert_receive_data(&[7]);
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_end_of_stream();
    assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
}

#[test]
fn receive_data_with_fin() {
    let mut test_env = setup_receive_only_test_env();
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u8(0),
                &[0, 1, 2, 3],
                false
            ),
            &mut events
        )
        .is_ok());
    assert_eq!(0, events.waker_count());

    test_env.assert_receive_data(&[0, 1, 2, 3]);
    assert_eq!(test_env.wake_counter, 0);

    test_env.assert_no_read_data();
    assert_eq!(test_env.wake_counter, 0);

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u8(4),
                &[4, 5, 6],
                true
            ),
            &mut events
        )
        .is_ok());
    assert_eq!(1, events.waker_count());
    assert_eq!(test_env.wake_counter, 0);
    events.wake_all();
    assert_eq!(test_env.wake_counter, 1);

    test_env.assert_receive_data(&[4, 5, 6]);
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_end_of_stream();
    assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
}

#[test]
fn receive_data_with_fin_in_separate_frame() {
    let mut test_env = setup_receive_only_test_env();
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u8(0),
                &[0, 1, 2, 3],
                false
            ),
            &mut events
        )
        .is_ok());
    assert_eq!(0, events.waker_count());

    test_env.assert_receive_data(&[0, 1, 2, 3]);
    assert_eq!(test_env.wake_counter, 0);

    test_env.assert_no_read_data();
    assert_eq!(test_env.wake_counter, 0);

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(test_env.stream.stream_id, VarInt::from_u8(4), &[], true),
            &mut events
        )
        .is_ok());
    assert_eq!(1, events.waker_count());
    assert_eq!(test_env.wake_counter, 0);
    events.wake_all();
    assert_eq!(test_env.wake_counter, 1);

    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_end_of_stream();
    assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
}

#[test]
fn receive_fin_only() {
    let mut test_env = setup_receive_only_test_env();
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    test_env.assert_no_read_data();
    assert_eq!(test_env.wake_counter, 0);

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(test_env.stream.stream_id, VarInt::from_u8(0), &[], true),
            &mut events
        )
        .is_ok());
    assert_eq!(1, events.waker_count());
    assert_eq!(test_env.wake_counter, 0);
    events.wake_all();
    assert_eq!(test_env.wake_counter, 1);

    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_end_of_stream();
    assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
}

#[test]
fn receive_fin_with_gap() {
    let mut test_env = setup_receive_only_test_env();

    test_env.assert_no_read_data();
    assert_eq!(test_env.wake_counter, 0);

    // Mark end of stream at offset 4, which leaves a gap at the start
    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(test_env.stream.stream_id, VarInt::from_u8(4), &[4, 5], true),
            &mut events
        )
        .is_ok());
    assert_eq!(0, events.waker_count());
    assert_eq!(test_env.wake_counter, 0);

    test_env.assert_no_read_data();
    assert_eq!(test_env.wake_counter, 0);

    // Fill the gap
    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u8(0),
                &[0, 1, 2, 3],
                false
            ),
            &mut events
        )
        .is_ok());
    assert_eq!(1, events.waker_count());
    assert_eq!(test_env.wake_counter, 0);
    events.wake_all();
    assert_eq!(test_env.wake_counter, 1);

    test_env.assert_receive_data(&[0, 1, 2, 3, 4, 5]);
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_end_of_stream();
    assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
    assert_eq!(test_env.wake_counter, 1);
}

#[test]
fn receive_fin_twice_at_same_position() {
    let mut test_env = setup_receive_only_test_env();

    test_env.assert_no_read_data();
    assert_eq!(test_env.wake_counter, 0);

    // Mark end of stream at the original offset
    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u32(0),
                &[0, 1, 2, 3],
                true
            ),
            &mut events
        )
        .is_ok());
    assert_eq!(1, events.waker_count());

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u32(2),
                &[2, 3],
                true
            ),
            &mut events
        )
        .is_ok());
    assert_eq!(0, events.waker_count());

    test_env.assert_receive_data(&[0, 1, 2, 3]);
    test_env.assert_end_of_stream();
}

#[test]
fn receive_fin_twice_at_different_positions() {
    for delta in &[-1, 1] {
        let mut test_env = setup_receive_only_test_env();

        test_env.assert_no_read_data();
        assert_eq!(test_env.wake_counter, 0);

        // Mark end of stream at the original offset
        let original_fin_offset: i32 = 4;
        let mut events = StreamEvents::new();
        assert!(test_env
            .stream
            .on_data(
                &stream_data(
                    test_env.stream.stream_id,
                    VarInt::from_u32(original_fin_offset as u32),
                    &[],
                    true
                ),
                &mut events
            )
            .is_ok());
        assert_eq!(0, events.waker_count());

        let data_len = original_fin_offset + delta;
        let data = vec![0u8; data_len as usize];
        let mut events = StreamEvents::new();
        assert_is_transport_error(
            test_env.stream.on_data(
                &stream_data(
                    test_env.stream.stream_id,
                    VarInt::from_u8(0),
                    &data[..],
                    true,
                ),
                &mut events,
            ),
            TransportError::FINAL_SIZE_ERROR,
        );

        // Reset the connection after the error
        events = StreamEvents::new();
        test_env
            .stream
            .on_internal_reset(ConnectionError::Unspecified.into(), &mut events);
        assert_eq!(1, events.waker_count());

        test_env.assert_pop_error();
    }
}

#[test]
fn reset_stream() {
    for is_waiting in &[false, true] {
        for has_buffered_data in &[false, true] {
            let mut test_env = setup_receive_only_test_env();
            assert_eq!(stream_interests(&[]), test_env.stream.interests());

            if *is_waiting {
                test_env.assert_no_read_data();
            }

            // If we have buffered data, then it should be dropped after the
            // reset is received.
            if *has_buffered_data {
                let mut events = StreamEvents::new();
                assert!(test_env
                    .stream
                    .on_data(
                        &stream_data(
                            test_env.stream.stream_id,
                            VarInt::from_u8(0),
                            &[0, 1, 2, 3],
                            false
                        ),
                        &mut events
                    )
                    .is_ok());
                if *is_waiting {
                    assert_eq!(1, events.waker_count());
                } else {
                    assert_eq!(0, events.waker_count());
                }
                events.wake_all();
            }

            let reset_frame = ResetStream {
                stream_id: test_env.stream.stream_id.into(),
                application_error_code: VarInt::from_u8(0),
                final_size: VarInt::new(test_env.stream.receive_stream.receive_buffer.len() as u64)
                    .unwrap(),
            };

            let mut events = StreamEvents::new();
            assert!(test_env.stream.on_reset(&reset_frame, &mut events).is_ok());
            if *is_waiting && !*has_buffered_data {
                assert_eq!(1, events.waker_count());
            } else {
                assert_eq!(0, events.waker_count());
            }
            events.wake_all();

            assert_eq!(stream_interests(&[]), test_env.stream.interests());
            test_env.assert_pop_error();
            if *is_waiting {
                assert_eq!(test_env.wake_counter, 1);
            } else {
                assert_eq!(test_env.wake_counter, 0);
            }
            assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
        }
    }
}

#[test]
fn reset_errors_if_final_size_contradicts_fin_size() {
    for final_size in &[0, 400, 799, 801] {
        let mut test_env = setup_receive_only_test_env();

        test_env.feed_data(VarInt::from_u32(0), 800);
        let mut events = StreamEvents::new();
        assert!(test_env
            .stream
            .on_data(
                &stream_data(test_env.stream.stream_id, VarInt::from_u32(800), &[], true),
                &mut events
            )
            .is_ok());

        let reset_frame = ResetStream {
            stream_id: test_env.stream.stream_id.into(),
            application_error_code: VarInt::from_u8(0),
            final_size: VarInt::new(*final_size).unwrap(),
        };

        let mut events = StreamEvents::new();
        assert_is_transport_error(
            test_env.stream.on_reset(&reset_frame, &mut events),
            TransportError::FINAL_SIZE_ERROR,
        );
        events.wake_all();
    }
}

#[test]
fn reset_has_no_impact_if_all_data_had_been_received() {
    for data_is_consumed in &[false, true] {
        let mut test_env = setup_receive_only_test_env();

        let mut events = StreamEvents::new();
        assert!(test_env
            .stream
            .on_data(
                &stream_data(
                    test_env.stream.stream_id,
                    VarInt::from_u8(0),
                    &[0, 1, 2, 3],
                    true
                ),
                &mut events
            )
            .is_ok());

        if *data_is_consumed {
            test_env.assert_receive_data(&[0, 1, 2, 3]);
            test_env.assert_end_of_stream();
            assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
        }

        let reset_frame = ResetStream {
            stream_id: test_env.stream.stream_id.into(),
            application_error_code: VarInt::from_u8(0),
            final_size: VarInt::new(test_env.stream.receive_stream.receive_buffer.len() as u64)
                .unwrap(),
        };
        let mut events = StreamEvents::new();
        assert!(test_env.stream.on_reset(&reset_frame, &mut events).is_ok());

        // If the data hasn't been consumed yet, it should still be available
        // after the reset.
        if !*data_is_consumed {
            test_env.assert_receive_data(&[0, 1, 2, 3]);
            assert_eq!(stream_interests(&[]), test_env.stream.interests());
        }

        test_env.assert_end_of_stream();
        assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
    }
}

#[test]
fn exceed_stream_flow_control_window() {
    let mut test_env = setup_receive_only_test_env();

    let remaining: u64 = test_env
        .stream
        .receive_stream
        .flow_controller
        .current_stream_receive_window()
        .into();

    // Completely fill the flow control window
    test_env.feed_data(VarInt::from_u32(0), remaining as usize);
    // Feed the window a second time to check whether the implementation can
    // deal with the fact that the required window was previously acquired and
    // does not need to get acquired again.
    test_env.feed_data(VarInt::from_u32(0), remaining as usize);

    // Next write errors
    let mut events = StreamEvents::new();
    assert_is_transport_error(
        test_env.stream.on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u32(remaining as u32),
                &[1],
                false,
            ),
            &mut events,
        ),
        TransportError::FLOW_CONTROL_ERROR,
    );

    // Reset the connection after the error
    events = StreamEvents::new();
    test_env
        .stream
        .on_internal_reset(ConnectionError::Unspecified.into(), &mut events);

    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_pop_error();
    assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
}

#[test]
fn exceed_connection_flow_control_window() {
    let mut test_env = setup_stream_test_env_with_config(conn_flow_control_test_env_config());

    let remaining: u64 = test_env
        .rx_connection_flow_controller
        .remaining_window()
        .into();

    // Completely fill the flow control window
    test_env.feed_data(VarInt::from_u32(0), remaining as usize);
    // Feed the window a second time to check whether the implementation can
    // deal with the fact that the required window was previously acquired and
    // does not need to get acquired again.
    test_env.feed_data(VarInt::from_u32(0), remaining as usize);

    // Next write errors
    let mut events = StreamEvents::new();
    assert_is_transport_error(
        test_env.stream.on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u32(remaining as u32),
                &[1],
                false,
            ),
            &mut events,
        ),
        TransportError::FLOW_CONTROL_ERROR,
    );

    // Reset the connection after the error
    events = StreamEvents::new();
    test_env
        .stream
        .on_internal_reset(ConnectionError::Unspecified.into(), &mut events);

    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_pop_error();
    assert_eq!(stream_interests(&["fin"]), test_env.stream.interests());
}

#[test]
fn receiving_data_will_lead_to_a_stream_flow_control_window_update() {
    let mut test_env = setup_receive_only_test_env();

    let old_window: u64 = test_env
        .stream
        .receive_stream
        .flow_controller
        .current_stream_receive_window()
        .into();
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    // Completely fill the flow control window
    test_env.feed_data(VarInt::from_u32(0), old_window as usize);
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    // And drain the data
    assert_eq!(old_window as usize, test_env.consume_all_data());
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    let expected_window = old_window
        + u64::from(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .desired_flow_control_window,
        );
    assert_eq!(
        expected_window,
        Into::<u64>::into(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .current_stream_receive_window()
        )
    );

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(expected_window).unwrap(),
        }),
        sent_frame.as_frame()
    );

    // Nothing new to write
    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());
    test_env.assert_write_frames(0);

    // Acknowledge the MaxStreamData frame
    test_env.ack_packet(sent_frame.packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
}

#[test]
fn receiving_data_will_lead_to_a_connection_flow_control_window_update() {
    let test_env_config = conn_flow_control_test_env_config();
    let mut test_env = setup_stream_test_env_with_config(test_env_config);

    let old_window: u64 = test_env
        .stream
        .receive_stream
        .flow_controller
        .remaining_connection_receive_window()
        .into();
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    // Completely fill the flow control window
    test_env.feed_data(VarInt::from_u32(0), old_window as usize);
    assert_eq!(
        VarInt::from_u32(0),
        Into::<u64>::into(test_env.rx_connection_flow_controller.remaining_window())
    );
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );

    // And drain the data
    assert_eq!(old_window as usize, test_env.consume_all_data());
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&["tx"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );

    assert_eq!(
        VarInt::from_u32(
            test_env
                .rx_connection_flow_controller
                .desired_flow_control_window()
        ),
        Into::<u64>::into(test_env.rx_connection_flow_controller.remaining_window())
    );
    assert_eq!(
        VarInt::new(old_window).unwrap()
            + VarInt::from_u32(test_env_config.desired_connection_flow_control_window),
        Into::<u64>::into(
            test_env
                .rx_connection_flow_controller
                .current_receive_window()
        )
    );

    let expected_window: u64 = old_window
        + u64::from(
            test_env
                .rx_connection_flow_controller
                .desired_flow_control_window(),
        );

    // We expect to have sent a MaxData frame
    // This is sent by the RX Flow Controller, and not by the Stream
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxData(MaxData {
            maximum_data: VarInt::new(expected_window).unwrap(),
        }),
        sent_frame.as_frame()
    );

    // Nothing new to write
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&["ack"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );
    test_env.assert_write_frames(0);

    // Acknowledge the MaxData frame
    test_env.ack_packet(sent_frame.packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );
}

#[test]
fn resetting_a_stream_will_free_remaining_connection_flow_control_window() {
    let test_env_config = conn_flow_control_test_env_config();
    let mut test_env = setup_stream_test_env_with_config(test_env_config);

    // Feed a chunk of data that gets consumed -> Window gets freed
    test_env.feed_data(VarInt::from_u32(0), 1000);
    assert_eq!(
        VarInt::new(test_env_config.initial_connection_receive_window_size - 1000).unwrap(),
        Into::<u64>::into(test_env.rx_connection_flow_controller.remaining_window())
    );
    test_env.consume_all_data();
    assert_eq!(
        VarInt::new(test_env_config.initial_connection_receive_window_size).unwrap(),
        Into::<u64>::into(test_env.rx_connection_flow_controller.remaining_window())
    );

    // Feed another chunk of data which does not yet consumed
    test_env.feed_data(VarInt::from_u32(1000), 1000);
    assert_eq!(
        VarInt::from_u32(test_env_config.desired_connection_flow_control_window - 1000),
        Into::<u64>::into(test_env.rx_connection_flow_controller.remaining_window())
    );
    assert_eq!(
        VarInt::from_u32(test_env_config.desired_connection_flow_control_window + 1000),
        test_env
            .rx_connection_flow_controller
            .current_receive_window()
    );

    let mut events = StreamEvents::new();
    let reset_frame = ResetStream {
        stream_id: test_env.stream.stream_id.into(),
        application_error_code: VarInt::from_u8(0),
        final_size: VarInt::from_u32(2000),
    };
    assert!(test_env.stream.on_reset(&reset_frame, &mut events).is_ok());
    events.wake_all();

    // Expect the connection flow control credits to be available again, since
    // the data got dropped
    assert_eq!(
        VarInt::from_u32(test_env_config.desired_connection_flow_control_window),
        Into::<u64>::into(test_env.rx_connection_flow_controller.remaining_window())
    );
    assert_eq!(
        VarInt::from_u32(test_env_config.desired_connection_flow_control_window + 2000),
        test_env
            .rx_connection_flow_controller
            .current_receive_window()
    );
}

#[test]
fn resetting_a_stream_will_acquire_connection_flow_control_window_up_to_final_size() {
    for remaining in &[0, 10] {
        let test_env_config = conn_flow_control_test_env_config();
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        test_env.feed_data(VarInt::from_u32(0), 2000);
        assert_eq!(
            VarInt::new(test_env_config.initial_connection_receive_window_size).unwrap() - 2000,
            test_env.rx_connection_flow_controller.remaining_window()
        );

        let final_size =
            VarInt::new(test_env_config.initial_connection_receive_window_size - *remaining)
                .unwrap();
        let mut events = StreamEvents::new();
        let reset_frame = ResetStream {
            stream_id: test_env.stream.stream_id.into(),
            application_error_code: VarInt::from_u8(0),
            final_size,
        };
        assert!(test_env.stream.on_reset(&reset_frame, &mut events).is_ok());
        events.wake_all();

        // The connection window which is available after the reset is the
        // final size of the Stream (which is all data which is deemed to be consumed
        // by the Stream) plus the desired window we want to maintain.
        assert_eq!(
            final_size + VarInt::from_u32(test_env_config.desired_connection_flow_control_window),
            test_env
                .rx_connection_flow_controller
                .current_receive_window()
        );

        // The remaining window is the full desired window, since resetting the
        // stream also increased the window which we synchronize to the peer
        assert_eq!(
            VarInt::from_u32(
                test_env
                    .rx_connection_flow_controller
                    .desired_flow_control_window()
            ),
            test_env.rx_connection_flow_controller.remaining_window()
        );
    }
}

#[test]
fn resetting_a_stream_errors_if_final_size_exceeds_connection_flow_control_window() {
    for extra in &[1, 10, 100] {
        let test_env_config = conn_flow_control_test_env_config();
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        test_env.feed_data(VarInt::from_u32(0), 2000);
        assert_eq!(
            VarInt::new(test_env_config.initial_connection_receive_window_size).unwrap() - 2000,
            test_env.rx_connection_flow_controller.remaining_window()
        );

        let mut events = StreamEvents::new();
        let reset_frame = ResetStream {
            stream_id: test_env.stream.stream_id.into(),
            application_error_code: VarInt::from_u8(0),
            final_size: VarInt::new(
                test_env_config.initial_connection_receive_window_size + *extra,
            )
            .unwrap(),
        };
        assert_is_transport_error(
            test_env.stream.on_reset(&reset_frame, &mut events),
            TransportError::FLOW_CONTROL_ERROR,
        );
        events.wake_all();
    }
}

#[test]
fn flow_control_window_update_is_only_sent_when_minimum_data_size_is_consumed() {
    let mut test_env = setup_receive_only_test_env();

    let relative_treshold = test_env
        .stream
        .receive_stream
        .flow_controller
        .desired_flow_control_window
        / 10;
    // This is the size that the read window needs to go to in order to send
    // an outgoing window update.
    let absolute_treshold: u64 = Into::<u64>::into(
        test_env
            .stream
            .receive_stream
            .flow_controller
            .current_stream_receive_window(),
    ) + u64::from(relative_treshold);
    // This is the amount of bytes that need to have been received to bring the
    // receive window up to that level
    let required_for_read_window_update = absolute_treshold
        - u64::from(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .desired_flow_control_window,
        );

    // Feed up to the treshold
    test_env.feed_data(
        VarInt::from_u32(0),
        required_for_read_window_update as usize - 1,
    );
    // And drain the data
    assert_eq!(
        required_for_read_window_update as usize - 1,
        test_env.consume_all_data()
    );
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    let expected_window = absolute_treshold as u64 - 1;
    assert_eq!(
        expected_window,
        Into::<u64>::into(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .current_stream_receive_window()
        )
    );

    // Nothing to write
    test_env.assert_write_frames(0);
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    // Send and consume one more byte to go over the absolute treshold
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32 - 1),
        1,
    );
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(1, test_env.consume_all_data());
    assert_eq!(
        absolute_treshold,
        Into::<u64>::into(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .current_stream_receive_window()
        )
    );
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(absolute_treshold).unwrap(),
        }),
        sent_frame.as_frame()
    );
    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());

    // Nothing new to write
    test_env.assert_write_frames(0);
}

#[test]
fn connection_flow_control_window_update_is_only_sent_when_minimum_data_size_is_consumed() {
    let test_env_config = conn_flow_control_test_env_config();
    let mut test_env = setup_stream_test_env_with_config(test_env_config);

    let relative_treshold = test_env_config.desired_connection_flow_control_window / 10;
    // This is the size that the read window needs to go to in order to send
    // an outgoing window update.
    let absolute_treshold: u64 =
        Into::<u64>::into(test_env_config.initial_connection_receive_window_size)
            + u64::from(relative_treshold);
    // This is the amount of bytes that need to have been received to bring the
    // receive window up to that level
    let required_for_read_window_update =
        absolute_treshold - u64::from(test_env_config.desired_connection_flow_control_window);

    // Feed up to the treshold
    test_env.feed_data(
        VarInt::from_u32(0),
        required_for_read_window_update as usize - 1,
    );
    // And drain the data
    assert_eq!(
        required_for_read_window_update as usize - 1,
        test_env.consume_all_data()
    );
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );

    let expected_window = absolute_treshold as u64 - 1;
    assert_eq!(
        expected_window,
        Into::<u64>::into(
            test_env
                .rx_connection_flow_controller
                .current_receive_window()
        )
    );

    // Nothing to write
    test_env.assert_write_frames(0);
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );

    // Send and consume one more byte to go over the absolute treshold
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32 - 1),
        1,
    );
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(1, test_env.consume_all_data());
    assert_eq!(
        absolute_treshold,
        Into::<u64>::into(
            test_env
                .rx_connection_flow_controller
                .current_receive_window()
        )
    );
    assert_eq!(
        frame_exchange_interests(&["tx"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );

    // We expect to have sent a MaxData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxData(MaxData {
            maximum_data: VarInt::new(absolute_treshold).unwrap(),
        }),
        sent_frame.as_frame()
    );
    assert_eq!(
        frame_exchange_interests(&["ack"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );

    // Nothing new to write
    test_env.assert_write_frames(0);
}

#[test]
fn if_flow_control_window_is_increased_enough_multiple_frames_are_emitted() {
    let mut test_env = setup_receive_only_test_env();

    let relative_treshold = test_env
        .stream
        .receive_stream
        .flow_controller
        .desired_flow_control_window as usize
        / 10;
    // This is the size that the read window needs to go to in order to send
    // an outgoing window update.
    let absolute_treshold = Into::<u64>::into(
        test_env
            .stream
            .receive_stream
            .flow_controller
            .current_stream_receive_window(),
    ) + relative_treshold as u64;
    // This is the amount of bytes that need to have been received to bring the
    // receive window up to that level
    let required_for_read_window_update = absolute_treshold
        - u64::from(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .desired_flow_control_window,
        );

    // Feed up to the treshold and drain data
    test_env.feed_data(
        VarInt::from_u32(0),
        required_for_read_window_update as usize,
    );
    assert_eq!(
        required_for_read_window_update as usize,
        test_env.consume_all_data()
    );
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(absolute_treshold).unwrap(),
        }),
        sent_frame.as_frame()
    );

    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());
    test_env.assert_write_frames(0);

    // Feed right before next treshold and drain data
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32),
        relative_treshold - 1,
    );
    assert_eq!(relative_treshold - 1, test_env.consume_all_data());
    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());

    // Feed up to the next treshold and drain data
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32 + relative_treshold as u32 - 1),
        1,
    );
    assert_eq!(1, test_env.consume_all_data());
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(absolute_treshold + relative_treshold as u64).unwrap(),
        }),
        sent_frame.as_frame()
    );

    test_env.assert_write_frames(0);

    // The acknowledgement of the first frame is now no longer interesting
    test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());
    test_env.ack_packet(pn(1), ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
}

#[test]
fn if_connection_flow_control_window_is_increased_enough_multiple_frames_are_emitted() {
    let test_env_config: TestEnvironmentConfig = conn_flow_control_test_env_config();
    let mut test_env = setup_stream_test_env_with_config(test_env_config);

    let relative_treshold = test_env_config.desired_connection_flow_control_window as usize / 10;
    // This is the size that the read window needs to go to in order to send
    // an outgoing window update.
    let absolute_treshold =
        Into::<u64>::into(test_env_config.initial_connection_receive_window_size)
            + relative_treshold as u64;
    // This is the amount of bytes that need to have been received to bring the
    // receive window up to that level
    let required_for_read_window_update =
        absolute_treshold - u64::from(test_env_config.desired_connection_flow_control_window);

    // Feed up to the treshold and drain data
    test_env.feed_data(
        VarInt::from_u32(0),
        required_for_read_window_update as usize,
    );
    assert_eq!(
        required_for_read_window_update as usize,
        test_env.consume_all_data()
    );
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&["tx"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );

    // We expect to have sent a MaxData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxData(MaxData {
            maximum_data: VarInt::new(absolute_treshold).unwrap(),
        }),
        sent_frame.as_frame()
    );

    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&["ack"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );
    test_env.assert_write_frames(0);

    // Feed right before next treshold and drain data
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32),
        relative_treshold - 1,
    );
    assert_eq!(relative_treshold - 1, test_env.consume_all_data());
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&["ack"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );
    test_env.assert_write_frames(0);

    // Feed up to the next treshold and drain data
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32 + relative_treshold as u32 - 1),
        1,
    );
    assert_eq!(1, test_env.consume_all_data());
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&["tx"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxData(MaxData {
            maximum_data: VarInt::new(absolute_treshold + relative_treshold as u64).unwrap(),
        }),
        sent_frame.as_frame()
    );

    assert_eq!(
        frame_exchange_interests(&["ack"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );
    test_env.assert_write_frames(0);

    // The acknowledgement of the first frame is now no longer interesting
    test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&["ack"]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );
    test_env.ack_packet(pn(1), ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    assert_eq!(
        frame_exchange_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .frame_exchange_interests()
    );
}

#[test]
fn resend_flow_control_update_if_lost() {
    let mut test_env = setup_receive_only_test_env();

    let old_window: u64 = test_env
        .stream
        .receive_stream
        .flow_controller
        .current_stream_receive_window()
        .into();

    // Completely fill and drain the flow control window
    test_env.feed_data(VarInt::from_u32(0), old_window as usize);
    assert_eq!(old_window as usize, test_env.consume_all_data());
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    let expected_window = old_window
        + u64::from(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .desired_flow_control_window,
        );
    assert_eq!(
        expected_window,
        Into::<u64>::into(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .current_stream_receive_window()
        )
    );

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(expected_window).unwrap(),
        }),
        sent_frame.as_frame()
    );
    let packet_nr = sent_frame.packet_nr;
    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());

    // Nothing new to write
    test_env.assert_write_frames(0);

    // Notify the stream about packet loss
    test_env.nack_packet(packet_nr);
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(expected_window).unwrap(),
        }),
        sent_frame.as_frame()
    );

    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());

    // Nothing new to write
    test_env.assert_write_frames(0);

    // Acknowledging the old packet does nothing
    test_env.ack_packet(packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());
    // When acknowleding the new frame we are done
    test_env.ack_packet(sent_frame.packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
}

#[test]
fn do_not_send_flow_control_update_if_stream_is_reset_or_eof() {
    for test_eof in &[true, false] {
        let mut test_env = setup_receive_only_test_env();

        // Write the full flow control window - this is the easiest way to force an update
        let to_write: u64 = test_env
            .stream
            .receive_stream
            .flow_controller
            .current_stream_receive_window()
            .into();
        test_env.feed_data(VarInt::from_u32(0), to_write as usize);
        assert_eq!(to_write as usize, test_env.consume_all_data());

        assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

        assert_eq!(
            to_write
                + u64::from(
                    test_env
                        .stream
                        .receive_stream
                        .flow_controller
                        .desired_flow_control_window
                ),
            Into::<u64>::into(
                test_env
                    .stream
                    .receive_stream
                    .flow_controller
                    .current_stream_receive_window()
            )
        );

        if *test_eof {
            let mut events = StreamEvents::new();
            assert!(test_env
                .stream
                .on_data(
                    &stream_data(
                        test_env.stream.stream_id,
                        VarInt::from_u32(to_write as u32),
                        &[],
                        true
                    ),
                    &mut events
                )
                .is_ok());
        } else {
            let reset_frame = ResetStream {
                stream_id: test_env.stream.stream_id.into(),
                application_error_code: VarInt::from_u8(0),
                final_size: VarInt::new(test_env.stream.receive_stream.receive_buffer.len() as u64)
                    .unwrap(),
            };
            let mut events = StreamEvents::new();
            assert!(test_env.stream.on_reset(&reset_frame, &mut events).is_ok());
        }

        // Expect no window update
        assert_eq!(stream_interests(&[]), test_env.stream.interests());
        test_env.assert_write_frames(0);
    }
}

#[test]
fn stop_sending_will_trigger_a_stop_sending_frame() {
    for available_data in &[0, 1] {
        for consume_data in &[false, true] {
            let mut test_env = setup_receive_only_test_env();
            test_env.assert_write_frames(0);

            if *available_data != 0 {
                let mut events = StreamEvents::new();
                let data = vec![0u8; *available_data];
                assert!(test_env
                    .stream
                    .on_data(
                        &stream_data(
                            test_env.stream.stream_id,
                            VarInt::from_u8(0),
                            &data[..],
                            false
                        ),
                        &mut events,
                    )
                    .is_ok());
            }

            if *consume_data {
                assert_eq!(*available_data, test_env.consume_all_data());
            }
            assert_eq!(stream_interests(&[]), test_env.stream.interests());

            assert!(test_env
                .stream
                .stop_sending(
                    ApplicationErrorCode::new(0x1234_5678).unwrap(),
                    &test_env.connection_context
                )
                .is_ok());
            assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

            // We expect to have sent a StopSending frame
            test_env.assert_write_frames(1);
            let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
            assert_eq!(
                Frame::StopSending(StopSending {
                    stream_id: test_env.stream.stream_id.into(),
                    application_error_code: VarInt::new(0x1234_5678).unwrap(),
                }),
                sent_frame.as_frame()
            );
            let packet_nr = sent_frame.packet_nr;

            assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());

            // Nothing new to write
            test_env.assert_write_frames(0);

            // Mark the frame as acknowledged
            let mut events = StreamEvents::new();
            test_env.stream.on_packet_ack(&packet_nr, &mut events);

            // Nothing new to write
            assert_eq!(stream_interests(&[]), test_env.stream.interests());
            test_env.assert_write_frames(0);
        }
    }
}

#[test]
fn do_not_retransmit_stop_sending_if_requested_twice() {
    for ack_packet in &[true, false] {
        for resend_before_write in &[true, false] {
            let mut test_env = setup_receive_only_test_env();
            test_env.assert_write_frames(0);

            assert!(test_env
                .stream
                .stop_sending(
                    ApplicationErrorCode::new(0x1234_5678).unwrap(),
                    &test_env.connection_context
                )
                .is_ok());
            assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

            if *resend_before_write {
                assert!(test_env
                    .stream
                    .stop_sending(
                        ApplicationErrorCode::new(0x4321_5678).unwrap(),
                        &test_env.connection_context
                    )
                    .is_ok());
                assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());
            }

            // We expect to have sent a StopSending frame
            test_env.assert_write_frames(1);
            let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
            assert_eq!(
                Frame::StopSending(StopSending {
                    stream_id: test_env.stream.stream_id.into(),
                    application_error_code: VarInt::new(0x1234_5678).unwrap(),
                }),
                sent_frame.as_frame()
            );
            assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());

            if *ack_packet {
                // Mark the frame as acknowledged
                let mut events = StreamEvents::new();
                test_env
                    .stream
                    .on_packet_ack(&sent_frame.packet_nr, &mut events);
                assert_eq!(stream_interests(&[]), test_env.stream.interests());
            }

            assert!(test_env
                .stream
                .stop_sending(
                    ApplicationErrorCode::new(0x1234_5678).unwrap(),
                    &test_env.connection_context
                )
                .is_ok());

            // Nothing new to write
            if *ack_packet {
                assert_eq!(stream_interests(&[]), test_env.stream.interests());
            } else {
                assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());
            }
            test_env.assert_write_frames(0);
        }
    }
}

#[test]
fn stop_sending_is_ignored_if_stream_is_already_reset() {
    let mut test_env = setup_receive_only_test_env();

    let reset_frame = ResetStream {
        stream_id: test_env.stream.stream_id.into(),
        application_error_code: VarInt::from_u8(0),
        final_size: VarInt::new(test_env.stream.receive_stream.receive_buffer.len() as u64)
            .unwrap(),
    };
    let mut events = StreamEvents::new();
    assert!(test_env.stream.on_reset(&reset_frame, &mut events).is_ok());
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    assert!(test_env
        .stream
        .stop_sending(
            ApplicationErrorCode::new(0x1234_5678).unwrap(),
            &test_env.connection_context
        )
        .is_ok());
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    test_env.assert_write_frames(0);
}

#[test]
fn stop_sending_is_cancelled_if_stream_is_reset_after_having_been_initiated() {
    let mut test_env = setup_receive_only_test_env();

    assert!(test_env
        .stream
        .stop_sending(
            ApplicationErrorCode::new(0x1234_5678).unwrap(),
            &test_env.connection_context
        )
        .is_ok());
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    let reset_frame = ResetStream {
        stream_id: test_env.stream.stream_id.into(),
        application_error_code: VarInt::from_u8(0),
        final_size: VarInt::new(test_env.stream.receive_stream.receive_buffer.len() as u64)
            .unwrap(),
    };
    let mut events = StreamEvents::new();
    assert!(test_env.stream.on_reset(&reset_frame, &mut events).is_ok());

    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_write_frames(0);
}

#[test]
fn stop_sending_is_ignored_if_stream_has_already_received_all_data() {
    let mut test_env = setup_receive_only_test_env();

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u8(0),
                &[1, 2, 3],
                true
            ),
            &mut events,
        )
        .is_ok());
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    assert!(test_env
        .stream
        .stop_sending(
            ApplicationErrorCode::new(0x1234_5678).unwrap(),
            &test_env.connection_context
        )
        .is_ok());
    assert_eq!(stream_interests(&[]), test_env.stream.interests());

    test_env.assert_write_frames(0);
}

#[test]
fn stop_sending_can_be_sent_if_size_is_known_but_data_is_still_missing() {
    for send_missing_data_before_ack in &[true, false] {
        let mut test_env = setup_receive_only_test_env();

        let mut events = StreamEvents::new();
        assert!(test_env
            .stream
            .on_data(
                &stream_data(test_env.stream.stream_id, VarInt::from_u8(3), &[], true),
                &mut events,
            )
            .is_ok());

        assert!(test_env
            .stream
            .stop_sending(
                ApplicationErrorCode::new(0x1234_5678).unwrap(),
                &test_env.connection_context
            )
            .is_ok());
        assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

        test_env.assert_write_frames(1);
        let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
        assert_eq!(
            Frame::StopSending(StopSending {
                stream_id: test_env.stream.stream_id.into(),
                application_error_code: VarInt::new(0x1234_5678).unwrap(),
            }),
            sent_frame.as_frame()
        );
        assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());

        if *send_missing_data_before_ack {
            let mut events = StreamEvents::new();
            assert!(test_env
                .stream
                .on_data(
                    &stream_data(
                        test_env.stream.stream_id,
                        VarInt::from_u8(0),
                        &[0, 1, 2],
                        true
                    ),
                    &mut events,
                )
                .is_ok());

            // Now we should not require stop sending anymore
            assert_eq!(stream_interests(&[]), test_env.stream.interests());
        }

        test_env.ack_packet(sent_frame.packet_nr, ExpectWakeup(Some(false)));
        assert_eq!(stream_interests(&[]), test_env.stream.interests());

        test_env.assert_write_frames(0);
    }
}

#[test]
fn stop_sending_is_aborted_if_stream_receives_all_data() {
    let mut test_env = setup_receive_only_test_env();

    assert!(test_env
        .stream
        .stop_sending(
            ApplicationErrorCode::new(0x1234_5678).unwrap(),
            &test_env.connection_context
        )
        .is_ok());
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u8(0),
                &[1, 2, 3],
                true
            ),
            &mut events,
        )
        .is_ok());

    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_write_frames(0);
}

#[test]
fn stop_sending_is_aborted_if_stream_receives_all_data_with_data_after_fin() {
    let mut test_env = setup_receive_only_test_env();

    assert!(test_env
        .stream
        .stop_sending(
            ApplicationErrorCode::new(0x1234_5678).unwrap(),
            &test_env.connection_context
        )
        .is_ok());
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    // This sends only the FIN
    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(test_env.stream.stream_id, VarInt::from_u8(3), &[], true),
            &mut events,
        )
        .is_ok());
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    // And this all data in front of the FIN
    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::from_u8(0),
                &[1, 2, 3],
                false
            ),
            &mut events,
        )
        .is_ok());

    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_write_frames(0);
}

#[test]
fn stop_sending_is_ignored_if_stream_has_received_or_consumed_all_data() {
    for do_consume in &[true, false] {
        let mut test_env = setup_receive_only_test_env();

        let mut events = StreamEvents::new();
        assert!(test_env
            .stream
            .on_data(
                &stream_data(
                    test_env.stream.stream_id,
                    VarInt::from_u8(0),
                    &[1, 2, 3],
                    true
                ),
                &mut events,
            )
            .is_ok());

        let expected_interests = if *do_consume {
            assert_eq!(3, test_env.consume_all_data());
            stream_interests(&["fin"])
        } else {
            stream_interests(&[])
        };

        assert_eq!(expected_interests, test_env.stream.interests());

        assert!(test_env
            .stream
            .stop_sending(
                ApplicationErrorCode::new(0x1234_5678).unwrap(),
                &test_env.connection_context
            )
            .is_ok());
        assert_eq!(expected_interests, test_env.stream.interests());

        test_env.assert_write_frames(0);
    }
}

#[test]
fn stop_sending_frames_are_retransmitted_on_loss() {
    let mut test_env = setup_receive_only_test_env();
    test_env.assert_write_frames(0);

    assert!(test_env
        .stream
        .stop_sending(
            ApplicationErrorCode::new(0x1234_5678).unwrap(),
            &test_env.connection_context
        )
        .is_ok());
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());

    // We expect to have sent a StopSending frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::StopSending(StopSending {
            stream_id: test_env.stream.stream_id.into(),
            application_error_code: VarInt::new(0x1234_5678).unwrap(),
        }),
        sent_frame.as_frame()
    );
    let packet_nr = sent_frame.packet_nr;
    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());

    // Nothing new to write
    test_env.assert_write_frames(0);

    // Mark the frame as lost
    let mut events = StreamEvents::new();
    test_env.stream.on_packet_loss(&packet_nr, &mut events);

    // Expect a retransmission of StopSending
    assert_eq!(stream_interests(&["tx"]), test_env.stream.interests());
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::StopSending(StopSending {
            stream_id: test_env.stream.stream_id.into(),
            application_error_code: VarInt::new(0x1234_5678).unwrap(),
        }),
        sent_frame.as_frame()
    );
    let packet_nr_2 = sent_frame.packet_nr;
    assert!(packet_nr != packet_nr_2);
    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());

    // Nothing new to write
    test_env.assert_write_frames(0);
    // First packet does not matter anymore
    test_env.ack_packet(packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&["ack"]), test_env.stream.interests());
    // Acknowledge the second packet and we are done
    test_env.ack_packet(packet_nr_2, ExpectWakeup(Some(false)));
    assert_eq!(stream_interests(&[]), test_env.stream.interests());
    test_env.assert_write_frames(0);
}
