// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    stream::{stream_interests::StreamInterestProvider, testing::*, StreamEvents, StreamTrait},
    transmission,
    transmission::interest::Provider,
};
use bytes::Bytes;
use core::{convert::TryFrom, task::Poll};
use s2n_quic_core::{
    application::Error as ApplicationErrorCode,
    connection, endpoint,
    frame::{Frame, MaxData, MaxStreamData, ResetStream, StopSending},
    stream::{ops, StreamError, StreamType},
    transport::Error as TransportError,
    varint::VarInt,
};

#[test]
fn locally_initiated_unidirectional_stream_can_not_be_read() {
    for local_endpoint_type in &[endpoint::Type::Client, endpoint::Type::Server] {
        let mut test_env_config: TestEnvironmentConfig =
            TestEnvironmentConfig::new(endpoint::Type::Server);
        test_env_config.stream_id =
            StreamId::initial(*local_endpoint_type, StreamType::Unidirectional);
        test_env_config.local_endpoint_type = *local_endpoint_type;
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        test_env.assert_end_of_stream();
    }
}

#[test]
fn bidirectional_and_remotely_initiated_unidirectional_streams_can_be_read() {
    for local_endpoint_type in &[endpoint::Type::Client, endpoint::Type::Server] {
        for stream_type in &[StreamType::Unidirectional, StreamType::Bidirectional] {
            for initiator in &[endpoint::Type::Client, endpoint::Type::Server] {
                // Skip locally initiated unidirectional stream
                if *stream_type == StreamType::Unidirectional && *initiator == *local_endpoint_type
                {
                    continue;
                }

                let stream_id = StreamId::initial(*initiator, *stream_type);
                let mut test_env_config: TestEnvironmentConfig =
                    TestEnvironmentConfig::new(endpoint::Type::Server);
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
    let mut test_env_config: TestEnvironmentConfig =
        TestEnvironmentConfig::new(endpoint::Type::Server);
    test_env_config.stream_id = StreamId::initial(
        test_env_config.local_endpoint_type.peer_type(),
        StreamType::Unidirectional,
    );
    setup_stream_test_env_with_config(test_env_config)
}

/// Returns a test environment configuration suitable for testing the
/// connection flow control window
fn conn_flow_control_test_env_config() -> TestEnvironmentConfig {
    let mut test_env_config: TestEnvironmentConfig = Default::default();
    test_env_config.stream_id = StreamId::initial(
        test_env_config.local_endpoint_type.peer_type(),
        StreamType::Unidirectional,
    );
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
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

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
    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_end_of_stream();
    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
}

#[test]
fn receive_data_with_fin() {
    let mut test_env = setup_receive_only_test_env();
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

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
    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_end_of_stream();
    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
}

#[test]
fn receive_data_with_fin_in_separate_frame() {
    let mut test_env = setup_receive_only_test_env();
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

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

    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_end_of_stream();
    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
}

#[test]
fn receive_fin_only() {
    let mut test_env = setup_receive_only_test_env();
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

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

    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_end_of_stream();
    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
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
    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_end_of_stream();
    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
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
            .on_internal_reset(connection::Error::unspecified().into(), &mut events);
        assert_eq!(1, events.waker_count());

        test_env.assert_pop_error();
    }
}

#[test]
fn reset_stream() {
    for is_waiting in &[false, true] {
        for has_buffered_data in &[false, true] {
            let mut test_env = setup_receive_only_test_env();
            assert_eq!(
                stream_interests(&[]),
                test_env.stream.get_stream_interests()
            );

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

            assert_eq!(
                stream_interests(&[]),
                test_env.stream.get_stream_interests()
            );
            test_env.assert_pop_error();
            if *is_waiting {
                assert_eq!(test_env.wake_counter, 1);
            } else {
                assert_eq!(test_env.wake_counter, 0);
            }
            assert_eq!(
                stream_interests(&["fin"]),
                test_env.stream.get_stream_interests()
            );
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
            assert_eq!(
                stream_interests(&["fin"]),
                test_env.stream.get_stream_interests()
            );
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
            assert_eq!(
                stream_interests(&["fin"]),
                test_env.stream.get_stream_interests()
            );
        }

        test_env.assert_end_of_stream();
        assert_eq!(
            stream_interests(&["fin"]),
            test_env.stream.get_stream_interests()
        );
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
        .on_internal_reset(connection::Error::unspecified().into(), &mut events);

    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_pop_error();
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
        .on_internal_reset(connection::Error::unspecified().into(), &mut events);

    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_pop_error();
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
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

    // Completely fill the flow control window
    test_env.feed_data(VarInt::from_u32(0), old_window as usize);
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    // And drain the data
    assert_eq!(old_window as usize, test_env.consume_all_data());
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

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
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_write_frames(0);

    // Acknowledge the MaxStreamData frame
    test_env.ack_packet(sent_frame.packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
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
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

    // Completely fill the flow control window
    test_env.feed_data(VarInt::from_u32(0), old_window as usize);
    assert_eq!(
        VarInt::from_u32(0),
        Into::<u64>::into(test_env.rx_connection_flow_controller.remaining_window())
    );
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(
        transmission_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
    );

    // And drain the data
    assert_eq!(old_window as usize, test_env.consume_all_data());
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(
        transmission_interests(&["tx"]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
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
    assert_eq!(
        transmission_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
    );
    assert!(test_env.rx_connection_flow_controller.is_inflight());
    test_env.assert_write_frames(0);

    // Acknowledge the MaxData frame
    test_env.ack_packet(sent_frame.packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(
        transmission_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
    );
    assert!(!test_env.rx_connection_flow_controller.is_inflight());
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

    let relative_threshold = test_env
        .stream
        .receive_stream
        .flow_controller
        .desired_flow_control_window
        / 10;
    // This is the size that the read window needs to go to in order to send
    // an outgoing window update.
    let absolute_threshold: u64 = Into::<u64>::into(
        test_env
            .stream
            .receive_stream
            .flow_controller
            .current_stream_receive_window(),
    ) + u64::from(relative_threshold);
    // This is the amount of bytes that need to have been received to bring the
    // receive window up to that level
    let required_for_read_window_update = absolute_threshold
        - u64::from(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .desired_flow_control_window,
        );

    // Feed up to the threshold
    test_env.feed_data(
        VarInt::from_u32(0),
        required_for_read_window_update as usize - 1,
    );
    // And drain the data
    assert_eq!(
        required_for_read_window_update as usize - 1,
        test_env.consume_all_data()
    );
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

    let expected_window = absolute_threshold - 1;
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
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

    // Send and consume one more byte to go over the absolute threshold
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32 - 1),
        1,
    );
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(1, test_env.consume_all_data());
    assert_eq!(
        absolute_threshold,
        Into::<u64>::into(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .current_stream_receive_window()
        )
    );
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(absolute_threshold).unwrap(),
        }),
        sent_frame.as_frame()
    );
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );

    // Nothing new to write
    test_env.assert_write_frames(0);
}

#[test]
fn connection_flow_control_window_update_is_only_sent_when_minimum_data_size_is_consumed() {
    let test_env_config = conn_flow_control_test_env_config();
    let mut test_env = setup_stream_test_env_with_config(test_env_config);

    let relative_threshold = test_env_config.desired_connection_flow_control_window / 10;
    // This is the size that the read window needs to go to in order to send
    // an outgoing window update.
    let absolute_threshold: u64 =
        Into::<u64>::into(test_env_config.initial_connection_receive_window_size)
            + u64::from(relative_threshold);
    // This is the amount of bytes that need to have been received to bring the
    // receive window up to that level
    let required_for_read_window_update =
        absolute_threshold - u64::from(test_env_config.desired_connection_flow_control_window);

    // Feed up to the threshold
    test_env.feed_data(
        VarInt::from_u32(0),
        required_for_read_window_update as usize - 1,
    );
    // And drain the data
    assert_eq!(
        required_for_read_window_update as usize - 1,
        test_env.consume_all_data()
    );
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(
        transmission_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
    );

    let expected_window = absolute_threshold - 1;
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
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(
        transmission_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
    );

    // Send and consume one more byte to go over the absolute threshold
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32 - 1),
        1,
    );
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(1, test_env.consume_all_data());
    assert_eq!(
        absolute_threshold,
        Into::<u64>::into(
            test_env
                .rx_connection_flow_controller
                .current_receive_window()
        )
    );
    assert_eq!(
        transmission_interests(&["tx"]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
    );

    // We expect to have sent a MaxData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxData(MaxData {
            maximum_data: VarInt::new(absolute_threshold).unwrap(),
        }),
        sent_frame.as_frame()
    );
    assert!(test_env.rx_connection_flow_controller.is_inflight());

    // Nothing new to write
    test_env.assert_write_frames(0);
}

#[test]
fn flow_control_window_update_is_not_sent_when_congestion_limited() {
    let mut test_env = setup_receive_only_test_env();
    test_env.transmission_constraint = transmission::Constraint::CongestionLimited;

    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

    // Completely fill the flow control window
    test_env.feed_data(
        VarInt::from_u32(0),
        test_env
            .stream
            .receive_stream
            .flow_controller
            .current_stream_receive_window()
            .as_u64() as usize,
    );
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    // And drain the data
    test_env.consume_all_data();
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

    // No MaxStreamData frame can be written
    test_env.assert_write_frames(0);
}

#[test]
fn only_lost_flow_control_update_is_sent_if_retransmission_only() {
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
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

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
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );

    // Nothing new to write
    test_env.assert_write_frames(0);

    // Notify the stream about packet loss
    test_env.nack_packet(packet_nr);
    assert_eq!(
        stream_interests(&["lost"]),
        test_env.stream.get_stream_interests()
    );

    // Now we are constrained to retransmission only
    test_env.transmission_constraint = transmission::Constraint::RetransmissionOnly;

    // We can send the lost MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(expected_window).unwrap(),
        }),
        sent_frame.as_frame()
    );

    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );

    // Nothing new to write
    test_env.assert_write_frames(0);

    // Completely fill and drain the flow control window
    test_env.feed_data(VarInt::from_u32(old_window as u32), old_window as usize);
    assert_eq!(old_window as usize, test_env.consume_all_data());
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

    // The new MaxStreamData frame cannot be sent
    test_env.assert_write_frames(0);
}

#[test]
fn if_flow_control_window_is_increased_enough_multiple_frames_are_emitted() {
    let mut test_env = setup_receive_only_test_env();

    let relative_threshold = test_env
        .stream
        .receive_stream
        .flow_controller
        .desired_flow_control_window as usize
        / 10;
    // This is the size that the read window needs to go to in order to send
    // an outgoing window update.
    let absolute_threshold = Into::<u64>::into(
        test_env
            .stream
            .receive_stream
            .flow_controller
            .current_stream_receive_window(),
    ) + relative_threshold as u64;
    // This is the amount of bytes that need to have been received to bring the
    // receive window up to that level
    let required_for_read_window_update = absolute_threshold
        - u64::from(
            test_env
                .stream
                .receive_stream
                .flow_controller
                .desired_flow_control_window,
        );

    // Feed up to the threshold and drain data
    test_env.feed_data(
        VarInt::from_u32(0),
        required_for_read_window_update as usize,
    );
    assert_eq!(
        required_for_read_window_update as usize,
        test_env.consume_all_data()
    );
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(absolute_threshold).unwrap(),
        }),
        sent_frame.as_frame()
    );

    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_write_frames(0);

    // Feed right before next threshold and drain data
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32),
        relative_threshold - 1,
    );
    assert_eq!(relative_threshold - 1, test_env.consume_all_data());
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );

    // Feed up to the next threshold and drain data
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32 + relative_threshold as u32 - 1),
        1,
    );
    assert_eq!(1, test_env.consume_all_data());
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxStreamData(MaxStreamData {
            stream_id: test_env.stream.stream_id.into(),
            maximum_stream_data: VarInt::new(absolute_threshold + relative_threshold as u64)
                .unwrap(),
        }),
        sent_frame.as_frame()
    );

    test_env.assert_write_frames(0);

    // The acknowledgement of the first frame is now no longer interesting
    test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );
    test_env.ack_packet(pn(1), ExpectWakeup(Some(false)));
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
}

#[test]
fn if_connection_flow_control_window_is_increased_enough_multiple_frames_are_emitted() {
    let test_env_config: TestEnvironmentConfig = conn_flow_control_test_env_config();
    let mut test_env = setup_stream_test_env_with_config(test_env_config);

    let relative_threshold = test_env_config.desired_connection_flow_control_window as usize / 10;
    // This is the size that the read window needs to go to in order to send
    // an outgoing window update.
    let absolute_threshold =
        Into::<u64>::into(test_env_config.initial_connection_receive_window_size)
            + relative_threshold as u64;
    // This is the amount of bytes that need to have been received to bring the
    // receive window up to that level
    let required_for_read_window_update =
        absolute_threshold - u64::from(test_env_config.desired_connection_flow_control_window);

    // Feed up to the threshold and drain data
    test_env.feed_data(
        VarInt::from_u32(0),
        required_for_read_window_update as usize,
    );
    assert_eq!(
        required_for_read_window_update as usize,
        test_env.consume_all_data()
    );
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(
        transmission_interests(&["tx"]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
    );

    // We expect to have sent a MaxData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxData(MaxData {
            maximum_data: VarInt::new(absolute_threshold).unwrap(),
        }),
        sent_frame.as_frame()
    );

    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert!(test_env.rx_connection_flow_controller.is_inflight());
    test_env.assert_write_frames(0);

    // Feed right before next threshold and drain data
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32),
        relative_threshold - 1,
    );
    assert_eq!(relative_threshold - 1, test_env.consume_all_data());
    assert!(test_env.rx_connection_flow_controller.is_inflight());
    test_env.assert_write_frames(0);

    // Feed up to the next threshold and drain data
    test_env.feed_data(
        VarInt::from_u32(required_for_read_window_update as u32 + relative_threshold as u32 - 1),
        1,
    );
    assert_eq!(1, test_env.consume_all_data());
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(
        transmission_interests(&["tx"]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
    );

    // We expect to have sent a MaxStreamData frame
    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::MaxData(MaxData {
            maximum_data: VarInt::new(absolute_threshold + relative_threshold as u64).unwrap(),
        }),
        sent_frame.as_frame()
    );

    assert!(test_env.rx_connection_flow_controller.is_inflight());
    test_env.assert_write_frames(0);

    // The acknowledgement of the first frame is now no longer interesting
    test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
    assert!(test_env.rx_connection_flow_controller.is_inflight());
    test_env.ack_packet(pn(1), ExpectWakeup(Some(false)));
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    assert_eq!(
        transmission_interests(&[]),
        test_env
            .rx_connection_flow_controller
            .get_transmission_interest()
    );
    assert!(!test_env.rx_connection_flow_controller.is_inflight());
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
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

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
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );

    // Nothing new to write
    test_env.assert_write_frames(0);

    // Notify the stream about packet loss
    test_env.nack_packet(packet_nr);
    assert_eq!(
        stream_interests(&["lost"]),
        test_env.stream.get_stream_interests()
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

    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );

    // Nothing new to write
    test_env.assert_write_frames(0);

    // Acknowledging the old packet does nothing
    test_env.ack_packet(packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );
    // When acknowledging the new frame we are done
    test_env.ack_packet(sent_frame.packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
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

        assert_eq!(
            stream_interests(&["tx"]),
            test_env.stream.get_stream_interests()
        );

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
        assert_eq!(
            stream_interests(&[]),
            test_env.stream.get_stream_interests()
        );
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
            assert_eq!(
                stream_interests(&[]),
                test_env.stream.get_stream_interests()
            );

            assert!(test_env
                .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
                .is_ok());
            assert_eq!(
                stream_interests(&["tx"]),
                test_env.stream.get_stream_interests()
            );

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

            assert_eq!(
                stream_interests(&["ack"]),
                test_env.stream.get_stream_interests()
            );

            // Nothing new to write
            test_env.assert_write_frames(0);

            // Mark the frame as acknowledged
            let mut events = StreamEvents::new();
            test_env.stream.on_packet_ack(&packet_nr, &mut events);

            // Nothing new to write; the stream should be finished
            assert_eq!(
                stream_interests(&[]),
                test_env.stream.get_stream_interests()
            );
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
                .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
                .is_ok());
            assert_eq!(
                stream_interests(&["tx"]),
                test_env.stream.get_stream_interests()
            );

            if *resend_before_write {
                assert!(test_env
                    .stop_sending(ApplicationErrorCode::new(0x4321_5678).unwrap())
                    .is_ok());
                assert_eq!(
                    stream_interests(&["tx"]),
                    test_env.stream.get_stream_interests()
                );
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
            assert_eq!(
                stream_interests(&["ack"]),
                test_env.stream.get_stream_interests()
            );

            if *ack_packet {
                // Mark the frame as acknowledged
                let mut events = StreamEvents::new();
                test_env
                    .stream
                    .on_packet_ack(&sent_frame.packet_nr, &mut events);

                assert_eq!(
                    stream_interests(&[]),
                    test_env.stream.get_stream_interests()
                );
            }

            assert!(test_env
                .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
                .is_ok());

            // Nothing new to write
            if *ack_packet {
                assert_eq!(
                    stream_interests(&[]),
                    test_env.stream.get_stream_interests()
                );
            } else {
                assert_eq!(
                    stream_interests(&["ack"]),
                    test_env.stream.get_stream_interests()
                );
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
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

    assert!(test_env
        .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
        .is_ok());
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

    test_env.assert_write_frames(0);
}

#[test]
fn stop_sending_is_cancelled_if_stream_is_reset_after_having_been_initiated() {
    let mut test_env = setup_receive_only_test_env();

    assert!(test_env
        .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
        .is_ok());
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

    let reset_frame = ResetStream {
        stream_id: test_env.stream.stream_id.into(),
        application_error_code: VarInt::from_u8(0),
        final_size: VarInt::new(test_env.stream.receive_stream.receive_buffer.len() as u64)
            .unwrap(),
    };
    let mut events = StreamEvents::new();
    assert!(test_env.stream.on_reset(&reset_frame, &mut events).is_ok());

    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
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
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );

    assert!(test_env
        .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
        .is_ok());
    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );

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
            .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
            .is_ok());
        assert_eq!(
            stream_interests(&["tx"]),
            test_env.stream.get_stream_interests()
        );

        test_env.assert_write_frames(1);
        let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
        assert_eq!(
            Frame::StopSending(StopSending {
                stream_id: test_env.stream.stream_id.into(),
                application_error_code: VarInt::new(0x1234_5678).unwrap(),
            }),
            sent_frame.as_frame()
        );
        assert_eq!(
            stream_interests(&["ack"]),
            test_env.stream.get_stream_interests()
        );

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
            assert_eq!(
                stream_interests(&["fin"]),
                test_env.stream.get_stream_interests()
            );
        }

        test_env.ack_packet(sent_frame.packet_nr, ExpectWakeup(Some(false)));

        if !*send_missing_data_before_ack {
            assert_eq!(
                stream_interests(&[]),
                test_env.stream.get_stream_interests()
            );
        }

        test_env.assert_write_frames(0);
    }
}

#[test]
fn stop_sending_is_aborted_if_stream_receives_all_data() {
    let mut test_env = setup_receive_only_test_env();

    assert!(test_env
        .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
        .is_ok());
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

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

    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_write_frames(0);
}

#[test]
fn stop_sending_is_aborted_if_stream_receives_all_data_with_data_after_fin() {
    let mut test_env = setup_receive_only_test_env();

    assert!(test_env
        .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
        .is_ok());
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

    // This sends only the FIN
    let mut events = StreamEvents::new();
    assert!(test_env
        .stream
        .on_data(
            &stream_data(test_env.stream.stream_id, VarInt::from_u8(3), &[], true),
            &mut events,
        )
        .is_ok());
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

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

    assert_eq!(
        stream_interests(&["fin"]),
        test_env.stream.get_stream_interests()
    );
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

        assert_eq!(expected_interests, test_env.stream.get_stream_interests());

        assert!(test_env
            .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
            .is_ok());
        assert_eq!(
            stream_interests(&["fin"]),
            test_env.stream.get_stream_interests()
        );

        test_env.assert_write_frames(0);
    }
}

#[test]
fn stop_sending_frames_are_retransmitted_on_loss() {
    let mut test_env = setup_receive_only_test_env();
    test_env.assert_write_frames(0);

    assert!(test_env
        .stop_sending(ApplicationErrorCode::new(0x1234_5678).unwrap())
        .is_ok());
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

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
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );

    // Nothing new to write
    test_env.assert_write_frames(0);

    // Mark the frame as lost
    let mut events = StreamEvents::new();
    test_env.stream.on_packet_loss(&packet_nr, &mut events);

    // Expect a retransmission of StopSending
    assert_eq!(
        stream_interests(&["lost"]),
        test_env.stream.get_stream_interests()
    );
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
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );

    // Nothing new to write
    test_env.assert_write_frames(0);
    // First packet does not matter anymore
    test_env.ack_packet(packet_nr, ExpectWakeup(Some(false)));
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );
    // Acknowledge the second packet and we are done
    test_env.ack_packet(packet_nr_2, ExpectWakeup(Some(false)));
    assert_eq!(
        stream_interests(&[]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_write_frames(0);
}

#[test]
fn receive_multiple_chunks_test() {
    let mut test_env = setup_receive_only_test_env();

    // feed enough data to create multiple slots in the receive buffer
    const AMOUNT: usize = 4000;

    test_env.feed_data(VarInt::from_u8(0), AMOUNT);

    assert_eq!(
        test_env
            .run_request(
                ops::Request::default().receive(&mut [Bytes::new(), Bytes::new()]),
                false
            )
            .unwrap()
            .rx
            .unwrap()
            .chunks
            .consumed,
        1,
        "response should return 1 chunk with contiguous receive buffer"
    );

    test_env.feed_data(VarInt::try_from(AMOUNT).unwrap(), AMOUNT);

    assert_eq!(
        test_env
            .run_request(
                ops::Request::default().receive(&mut [Bytes::new(), Bytes::new(), Bytes::new()]),
                false
            )
            .unwrap()
            .rx
            .unwrap()
            .chunks
            .consumed,
        2,
        "response should return 2 chunk with contiguous receive buffer"
    );
}

#[test]
fn receive_multiple_chunks_and_finishing_test() {
    let mut test_env = setup_receive_only_test_env();

    // feed enough data to create multiple slots in the receive buffer
    const AMOUNT: usize = 4000;

    test_env.feed_data(VarInt::from_u8(0), AMOUNT);

    assert!(
        test_env.poll_pop().is_ready(),
        "response should return 1 chunk with contiguous receive buffer"
    );

    test_env.feed_data(VarInt::try_from(AMOUNT).unwrap(), AMOUNT);
    assert!(test_env
        .stream
        .on_data(
            &stream_data(
                test_env.stream.stream_id,
                VarInt::try_from(AMOUNT * 2).unwrap(),
                &[],
                true
            ),
            &mut StreamEvents::new(),
        )
        .is_ok());

    assert_eq!(
        test_env.run_request(ops::Request::default().receive(&mut []), false),
        Ok(ops::Response {
            rx: Some(ops::rx::Response {
                bytes: ops::Bytes {
                    available: AMOUNT,
                    consumed: 0,
                },
                chunks: ops::Chunks {
                    available: 2,
                    consumed: 0,
                },
                status: ops::Status::Finishing,
                ..Default::default()
            }),
            ..Default::default()
        }),
        "response should indicate stream is finishing"
    );

    assert_eq!(
        test_env.run_request(
            ops::Request::default().receive(&mut [Bytes::new(), Bytes::new(), Bytes::new()]),
            false
        ),
        Ok(ops::Response {
            rx: Some(ops::rx::Response {
                bytes: ops::Bytes {
                    available: 0,
                    consumed: AMOUNT,
                },
                chunks: ops::Chunks {
                    available: 0,
                    consumed: 2,
                },
                will_wake: false,
                status: ops::Status::Finished,
            }),
            ..Default::default()
        }),
        "response should indicate stream is finished"
    );
}

#[test]
fn receive_low_watermark_test() {
    let mut test_env = setup_receive_only_test_env();

    assert_eq!(
        test_env.poll_request(
            ops::Request::default()
                .receive(&mut [Bytes::new(), Bytes::new()])
                .with_low_watermark(100)
        ),
        Poll::Pending,
        "polling with a low watermark and empty buffer should return pending"
    );

    test_env.feed_data(VarInt::from_u8(0), 50);
    assert_eq!(
        test_env.wake_counter, 0,
        "receiving data under the low watermark should not wake"
    );

    test_env.feed_data(VarInt::from_u8(50), 50);
    assert_eq!(
        test_env.wake_counter, 1,
        "receiving data beyond the low watermark should wake"
    );

    assert_eq!(
        test_env.poll_request(
            ops::Request::default()
                .receive(&mut [Bytes::new(), Bytes::new()])
                .with_low_watermark(100)
        ),
        Poll::Ready(Ok(ops::Response {
            rx: Some(ops::rx::Response {
                bytes: ops::Bytes {
                    available: 0,
                    consumed: 100,
                },
                chunks: ops::Chunks {
                    available: 0,
                    consumed: 1,
                },
                will_wake: false,
                status: ops::Status::Open,
            }),
            ..Default::default()
        })),
        "polling with a low watermark and empty buffer should return pending"
    );
}

#[test]
fn receive_low_watermark_with_data_test() {
    let mut test_env = setup_receive_only_test_env();

    test_env.feed_data(VarInt::from_u8(0), 50);
    assert_eq!(
        test_env.wake_counter, 0,
        "receiving data under the low watermark should not wake"
    );

    assert_eq!(
        test_env.poll_request(
            ops::Request::default()
                .receive(&mut [Bytes::new(), Bytes::new()])
                .with_low_watermark(100)
        ),
        Poll::Pending,
        "polling with a low watermark and partial buffer should return pending"
    );

    test_env.feed_data(VarInt::from_u8(50), 50);
    assert_eq!(
        test_env.wake_counter, 1,
        "receiving data beyond the low watermark should wake"
    );

    assert_eq!(
        test_env.poll_request(
            ops::Request::default()
                .receive(&mut [Bytes::new(), Bytes::new()])
                .with_low_watermark(100)
        ),
        Poll::Ready(Ok(ops::Response {
            rx: Some(ops::rx::Response {
                bytes: ops::Bytes {
                    available: 0,
                    consumed: 100,
                },
                chunks: ops::Chunks {
                    available: 0,
                    consumed: 1,
                },
                will_wake: false,
                status: ops::Status::Open,
            }),
            ..Default::default()
        })),
        "polling with a low watermark and full buffer should consume the chunks"
    );
}

#[test]
fn receive_high_watermark_test() {
    let mut test_env = setup_receive_only_test_env();

    assert_eq!(
        test_env.poll_request(
            ops::Request::default()
                .receive(&mut [Bytes::new(), Bytes::new()])
                .with_high_watermark(10)
        ),
        Poll::Pending,
        "polling with a high watermark and empty buffer should return pending"
    );

    test_env.feed_data(VarInt::from_u8(0), 20);

    assert_eq!(
        test_env.poll_request(
            ops::Request::default()
                .receive(&mut [Bytes::new(), Bytes::new()])
                .with_high_watermark(10)
        ),
        Poll::Ready(Ok(ops::Response {
            rx: Some(ops::rx::Response {
                bytes: ops::Bytes {
                    available: 10,
                    consumed: 10,
                },
                chunks: ops::Chunks {
                    available: 1,
                    consumed: 1,
                },
                will_wake: false,
                status: ops::Status::Open,
            }),
            ..Default::default()
        })),
        "polling with a high watermark should return a partial chunk"
    );

    assert_eq!(
        test_env.poll_request(
            ops::Request::default()
                .receive(&mut [Bytes::new(), Bytes::new()])
                .with_high_watermark(15)
        ),
        Poll::Ready(Ok(ops::Response {
            rx: Some(ops::rx::Response {
                bytes: ops::Bytes {
                    available: 0,
                    consumed: 10,
                },
                chunks: ops::Chunks {
                    available: 0,
                    consumed: 1,
                },
                will_wake: false,
                status: ops::Status::Open,
            }),
            ..Default::default()
        })),
        "polling with a higher watermark than available should return the rest"
    );
}

#[test]
fn receiving_into_non_empty_buffers_returns_an_error() {
    let mut test_env = setup_receive_only_test_env();

    test_env.feed_data(VarInt::from_u8(0), 32);

    assert_matches!(
        test_env.poll_request(ops::Request::default().receive(&mut [Bytes::from(&[1][..])])),
        Poll::Ready(Err(StreamError::NonEmptyOutput { .. })),
    );

    assert_matches!(
        test_env.poll_request(
            ops::Request::default().receive(&mut [Bytes::new(), Bytes::from(&[1][..])])
        ),
        Poll::Ready(Err(StreamError::NonEmptyOutput { .. })),
    );

    assert_eq!(
        test_env.consume_all_data(),
        32,
        "data should not be lost when returning an error"
    );
}
