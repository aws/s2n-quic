// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::stream::{
    stream_interests::{StreamInterestProvider, StreamInterests},
    testing::*,
    StreamError, StreamEvents, StreamTrait,
};
use bytes::Bytes;
use core::task::Poll;
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    application::Error as ApplicationErrorCode,
    connection, endpoint,
    frame::{Frame, MaxData, MaxStreamData, StopSending},
    packet::number::PacketNumber,
    stream::{ops, StreamType},
    transmission,
    varint::{VarInt, MAX_VARINT_VALUE},
};

/// Sets up a Test environment for Streams where only the sending half of
/// the Stream is open
fn setup_send_only_test_env() -> TestEnvironment {
    let test_env_config = TestEnvironmentConfig {
        stream_id: StreamId::initial(endpoint::Type::Server, StreamType::Unidirectional),
        ..Default::default()
    };
    setup_stream_test_env_with_config(test_env_config)
}

#[test]
fn remotely_initiated_unidirectional_stream_can_not_be_sent_to() {
    for local_endpoint_type in &[endpoint::Type::Client, endpoint::Type::Server] {
        let initiator_endpoint_type = if *local_endpoint_type == endpoint::Type::Client {
            endpoint::Type::Server
        } else {
            endpoint::Type::Client
        };

        let test_env_config = TestEnvironmentConfig {
            local_endpoint_type: *local_endpoint_type,
            stream_id: StreamId::initial(initiator_endpoint_type, StreamType::Unidirectional),
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        let data = Bytes::from_static(b"1");
        assert_matches!(
            test_env.poll_push(data),
            Poll::Ready(Err(StreamError::SendAfterFinish { .. })),
        );
    }
}

#[test]
fn bidirectional_and_locally_initiated_unidirectional_streams_can_be_written_to() {
    for local_endpoint_type in &[endpoint::Type::Client, endpoint::Type::Server] {
        for stream_type in &[StreamType::Unidirectional, StreamType::Bidirectional] {
            for initiator in &[endpoint::Type::Client, endpoint::Type::Server] {
                // Skip remotely initiated unidirectional stream
                if *stream_type == StreamType::Unidirectional && *initiator != *local_endpoint_type
                {
                    continue;
                }

                let test_env_config = TestEnvironmentConfig {
                    local_endpoint_type: *local_endpoint_type,
                    stream_id: StreamId::initial(*initiator, *stream_type),
                    ..Default::default()
                };
                let mut test_env = setup_stream_test_env_with_config(test_env_config);

                let data = Bytes::from_static(b"a");
                assert_eq!(test_env.poll_push(data), Poll::Ready(Ok(())));
            }
        }
    }
}

#[derive(Debug)]
enum Instruction {
    /// Enqueues data via `poll_push()` of the given size at the given offset.
    /// The boolean parameter denotes whether this is expected to succeed.
    EnqueueData(VarInt, usize, bool),
    /// Calls `poll_finish()` on the Stream.
    /// The boolean parameter denotes whether this is expected to succeed.
    Finish(bool),
    /// Initiates a `RESET` with the given error code
    Reset(ApplicationErrorCode, bool),
    /// Ingest a `MAX_DATA` frame which indicates the given window.
    SetMaxData(VarInt),
    /// Ingest a `MAX_STREAM_DATA` frame which indicates the given window.
    SetMaxStreamData(VarInt, ExpectWakeup),
    /// Ingest a `STOP_SENDING` frame
    StopSending(ApplicationErrorCode, ExpectWakeup),
    /// Checks that a stream data frame with the given parameters had been
    /// transmitted.
    CheckDataTx(
        VarInt, // offset
        usize,  // size
        bool,   // eof
        bool,   // is last frame
        PacketNumber,
    ),
    /// Checks that a stream data blocked frame with the given parameters had been
    /// transmitted.
    CheckStreamDataBlockedTx(
        VarInt, // stream data limit
        PacketNumber,
    ),
    /// Checks whether a reset frame is transmitted
    CheckResetTx(ApplicationErrorCode, PacketNumber, VarInt),
    /// Checks whether a Stream is interested in the given interactions
    CheckInterests(StreamInterests),
    /// Checks that no outgoing data is written in an `on_transmit` call and
    /// that no frames are still queued.
    CheckNoTx,
    /// Acknowledges a packet with a given packet number as received
    AckPacket(PacketNumber, ExpectWakeup),
    /// Declares a packet with a given packet number as lost
    NackPacket(PacketNumber),
}

fn execute_instructions(test_env: &mut TestEnvironment, instructions: &[Instruction]) {
    println!("executing {} instructions", instructions.len());
    for (id, instruction) in instructions.iter().enumerate() {
        println!("Executing instruction {id:?} {instruction:?}");
        match instruction {
            Instruction::EnqueueData(offset, size, expect_success) => {
                let data = Bytes::from(gen_pattern_test_data(*offset, *size));

                let poll_result = test_env.poll_push(data);

                if *expect_success {
                    assert_eq!(poll_result, Poll::Ready(Ok(())));
                } else {
                    assert_eq!(poll_result, Poll::Pending);
                }
            }
            Instruction::Finish(expect_success) => {
                let poll_result = test_env.poll_finish();

                if *expect_success {
                    assert_eq!(poll_result, Poll::Ready(Ok(())));
                } else {
                    assert_eq!(poll_result, Poll::Pending);
                }
            }
            Instruction::Reset(error_code, expect_success) => {
                let result = test_env
                    .stream
                    .poll_request(ops::Request::default().reset(*error_code), None);

                assert_eq!(*expect_success, result.is_ok(), "Unexpected reset result");
            }
            Instruction::SetMaxData(max_data) => {
                let was_waiting_for_connection_window = test_env
                    .stream
                    .get_stream_interests()
                    .connection_flow_control_credits;
                test_env.tx_connection_flow_controller.on_max_data(MaxData {
                    maximum_data: *max_data,
                });

                if was_waiting_for_connection_window {
                    test_env.stream.on_connection_window_available();
                }
            }
            Instruction::SetMaxStreamData(max_stream_data, expect_writer_wakeup) => {
                let mut events = StreamEvents::new();
                assert!(test_env
                    .stream
                    .on_max_stream_data(
                        &MaxStreamData {
                            stream_id: test_env.stream.stream_id.into(),
                            maximum_stream_data: *max_stream_data,
                        },
                        &mut events
                    )
                    .is_ok());
                assert!(events.read_wake.is_none());
                if let ExpectWakeup(Some(wakeup_expected)) = expect_writer_wakeup {
                    assert_eq!(*wakeup_expected, events.write_wake.is_some());
                }
            }
            Instruction::StopSending(error_code, expect_writer_wakeup) => {
                let mut events = StreamEvents::new();
                assert!(test_env
                    .stream
                    .on_stop_sending(
                        &StopSending {
                            stream_id: test_env.stream.stream_id.into(),
                            application_error_code: (*error_code).into(),
                        },
                        &mut events,
                    )
                    .is_ok());
                if let ExpectWakeup(Some(wakeup_expected)) = expect_writer_wakeup {
                    assert_eq!(*wakeup_expected, events.write_wake.is_some());
                }
            }
            Instruction::CheckNoTx => {
                test_env.sent_frames.clear();
                test_env.assert_write_frames(0);
                assert_eq!(
                    test_env.sent_frames.len(),
                    0,
                    "Expected no queued frames {:?}",
                    &test_env.sent_frames
                );
            }
            Instruction::CheckDataTx(
                expected_offset,
                expected_size,
                expect_eof,
                expect_is_last_frame,
                expected_packet_number,
            ) => {
                test_env.assert_write_of(
                    *expected_offset,
                    *expected_size,
                    *expect_eof,
                    *expect_is_last_frame,
                    *expected_packet_number,
                );
            }
            Instruction::CheckStreamDataBlockedTx(stream_data_limit, expected_packet_number) => {
                test_env.assert_write_stream_data_blocked_frame(
                    *stream_data_limit,
                    *expected_packet_number,
                )
            }
            Instruction::CheckResetTx(
                expected_error_code,
                expected_packet_number,
                expected_final_size,
            ) => {
                test_env.assert_write_reset_frame(
                    *expected_error_code,
                    *expected_packet_number,
                    *expected_final_size,
                );
            }
            Instruction::CheckInterests(expected_interests) => {
                assert_eq!(*expected_interests, test_env.stream.get_stream_interests());
            }
            Instruction::AckPacket(packet_number, expect_writer_wakeup) => {
                test_env.ack_packet(*packet_number, *expect_writer_wakeup);
            }
            Instruction::NackPacket(packet_number) => {
                test_env.nack_packet(*packet_number);
            }
        }
    }
}

#[test]
fn sent_data_gets_enqueued_as_frames() {
    let mut test_env = setup_send_only_test_env();

    let data1 = Bytes::from_static(b"123");
    let data2 = Bytes::from_static(b"456");
    let data3 = Bytes::from_static(b"789");

    assert_eq!(test_env.poll_push(data1.clone()), Poll::Ready(Ok(())),);
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

    assert_eq!(test_env.poll_push(data2.clone()), Poll::Ready(Ok(())),);
    assert_eq!(
        stream_interests(&["tx"]),
        test_env.stream.get_stream_interests()
    );

    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::Stream(stream_data(
            test_env.stream.stream_id,
            VarInt::from_u32(0),
            DecoderBufferMut::new(
                &mut data1
                    .iter()
                    .chain(data2.iter())
                    .copied()
                    .collect::<Vec<_>>()[..]
            ),
            false
        )),
        sent_frame.as_frame()
    );

    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );
    test_env.assert_write_frames(0);

    assert_eq!(test_env.poll_push(data3.clone()), Poll::Ready(Ok(())),);
    assert_eq!(
        stream_interests(&["ack", "tx"]),
        test_env.stream.get_stream_interests()
    );

    test_env.assert_write_frames(1);
    let mut sent_frame = test_env.sent_frames.pop_front().expect("Frame is written");
    assert_eq!(
        Frame::Stream(stream_data(
            test_env.stream.stream_id,
            VarInt::from_u32((data1.len() + data2.len()) as u32),
            DecoderBufferMut::new(&mut data3.to_vec()[..]),
            false
        )),
        sent_frame.as_frame()
    );
    assert_eq!(
        stream_interests(&["ack"]),
        test_env.stream.get_stream_interests()
    );
}

#[test]
fn can_not_enqueue_data_if_max_buffer_size_has_been_reached() {
    const MAX_BUFFER_SIZE: usize = 1024;
    let max_buffer_size_varint = VarInt::from_u32(MAX_BUFFER_SIZE as u32);
    const RECEIVE_WINDOW: u64 = 8 * 1024;

    let test_configs = &[
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), MAX_BUFFER_SIZE, true),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
            // Transmitting data does not increase the window
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), MAX_BUFFER_SIZE, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
        ][..],
        &[
            // We can actually write more data then the maximum buffer size.
            // We can just not write beyond this.
            Instruction::EnqueueData(VarInt::from_u32(0), 128, true),
            Instruction::EnqueueData(VarInt::from_u32(128), MAX_BUFFER_SIZE, true),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
            // Transmitting data does not increase the window
            Instruction::CheckDataTx(VarInt::from_u32(0), 1152, false, false, pn(0)),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
        ][..],
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), MAX_BUFFER_SIZE - 200, true),
            Instruction::EnqueueData(max_buffer_size_varint - 200, 150, true),
            Instruction::EnqueueData(max_buffer_size_varint - 50, 50, true),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
        ][..],
        &[
            // Checks that we can enqueue more than the maximum buffer size,
            // as long as we are below the flow control window. We subtract 2
            // in the second instruction so that even the the third instruction
            // would not cause us a rejection based on the flow control size.
            Instruction::EnqueueData(VarInt::from_u32(0), MAX_BUFFER_SIZE - 1, true),
            Instruction::EnqueueData(
                max_buffer_size_varint - 1,
                RECEIVE_WINDOW as usize - MAX_BUFFER_SIZE - 2,
                true,
            ),
            Instruction::EnqueueData(VarInt::from_u32(RECEIVE_WINDOW as u32) - 1, 1, false),
        ][..],
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), MAX_BUFFER_SIZE, true),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), MAX_BUFFER_SIZE, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            // Now we should have MAX_BUFFER_SIZE available again
            Instruction::EnqueueData(max_buffer_size_varint, 512, true),
            Instruction::EnqueueData(max_buffer_size_varint + 512, 2 * MAX_BUFFER_SIZE, true),
            Instruction::EnqueueData(max_buffer_size_varint * 3 + 512, 1, false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // Transmit and ACK data. There should still be no buffer space
            Instruction::CheckDataTx(max_buffer_size_varint, 1196, false, true, pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::EnqueueData(max_buffer_size_varint * 3 + 512, 1, false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // After acknowledging the big chunk, we have capacity again
            Instruction::CheckDataTx(max_buffer_size_varint + 1196, 1196, false, true, pn(2)),
            Instruction::CheckDataTx(max_buffer_size_varint + 1196 * 2, 168, false, false, pn(3)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(2), ExpectWakeup(Some(true))),
            Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::EnqueueData(max_buffer_size_varint * 3 + 512, 1, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
        ][..],
    ];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: MAX_BUFFER_SIZE,
            initial_send_window: RECEIVE_WINDOW,
            max_packet_size: Some(1200),
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn zero_sized_buffers_can_always_be_enqueued() {
    const MAX_BUFFER_SIZE: usize = 1024;

    let test_configs = &[
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), 2 * MAX_BUFFER_SIZE, true),
            Instruction::EnqueueData(
                VarInt::from_u32(2 * MAX_BUFFER_SIZE as u32),
                2 * MAX_BUFFER_SIZE,
                false,
            ),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::EnqueueData(VarInt::from_u32(2 * MAX_BUFFER_SIZE as u32), 0, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
        ][..],
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), 0, true),
            Instruction::CheckInterests(stream_interests(&[])),
        ][..],
    ];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: MAX_BUFFER_SIZE,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn multiple_stream_frames_are_sent_in_a_packet() {
    const MAX_BUFFER_SIZE: usize = 1024;
    let max_buffer_size_varint = VarInt::from_u32(MAX_BUFFER_SIZE as u32);
    const RECEIVE_WINDOW: u64 = 8 * 1024;

    let test_configs = &[
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), MAX_BUFFER_SIZE, true),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // Transmitting data does not increase the window
            Instruction::CheckDataTx(VarInt::from_u32(0), MAX_BUFFER_SIZE, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
        ][..],
        &[
            // We can actually write more data then the maximum buffer size.
            // We can just not write beyond this.
            Instruction::EnqueueData(VarInt::from_u32(0), 128, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::EnqueueData(VarInt::from_u32(128), MAX_BUFFER_SIZE, true),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
            // Transmitting data does not increase the window
            Instruction::CheckDataTx(VarInt::from_u32(0), 1152, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
        ][..],
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), MAX_BUFFER_SIZE - 200, true),
            Instruction::EnqueueData(max_buffer_size_varint - 200, 150, true),
            Instruction::EnqueueData(max_buffer_size_varint - 50, 50, true),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
        ][..],
        &[
            // Checks that we can enqueue more than the maximum buffer size,
            // as long as we are below the flow control window. We subtract 2
            // in the second instruction so that even the the third instruction
            // would not cause us a rejection based on the flow control size.
            Instruction::EnqueueData(VarInt::from_u32(0), MAX_BUFFER_SIZE - 1, true),
            Instruction::EnqueueData(
                max_buffer_size_varint - 1,
                RECEIVE_WINDOW as usize - MAX_BUFFER_SIZE - 2,
                true,
            ),
            Instruction::EnqueueData(VarInt::from_u32(RECEIVE_WINDOW as u32) - 1, 1, false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
        ][..],
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), MAX_BUFFER_SIZE, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::EnqueueData(max_buffer_size_varint, 1, false),
            Instruction::CheckDataTx(VarInt::from_u32(0), MAX_BUFFER_SIZE, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            // Now we should have MAX_BUFFER_SIZE available again
            Instruction::EnqueueData(max_buffer_size_varint, 512, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::EnqueueData(max_buffer_size_varint + 512, 2 * MAX_BUFFER_SIZE, true),
            Instruction::EnqueueData(max_buffer_size_varint * 3 + 512, 1, false),
            // Transmit and ACK data. There should still be no buffer space
            Instruction::CheckDataTx(max_buffer_size_varint, 1196, false, true, pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::EnqueueData(max_buffer_size_varint * 3 + 512, 1, false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // After acknowledging the big chunk, we have capacity again
            Instruction::CheckDataTx(max_buffer_size_varint + 1196, 1196, false, true, pn(2)),
            Instruction::CheckDataTx(max_buffer_size_varint + 1196 * 2, 168, false, false, pn(3)),
            Instruction::AckPacket(pn(2), ExpectWakeup(Some(true))),
            Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::EnqueueData(max_buffer_size_varint * 3 + 512, 1, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
        ][..],
    ];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: MAX_BUFFER_SIZE,
            initial_send_window: RECEIVE_WINDOW,
            max_packet_size: Some(1200),
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn bigger_data_is_split_across_packets() {
    const MAX_PACKET_SIZE: usize = 1024;
    const TOTAL_WRITE_SIZE: usize = 4 * MAX_PACKET_SIZE + 31;

    let test_configs = &[
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), TOTAL_WRITE_SIZE, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 1022, false, true, pn(0)),
            // The next frames need a 2 byte stream ID + 2 byte offset
            Instruction::CheckDataTx(VarInt::from_u32(1022), 1020, false, true, pn(1)),
            Instruction::CheckDataTx(VarInt::from_u32(2042), 1020, false, true, pn(2)),
            Instruction::CheckDataTx(VarInt::from_u32(3062), 1020, false, true, pn(3)),
            Instruction::CheckDataTx(
                VarInt::from_u32(4082),
                TOTAL_WRITE_SIZE - 4082,
                false,
                false,
                pn(4),
            ),
            Instruction::CheckInterests(stream_interests(&["ack"])),
        ][..],
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), 512, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::EnqueueData(VarInt::from_u32(512), TOTAL_WRITE_SIZE, true),
            Instruction::CheckDataTx(VarInt::from_u32(0), 1022, false, true, pn(0)),
            // The next frames need a 2 byte stream ID + 2 byte offset
            Instruction::CheckDataTx(VarInt::from_u32(1022), 1020, false, true, pn(1)),
            Instruction::CheckDataTx(VarInt::from_u32(2042), 1020, false, true, pn(2)),
            Instruction::CheckDataTx(VarInt::from_u32(3062), 1020, false, true, pn(3)),
            Instruction::CheckDataTx(
                VarInt::from_u32(4082),
                512 + TOTAL_WRITE_SIZE - 4082,
                false,
                false,
                pn(4),
            ),
            Instruction::CheckInterests(stream_interests(&["ack"])),
        ][..],
    ];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: 16 * 1024,
            initial_send_window: 16 * 1024,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);
        test_env
            .sent_frames
            .set_max_packet_size(Some(MAX_PACKET_SIZE));

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn lost_data_is_retransmitted() {
    const MAX_PACKET_SIZE: usize = 1000;

    let test_configs = &[
        &[
            // These go all into the first packet, which will be OK
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::EnqueueData(VarInt::from_u32(500), 400, true),
            Instruction::CheckDataTx(VarInt::from_u32(0), 900, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            // These go into the next packet, which will get lost
            Instruction::EnqueueData(VarInt::from_u32(900), 700, true),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::EnqueueData(VarInt::from_u32(1600), 200, true),
            Instruction::CheckDataTx(VarInt::from_u32(900), 900, false, false, pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            // The next packet will be good again
            Instruction::EnqueueData(VarInt::from_u32(1800), 900, true),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(1800), 900, false, false, pn(2)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::CheckNoTx,
            // Declare the packet as lost.
            // We now expect a retransmission of those segments
            Instruction::NackPacket(pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
            Instruction::CheckDataTx(VarInt::from_u32(900), 900, false, false, pn(3)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(2), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::CheckNoTx,
        ][..],
        &[
            // Split initial range across various packets, in or to check that
            // only required ranges are retransmitted.
            // Requires 504 bytes
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // Requires 406 bytes. Remaining: 90
            Instruction::EnqueueData(VarInt::from_u32(500), 400, true),
            // 86 bytes can be transmitted in the first packet. 2214 left
            // 996 bytes can be transmitted in the second packet. 1218 left
            // 996 bytes can be transmitted in the third packet. 222 left
            // 228 bytes required in the forth packet. 772 remaining for other frames
            Instruction::EnqueueData(VarInt::from_u32(900), 2300, true),
            // 768 bytes transmitted in the forth packet. 232 left
            // 238 bytes required in the fifth packet.
            Instruction::EnqueueData(VarInt::from_u32(3200), 1000, true),
            // 106 bytes required in the fifth packet.
            Instruction::EnqueueData(VarInt::from_u32(4200), 100, true),
            Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(1)),
            Instruction::CheckDataTx(VarInt::from_u32(1994), 996, false, true, pn(2)),
            Instruction::CheckDataTx(VarInt::from_u32(2990), 996, false, true, pn(3)),
            Instruction::CheckDataTx(VarInt::from_u32(3986), 314, false, false, pn(4)),
            // Packet 1 and Packet 3 get lost
            Instruction::NackPacket(pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
            Instruction::NackPacket(pn(3)),
            Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
            Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(5)),
            Instruction::CheckDataTx(VarInt::from_u32(2990), 996, false, true, pn(6)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(2), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(4), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(5), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(6), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::AckPacket(pn(6), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::CheckNoTx,
        ][..],
    ];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: 10 * MAX_PACKET_SIZE,
            initial_send_window: 10 * MAX_PACKET_SIZE as u64,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);
        test_env
            .sent_frames
            .set_max_packet_size(Some(MAX_PACKET_SIZE));

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn can_not_transmit_data_when_congestion_limited() {
    const MAX_PACKET_SIZE: usize = 1000;

    let test_config = &[
        Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        // Verify data is not transmitted
        Instruction::CheckNoTx,
        Instruction::Reset(ApplicationErrorCode::new(1).unwrap(), true),
        // Verify reset is not synced
        Instruction::CheckNoTx,
    ];

    let test_env_config = TestEnvironmentConfig {
        transmission_constraint: transmission::Constraint::CongestionLimited,
        ..Default::default()
    };
    let mut test_env = setup_stream_test_env_with_config(test_env_config);
    test_env
        .sent_frames
        .set_max_packet_size(Some(MAX_PACKET_SIZE));

    execute_instructions(&mut test_env, test_config);
}

#[test]
fn only_lost_data_is_sent_when_constrained_to_retransmission_only() {
    const MAX_PACKET_SIZE: usize = 1000;

    let test_config = &[
        // Send 800 bytes in a packet that will be lost
        Instruction::EnqueueData(VarInt::from_u32(0), 800, true),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        Instruction::CheckDataTx(VarInt::from_u32(0), 800, false, false, pn(0)),
        Instruction::CheckInterests(stream_interests(&["ack"])),
        Instruction::NackPacket(pn(0)),
        Instruction::CheckInterests(stream_interests(&["lost"])),
    ];

    let test_env_config: TestEnvironmentConfig = Default::default();
    let mut test_env = setup_stream_test_env_with_config(test_env_config);
    test_env
        .sent_frames
        .set_max_packet_size(Some(MAX_PACKET_SIZE));
    execute_instructions(&mut test_env, test_config);

    let test_config = &[
        // Enqueue 900 bytes of new data
        Instruction::EnqueueData(VarInt::from_u32(0), 900, true),
        Instruction::CheckInterests(stream_interests(&["tx", "lost"])),
        // Verify that only the lost data was sent
        Instruction::CheckDataTx(VarInt::from_u32(0), 800, false, false, pn(1)),
        Instruction::CheckNoTx,
        // Verify we still want to transmit the new data
        Instruction::CheckInterests(stream_interests(&["tx", "ack"])),
    ];

    test_env.transmission_constraint = transmission::Constraint::RetransmissionOnly;
    execute_instructions(&mut test_env, test_config);
}

#[test]
fn only_lost_reset_is_sent_when_constrained_to_retransmission_only() {
    const MAX_PACKET_SIZE: usize = 1000;
    let error_code = ApplicationErrorCode::new(1).unwrap();

    let test_config = &[
        // Send a reset that will be lost
        Instruction::Reset(error_code, true),
        Instruction::CheckResetTx(error_code, pn(0), VarInt::from_u32(0)),
        Instruction::NackPacket(pn(0)),
    ];

    let test_env_config: TestEnvironmentConfig = Default::default();
    let mut test_env = setup_stream_test_env_with_config(test_env_config);
    test_env
        .sent_frames
        .set_max_packet_size(Some(MAX_PACKET_SIZE));
    execute_instructions(&mut test_env, test_config);

    let test_config = &[
        // Verify the lost reset was sent
        Instruction::CheckResetTx(error_code, pn(1), VarInt::from_u32(0)),
        // Verify a new reset cannot be sent
        Instruction::Reset(error_code, true),
        Instruction::CheckNoTx,
    ];

    test_env.transmission_constraint = transmission::Constraint::RetransmissionOnly;
    execute_instructions(&mut test_env, test_config);
}

#[test]
fn retransmitted_data_is_sent_in_same_packets_as_new_data() {
    const MAX_PACKET_SIZE: usize = 1000;

    let test_configs = &[
        &[
            // Enqueue data which gets sent in one packet, but doesn't take up
            // all it's space.
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            // Prepare more data
            Instruction::EnqueueData(VarInt::from_u32(500), 1000, true),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::NackPacket(pn(0)),
            Instruction::CheckInterests(stream_interests(&["lost"])),
            // Transmit after packet loss had been announced.
            // The retransmit requires 504 bytes. 496 are left. 492 can be
            // used for content
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(1)),
            Instruction::CheckDataTx(VarInt::from_u32(500), 492, false, true, pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(2), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::CheckNoTx,
        ][..],
        &[
            // Enqueue data which gets sent in one packet, but doesn't take up
            // all it's space. In the retransmission the space will be used for
            // other data.
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            // Send 2 chunks. One which will span multiple frames
            Instruction::EnqueueData(VarInt::from_u32(500), 500, true),
            Instruction::EnqueueData(VarInt::from_u32(1000), 1000, true),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            // Requires 506 bytes
            Instruction::CheckDataTx(VarInt::from_u32(500), 996, false, true, pn(1)),
            Instruction::CheckDataTx(VarInt::from_u32(1496), 504, false, false, pn(2)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::NackPacket(pn(0)),
            Instruction::NackPacket(pn(2)),
            Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
            Instruction::EnqueueData(VarInt::from_u32(2000), 10, true),
            Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
            // Retransmission of the first chunk requires 504 bytes. 496 left
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(3)),
            Instruction::CheckDataTx(VarInt::from_u32(1496), 492, false, true, pn(3)),
            Instruction::CheckDataTx(VarInt::from_u32(1988), 12, false, false, pn(4)),
            Instruction::CheckDataTx(VarInt::from_u32(2000), 10, false, false, pn(4)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(4), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&[])),
        ][..],
    ];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: 10 * MAX_PACKET_SIZE,
            initial_send_window: 10 * MAX_PACKET_SIZE as u64,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);
        test_env
            .sent_frames
            .set_max_packet_size(Some(MAX_PACKET_SIZE));

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
//= type=test
//# A sender SHOULD send a
//# STREAM_DATA_BLOCKED or DATA_BLOCKED frame to indicate to the receiver
//# that it has data to write but is blocked by flow control limits.

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.13
//= type=test
//# A sender SHOULD send a STREAM_DATA_BLOCKED frame (type=0x15) when it
//# wishes to send data, but is unable to do so due to stream-level flow
//# control.
#[test]
fn writes_not_more_than_max_stream_data_even_if_more_data_is_enqueued() {
    const MAX_PACKET_SIZE: usize = 1000;
    const MAX_BUFFER_SIZE: usize = 1500;
    const WINDOW_SIZE: usize = 2000;

    let test_configs = &[&[
        // We are able to enqueue data which is bigger than the window
        Instruction::EnqueueData(VarInt::from_u32(0), 2500, true),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        // Try to enqueue some data so we can observe wakeups
        Instruction::EnqueueData(VarInt::from_u32(2500), 10, false),
        Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
        Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(1)),
        Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(2)),
        Instruction::CheckStreamDataBlockedTx(VarInt::from_u32(2000), pn(2)),
        Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
        // Acking the packets does not unblock sending more data
        Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
        Instruction::AckPacket(pn(1), ExpectWakeup(Some(true))),
        Instruction::AckPacket(pn(2), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["sf"])),
        Instruction::CheckNoTx,
        // Sending a window update should unblock sending more data.
        // Since we still have enqueued more data than the new flow control window,
        // we expect no wakeup here.
        Instruction::SetMaxStreamData(VarInt::from_u32(2100), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        Instruction::CheckDataTx(VarInt::from_u32(2000), 100, false, false, pn(3)),
        Instruction::CheckStreamDataBlockedTx(VarInt::from_u32(2100), pn(3)),
        Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
        Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["sf"])),
        Instruction::CheckNoTx,
        Instruction::SetMaxStreamData(VarInt::from_u32(2499), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        Instruction::CheckDataTx(VarInt::from_u32(2100), 399, false, false, pn(4)),
        Instruction::CheckStreamDataBlockedTx(VarInt::from_u32(2499), pn(4)),
        Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
        Instruction::AckPacket(pn(4), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["sf"])),
        Instruction::CheckNoTx,
        Instruction::SetMaxStreamData(VarInt::from_u32(2499), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["sf"])),
        Instruction::CheckNoTx,
        Instruction::SetMaxStreamData(VarInt::from_u32(2000), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["sf"])),
        Instruction::CheckNoTx,
        Instruction::SetMaxStreamData(VarInt::from_u32(2500), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        Instruction::CheckDataTx(VarInt::from_u32(2499), 1, false, false, pn(5)),
        Instruction::CheckInterests(stream_interests(&["ack"])),
        Instruction::AckPacket(pn(5), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&[])),
        Instruction::CheckNoTx,
        Instruction::SetMaxStreamData(VarInt::from_u32(2501), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&[])),
    ][..]];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: MAX_BUFFER_SIZE,
            initial_send_window: WINDOW_SIZE as u64,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);
        test_env
            .sent_frames
            .set_max_packet_size(Some(MAX_PACKET_SIZE));

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn blocked_on_stream_flow_control_does_not_prevent_retransmissions() {
    const MAX_PACKET_SIZE: usize = 1000;
    // Max buffer is lower than WINDOW, in order to get blocked before
    // we reach the window
    const MAX_BUFFER_SIZE: usize = 1500;
    const WINDOW_SIZE: usize = 2000;

    let test_configs = &[&[
        // We are able to enqueue data which is bigger than the window
        Instruction::EnqueueData(VarInt::from_u32(0), 3000, true),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        // Check that we indeed can't enqueue more data
        Instruction::EnqueueData(VarInt::from_u32(2500), 10, false),
        Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
        Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(1)),
        Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(2)),
        Instruction::CheckStreamDataBlockedTx(VarInt::from_u32(2000), pn(2)),
        Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
        // Nack packets. This should lead to retransmissions
        Instruction::NackPacket(pn(1)),
        Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
        Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(3)),
        Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
        Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
        Instruction::NackPacket(pn(0)),
        Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
        Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(4)),
        Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
        Instruction::AckPacket(pn(4), ExpectWakeup(Some(true))),
        Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
        Instruction::NackPacket(pn(2)),
        Instruction::CheckInterests(stream_interests(&["lost"])),
        Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(5)),
        Instruction::CheckStreamDataBlockedTx(VarInt::from_u32(2000), pn(5)),
        Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
        Instruction::CheckNoTx,
        Instruction::AckPacket(pn(5), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["sf"])),
        Instruction::CheckNoTx,
    ][..]];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: MAX_BUFFER_SIZE,
            initial_send_window: WINDOW_SIZE as u64,
            max_packet_size: Some(MAX_PACKET_SIZE),
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn writes_not_more_than_max_data_even_if_more_data_is_enqueued() {
    const MAX_PACKET_SIZE: usize = 1000;
    const MAX_BUFFER_SIZE: usize = 1500;
    const STREAM_WINDOW_SIZE: usize = 100 * 1024; // Do not want to block on this
    const CONN_WINDOW_SIZE: usize = 2000;

    let test_configs = &[&[
        // We are able to enqueue data which is bigger than the window
        Instruction::EnqueueData(VarInt::from_u32(0), 2500, true),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        // Try to enqueue some data so we can observe wakeups
        Instruction::EnqueueData(VarInt::from_u32(2500), 10, false),
        Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
        Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(1)),
        Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(2)),
        Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
        // Acking the packets does not unblock sending more data
        Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
        Instruction::AckPacket(pn(1), ExpectWakeup(Some(true))),
        Instruction::AckPacket(pn(2), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["cf"])),
        Instruction::CheckNoTx,
        // Sending a window update should unblock sending more data.
        // Since we still have enqueued more data than the new flow control window,
        // we expect no wakeup here.
        Instruction::SetMaxData(VarInt::from_u32(2100)),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        Instruction::CheckDataTx(VarInt::from_u32(2000), 100, false, false, pn(3)),
        Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
        Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["cf"])),
        Instruction::CheckNoTx,
        Instruction::SetMaxData(VarInt::from_u32(2499)),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        Instruction::CheckDataTx(VarInt::from_u32(2100), 399, false, false, pn(4)),
        Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
        Instruction::AckPacket(pn(4), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["cf"])),
        Instruction::CheckNoTx,
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
        //= type=test
        //# A sender MUST ignore any MAX_STREAM_DATA or MAX_DATA frames that do
        //# not increase flow control limits.
        Instruction::SetMaxData(VarInt::from_u32(2499)),
        Instruction::CheckInterests(stream_interests(&["cf"])),
        Instruction::CheckNoTx,
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
        //= type=test
        //# A sender MUST ignore any MAX_STREAM_DATA or MAX_DATA frames that do
        //# not increase flow control limits.
        Instruction::SetMaxData(VarInt::from_u32(2000)),
        Instruction::CheckInterests(stream_interests(&["cf"])),
        Instruction::CheckNoTx,
        Instruction::SetMaxData(VarInt::from_u32(2500)),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        Instruction::CheckDataTx(VarInt::from_u32(2499), 1, false, false, pn(5)),
        Instruction::CheckInterests(stream_interests(&["ack"])),
        Instruction::AckPacket(pn(5), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&[])),
        Instruction::CheckNoTx,
        Instruction::SetMaxData(VarInt::from_u32(2501)),
        Instruction::CheckInterests(stream_interests(&[])),
    ][..]];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: MAX_BUFFER_SIZE,
            initial_send_window: STREAM_WINDOW_SIZE as u64,
            initial_connection_send_window_size: CONN_WINDOW_SIZE as u64,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);
        test_env
            .sent_frames
            .set_max_packet_size(Some(MAX_PACKET_SIZE));

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn blocked_on_connection_flow_control_does_not_prevent_retransmissions() {
    const MAX_PACKET_SIZE: usize = 1000;
    // Max buffer is lower than WINDOW, in order to get blocked before
    // we reach the window
    const MAX_BUFFER_SIZE: usize = 1500;
    const STREAM_WINDOW_SIZE: usize = 100 * 1024; // Do not want to block on this
    const CONN_WINDOW_SIZE: usize = 2000;

    let test_configs = &[&[
        // We are able to enqueue data which is bigger than the window
        Instruction::EnqueueData(VarInt::from_u32(0), 2500, true),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        // Check that we indeed can't enqueue more data
        Instruction::EnqueueData(VarInt::from_u32(2500), 10, false),
        Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
        Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(1)),
        Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(2)),
        Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
        // Nack packets. This should lead to retransmissions
        Instruction::NackPacket(pn(1)),
        Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
        Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(3)),
        Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
        Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
        Instruction::NackPacket(pn(0)),
        Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
        Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(4)),
        Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
        Instruction::AckPacket(pn(4), ExpectWakeup(Some(true))),
        Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
        Instruction::NackPacket(pn(2)),
        Instruction::CheckInterests(stream_interests(&["lost"])),
        Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(5)),
        Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
        Instruction::CheckNoTx,
        Instruction::AckPacket(pn(5), ExpectWakeup(Some(false))),
        Instruction::CheckInterests(stream_interests(&["cf"])),
        Instruction::CheckNoTx,
    ][..]];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: MAX_BUFFER_SIZE,
            initial_send_window: STREAM_WINDOW_SIZE as u64,
            initial_connection_send_window_size: CONN_WINDOW_SIZE as u64,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);
        test_env
            .sent_frames
            .set_max_packet_size(Some(MAX_PACKET_SIZE));

        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn can_write_up_to_max_stream_size() {
    // This is a bit hacky. We can obviously not write 2**62 bytes of data in
    // the test. Therefore we resort to manipulating some internal pointers
    let max_varint = VarInt::new(MAX_VARINT_VALUE).unwrap();

    let mut test_env = setup_send_only_test_env();
    test_env
        .stream
        .send_stream
        .data_sender
        .set_total_acknowledged_len(max_varint - 1);
    test_env
        .stream
        .send_stream
        .data_sender
        .flow_controller_mut()
        .set_max_stream_data(max_varint);

    assert_eq!(
        Poll::Ready(Ok(())),
        test_env.poll_push(Bytes::from_static(b"a"))
    );
}

#[test]
fn can_not_write_more_data_than_maximum_stream_size() {
    // This is a bit hacky. We can obviously not write 2**62 bytes of data in
    // the test. Therefore we resort to manipulating some internal pointers
    let max_varint = VarInt::new(MAX_VARINT_VALUE).unwrap();

    // This part checks what happens if we go over the limit with one big chunk
    let mut test_env = setup_send_only_test_env();
    test_env
        .stream
        .send_stream
        .data_sender
        .set_total_acknowledged_len(max_varint - 1);
    test_env
        .stream
        .send_stream
        .data_sender
        .flow_controller_mut()
        .set_max_stream_data(max_varint);

    assert_matches!(
        test_env.poll_push(Bytes::from_static(b"aa")),
        Poll::Ready(Err(StreamError::MaxStreamDataSizeExceeded { .. })),
    );

    // And this part checks what happens if are exactly at the limit before
    let mut test_env = setup_send_only_test_env();
    test_env
        .stream
        .send_stream
        .data_sender
        .set_total_acknowledged_len(max_varint - 1);
    test_env
        .stream
        .send_stream
        .data_sender
        .flow_controller_mut()
        .set_max_stream_data(max_varint);

    assert_eq!(
        Poll::Ready(Ok(())),
        test_env.poll_push(Bytes::from_static(b"a"))
    );
    assert_matches!(
        test_env.poll_push(Bytes::from_static(b"a")),
        Poll::Ready(Err(StreamError::MaxStreamDataSizeExceeded { .. })),
    );
}

#[test]
fn push_data_after_stream_is_reset_locally() {
    for reset_is_acknowledged in &[true, false] {
        let mut test_env = setup_send_only_test_env();
        let error_code = ApplicationErrorCode::new(5).unwrap();

        // Call finish and expect the frame to get emitted
        execute_instructions(
            &mut test_env,
            &[
                Instruction::Reset(error_code, true),
                Instruction::CheckInterests(stream_interests(&["tx"])),
                Instruction::CheckResetTx(error_code, pn(0), VarInt::from_u32(0)),
                Instruction::CheckInterests(stream_interests(&["ack"])),
            ],
        );

        if *reset_is_acknowledged {
            execute_instructions(
                &mut test_env,
                &[
                    Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
                    Instruction::CheckInterests(stream_interests(&["fin"])),
                ],
            );
        }

        assert_matches!(
            test_env.poll_push(Bytes::from_static(b"1")),
            Poll::Ready(Err(StreamError::StreamReset { .. })),
        );
    }
}

#[test]
fn push_data_after_finish_was_called() {
    let mut test_env = setup_send_only_test_env();

    assert_eq!(Poll::Pending, test_env.poll_finish());

    assert_matches!(
        test_env.poll_push(Bytes::from_static(b"1")),
        Poll::Ready(Err(StreamError::SendAfterFinish { .. })),
    );
}

#[test]
fn push_data_after_stream_is_reset_due_to_stop_sending() {
    for acknowledge_reset_early in &[true, false] {
        let mut test_env = setup_send_only_test_env();

        let error_code = ApplicationErrorCode::new(0x1234_5678).unwrap();

        execute_instructions(
            &mut test_env,
            &[
                Instruction::StopSending(error_code, ExpectWakeup(Some(false))),
                Instruction::CheckInterests(stream_interests(&["tx"])),
                Instruction::CheckResetTx(error_code, pn(0), VarInt::from_u32(0)),
                Instruction::CheckInterests(stream_interests(&["ack"])),
            ],
        );

        if *acknowledge_reset_early {
            test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
            assert_eq!(
                stream_interests(&[]),
                test_env.stream.get_stream_interests()
            );
        }

        // Poll two times.
        // The first call checks the branch where the user is not yet aware
        // about the reset.
        assert_matches!(
            test_env.poll_push(Bytes::from_static(b"1")),
            Poll::Ready(Err(StreamError::StreamReset { .. })),
        );

        if !*acknowledge_reset_early {
            assert_eq!(
                stream_interests(&["ack"]),
                test_env.stream.get_stream_interests()
            );
            // The user is already aware about the reset.
            // Delivering an ack now should bring us into the final state
            test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
        }
        assert_eq!(
            stream_interests(&["fin"]),
            test_env.stream.get_stream_interests()
        );

        // The second call checks whether the same result is delivered after the
        // user had been notified.
        assert_matches!(
            test_env.poll_push(Bytes::from_static(b"1")),
            Poll::Ready(Err(StreamError::StreamReset { .. })),
        );

        assert_eq!(
            stream_interests(&["fin"]),
            test_env.stream.get_stream_interests()
        );
    }
}

#[test]
fn writes_finish_packet_in_dedicated_packet() {
    let test_configs = &[
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckDataTx(VarInt::from_u32(500), 0, true, false, pn(1)),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            // Acknowledge other packet(s), but not the FIN.
            // That should not trigger a wakeup and finish to return Ready.
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
        ][..],
        &[
            // use a dedicated packet, but acknowledge everything in front of it
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(500), 0, true, false, pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
        ][..],
        &[
            // Acknowledge the FIN packet, but leave the other packet not acknowledged
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(500), 0, true, false, pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
        ][..],
    ];

    for test_config in test_configs.iter() {
        let mut test_env = setup_send_only_test_env();
        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn adds_finish_packet_in_enqueued_packet() {
    let test_configs = &[
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, true, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
        ][..],
        &[
            // Lets mark the initial packet as lost, and piggyback the FIN
            // on the retransmit
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::NackPacket(pn(0)),
            Instruction::CheckInterests(stream_interests(&["lost"])),
            Instruction::Finish(false),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, true, false, pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
        ][..],
    ];

    for test_config in test_configs.iter() {
        let mut test_env = setup_send_only_test_env();
        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn finish_without_data() {
    let test_configs = &[&[
        Instruction::Finish(false),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        Instruction::CheckDataTx(VarInt::from_u32(0), 0, true, false, pn(0)),
        Instruction::CheckInterests(stream_interests(&["ack"])),
        Instruction::Finish(false),
        Instruction::AckPacket(pn(0), ExpectWakeup(Some(true))),
        Instruction::CheckInterests(stream_interests(&[])),
        Instruction::Finish(true),
        Instruction::CheckInterests(stream_interests(&["fin"])),
    ][..]];

    for test_config in test_configs.iter() {
        let mut test_env = setup_send_only_test_env();
        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn finish_after_stream_is_finished() {
    let test_configs = &[&[
        Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
        Instruction::Finish(false),
        Instruction::CheckDataTx(VarInt::from_u32(0), 500, true, false, pn(0)),
        Instruction::CheckInterests(stream_interests(&["ack"])),
        Instruction::AckPacket(pn(0), ExpectWakeup(Some(true))),
        Instruction::CheckInterests(stream_interests(&[])),
        Instruction::Finish(true),
        Instruction::CheckInterests(stream_interests(&["fin"])),
        Instruction::Finish(true),
        Instruction::CheckInterests(stream_interests(&["fin"])),
    ][..]];

    for test_config in test_configs.iter() {
        let mut test_env = setup_send_only_test_env();
        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn transmit_fin_in_packet_which_gets_split() {
    let test_configs = &[&[
        // 1500 bytes do not fit into a single packet. That means even if
        // the FIN has been initially merged onto this packet, it needs to
        // get sent later.
        Instruction::EnqueueData(VarInt::from_u32(0), 1500, true),
        Instruction::Finish(false),
        Instruction::CheckInterests(stream_interests(&["tx"])),
        Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
        Instruction::CheckDataTx(VarInt::from_u32(998), 502, true, false, pn(1)),
        Instruction::Finish(false),
        Instruction::CheckInterests(stream_interests(&["ack"])),
        Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
        Instruction::Finish(false),
        Instruction::CheckInterests(stream_interests(&["ack"])),
        Instruction::AckPacket(pn(1), ExpectWakeup(Some(true))),
        Instruction::CheckInterests(stream_interests(&[])),
        Instruction::Finish(true),
        Instruction::CheckInterests(stream_interests(&[])),
        Instruction::CheckNoTx,
    ][..]];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            initial_send_window: 2000,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);
        test_env.sent_frames.set_max_packet_size(Some(1000));
        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn retransmit_fin_if_lost() {
    let test_configs = &[
        &[
            // Standalone FIN
            Instruction::Finish(false),
            Instruction::CheckDataTx(VarInt::from_u32(0), 0, true, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::NackPacket(pn(0)),
            Instruction::CheckInterests(stream_interests(&["lost"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 0, true, false, pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
            Instruction::CheckNoTx,
        ][..],
        &[
            // Piggybacked FIN
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, true, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::NackPacket(pn(0)),
            Instruction::CheckInterests(stream_interests(&["lost"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, true, false, pn(1)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
            Instruction::CheckNoTx,
        ][..],
        &[
            // FIN gets combined gets with another lost packet
            Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::EnqueueData(VarInt::from_u32(500), 250, true),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(500), 250, false, false, pn(1)),
            Instruction::Finish(false),
            Instruction::CheckDataTx(VarInt::from_u32(750), 0, true, false, pn(2)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::NackPacket(pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack", "lost"])),
            Instruction::NackPacket(pn(2)),
            Instruction::CheckInterests(stream_interests(&["lost"])),
            // The retransmitted chunks should all end up in one packet
            Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(3)),
            Instruction::CheckDataTx(VarInt::from_u32(750), 0, true, false, pn(3)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::CheckNoTx,
            Instruction::AckPacket(pn(3), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
            Instruction::CheckNoTx,
        ][..],
    ];

    for test_config in test_configs.iter() {
        let mut test_env = setup_send_only_test_env();
        test_env.sent_frames.set_max_packet_size(Some(1000));
        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn transmit_fin_while_outstanding_data_exceeds_stream_flow_control_window() {
    let test_configs = &[
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), 4000, true),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // Send 3 initial packets, then the flow control window limits us
            Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(1)),
            Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(2)),
            Instruction::CheckStreamDataBlockedTx(VarInt::from_u32(2000), pn(2)),
            Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
            Instruction::CheckNoTx,
            Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
            Instruction::AckPacket(pn(2), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["sf"])),
            Instruction::SetMaxStreamData(VarInt::from_u32(3000), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(2000), 996, false, true, pn(3)),
            Instruction::CheckDataTx(VarInt::from_u32(2996), 4, false, false, pn(4)),
            Instruction::CheckStreamDataBlockedTx(VarInt::from_u32(3000), pn(4)),
            Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
            // Flow control window is now big enough to get all data transmitted
            Instruction::SetMaxStreamData(VarInt::from_u32(4000), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(3000), 996, false, true, pn(5)),
            Instruction::CheckDataTx(VarInt::from_u32(3996), 4, true, false, pn(6)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(5), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(6), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(4), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
            Instruction::CheckNoTx,
        ][..],
        &[
            // This is a second iteration of the test, where the window update
            // is bigger (not equal) to the queued data.
            Instruction::EnqueueData(VarInt::from_u32(0), 3000, true),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // Send 3 initial packets, then the flow control window limits us
            Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(1)),
            Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(2)),
            Instruction::CheckStreamDataBlockedTx(VarInt::from_u32(2000), pn(2)),
            Instruction::CheckNoTx,
            Instruction::CheckInterests(stream_interests(&["ack", "sf"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(2), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["sf"])),
            Instruction::SetMaxStreamData(VarInt::from_u32(3001), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(2000), 996, false, true, pn(3)),
            Instruction::CheckDataTx(VarInt::from_u32(2996), 4, true, false, pn(4)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(4), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
            Instruction::CheckNoTx,
        ][..],
    ];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            initial_send_window: 2000,
            stream_id: StreamId::initial(endpoint::Type::Server, StreamType::Unidirectional),
            local_endpoint_type: endpoint::Type::Server,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);
        test_env.sent_frames.set_max_packet_size(Some(1000));
        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn transmit_fin_while_outstanding_data_exceeds_connection_flow_control_window() {
    let test_configs = &[
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), 4000, true),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // Send 3 initial packets, then the flow control window limits us
            Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
            Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(1)),
            Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(2)),
            Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
            Instruction::CheckNoTx,
            Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
            Instruction::AckPacket(pn(2), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["cf"])),
            Instruction::SetMaxData(VarInt::from_u32(3000)),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(2000), 996, false, true, pn(3)),
            Instruction::CheckDataTx(VarInt::from_u32(2996), 4, false, false, pn(4)),
            Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
            // Flow control window is now big enough to get all data transmitted
            Instruction::SetMaxData(VarInt::from_u32(4000)),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(3000), 996, false, true, pn(5)),
            Instruction::CheckDataTx(VarInt::from_u32(3996), 4, true, false, pn(6)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(5), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(6), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(4), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
            Instruction::CheckNoTx,
        ][..],
        &[
            // This is a second iteration of the test, where the window update
            // is bigger (not equal) to the queued data.
            Instruction::EnqueueData(VarInt::from_u32(0), 3000, true),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // Send 3 initial packets, then the flow control window limits us
            Instruction::CheckDataTx(VarInt::from_u32(0), 998, false, true, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(998), 996, false, true, pn(1)),
            Instruction::CheckDataTx(VarInt::from_u32(1994), 6, false, false, pn(2)),
            Instruction::CheckNoTx,
            Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::AckPacket(pn(2), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["cf"])),
            Instruction::SetMaxData(VarInt::from_u32(3001)),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(2000), 996, false, true, pn(3)),
            Instruction::CheckDataTx(VarInt::from_u32(2996), 4, true, false, pn(4)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(3), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::Finish(false),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            Instruction::AckPacket(pn(4), ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&[])),
            Instruction::Finish(true),
            Instruction::CheckInterests(stream_interests(&["fin"])),
            Instruction::CheckNoTx,
        ][..],
    ];

    for test_config in test_configs.iter() {
        let test_env_config = TestEnvironmentConfig {
            initial_send_window: 200 * 1024,
            initial_connection_send_window_size: 2000,
            stream_id: StreamId::initial(endpoint::Type::Server, StreamType::Unidirectional),
            local_endpoint_type: endpoint::Type::Server,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);
        test_env.sent_frames.set_max_packet_size(Some(1000));
        execute_instructions(&mut test_env, &test_config[..]);
    }
}

#[test]
fn finish_after_stream_is_reset_locally() {
    for acknowledge_reset_early in &[true, false] {
        let mut test_env = setup_send_only_test_env();

        let error_code = ApplicationErrorCode::new(5).unwrap();

        assert!(test_env.reset(error_code).is_ok());
        assert_eq!(
            stream_interests(&["tx"]),
            test_env.stream.get_stream_interests()
        );
        test_env.assert_write_reset_frame(error_code, pn(0), VarInt::from_u32(0));
        assert_eq!(
            stream_interests(&["ack"]),
            test_env.stream.get_stream_interests()
        );

        if *acknowledge_reset_early {
            test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
            assert_eq!(
                stream_interests(&["fin"]),
                test_env.stream.get_stream_interests()
            );
        }

        assert_matches!(
            test_env.poll_finish(),
            Poll::Ready(Err(StreamError::StreamReset { .. })),
        );

        if !*acknowledge_reset_early {
            // We are done after the ack, because the user is already aware
            // about the reset
            test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
        }

        assert_eq!(
            stream_interests(&["fin"]),
            test_env.stream.get_stream_interests()
        );
    }
}

#[test]
fn finish_after_stream_is_reset_due_to_stop_sending() {
    for acknowledge_reset_early in &[true, false] {
        let mut test_env = setup_send_only_test_env();

        let error_code = ApplicationErrorCode::new(0x1234_5678).unwrap();

        execute_instructions(
            &mut test_env,
            &[
                Instruction::StopSending(error_code, ExpectWakeup(Some(false))),
                Instruction::CheckInterests(stream_interests(&["tx"])),
                Instruction::CheckResetTx(error_code, pn(0), VarInt::from_u32(0)),
                Instruction::CheckInterests(stream_interests(&["ack"])),
            ],
        );

        if *acknowledge_reset_early {
            test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
            assert_eq!(
                stream_interests(&[]),
                test_env.stream.get_stream_interests()
            );
        }

        // Poll two times.
        // The first call checks the branch where the user is not yet aware
        // about the reset.
        assert_matches!(
            test_env.poll_finish(),
            Poll::Ready(Err(StreamError::StreamReset { .. })),
        );

        // Now that the user is aware the outstanding acknowledge can finalize
        // the stream
        if !*acknowledge_reset_early {
            assert_eq!(
                stream_interests(&["ack"]),
                test_env.stream.get_stream_interests()
            );
            test_env.ack_packet(pn(0), ExpectWakeup(Some(false)));
        }

        assert_eq!(
            stream_interests(&["fin"]),
            test_env.stream.get_stream_interests()
        );

        // The second call checks whether the same result is delivered after the
        // user had been notified.
        assert_matches!(
            test_env.poll_finish(),
            Poll::Ready(Err(StreamError::StreamReset { .. })),
        );

        assert_eq!(
            stream_interests(&["fin"]),
            test_env.stream.get_stream_interests()
        );
    }
}

#[test]
fn stop_sending_while_waiting_for_fin_to_get_acknowledged_leads_to_a_reset() {
    let mut test_env = setup_send_only_test_env();
    let error_code = ApplicationErrorCode::new(0x1234_5678).unwrap();

    execute_instructions(
        &mut test_env,
        &[
            // Call finish and expect the frame to get emitted
            Instruction::Finish(false),
            Instruction::CheckDataTx(VarInt::from_u32(0), 0, true, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            // Inject stop sending. This should lead to a reset frame,
            // and to no longer wait for an acknowledge on the FIN.
            // It should thereby also wake up the waiter
            Instruction::StopSending(error_code, ExpectWakeup(Some(true))),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckResetTx(error_code, pn(1), VarInt::from_u32(0)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
            // This is the ACK for the FIN, for which we no longer care
            Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["ack"])),
        ],
    );

    assert_matches!(
        test_env.poll_finish(),
        Poll::Ready(Err(StreamError::StreamReset { .. })),
    );

    execute_instructions(
        &mut test_env,
        &[
            // This is the ACK for the Reset
            Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
            Instruction::CheckInterests(stream_interests(&["fin"])),
        ],
    );
}

#[test]
fn stop_sending_while_sending_data_leads_to_a_reset() {
    for require_wakeup in &[true, false] {
        let test_env_config = TestEnvironmentConfig {
            max_send_buffer_size: 1000,
            stream_id: StreamId::initial(endpoint::Type::Server, StreamType::Unidirectional),
            local_endpoint_type: endpoint::Type::Server,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        execute_instructions(
            &mut test_env,
            &[
                Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
                Instruction::CheckDataTx(VarInt::from_u32(0), 500, false, false, pn(0)),
                Instruction::CheckInterests(stream_interests(&["ack"])),
                Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
                Instruction::CheckInterests(stream_interests(&[])),
            ],
        );
        let transmitted_data = 500;

        if *require_wakeup {
            // Enqueue enough data that the stream is blocked
            execute_instructions(
                &mut test_env,
                &[
                    Instruction::EnqueueData(VarInt::from_u32(500), 2000, true),
                    Instruction::CheckInterests(stream_interests(&["tx"])),
                    Instruction::EnqueueData(VarInt::from_u32(2500), 2000, false),
                ],
            );
        }

        let error_code = ApplicationErrorCode::new(0x1234_5678).unwrap();

        execute_instructions(
            &mut test_env,
            &[
                // Inject stop sending. This should lead to a reset frame
                Instruction::StopSending(error_code, ExpectWakeup(Some(*require_wakeup))),
                Instruction::CheckInterests(stream_interests(&["tx"])),
                Instruction::CheckResetTx(error_code, pn(1), VarInt::from_u32(transmitted_data)),
                Instruction::CheckInterests(stream_interests(&["ack"])),
                Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
                Instruction::CheckInterests(stream_interests(&[])),
            ],
        );

        assert_matches!(
            test_env.poll_finish(),
            Poll::Ready(Err(StreamError::StreamReset { .. })),
        );

        execute_instructions(
            &mut test_env,
            &[Instruction::CheckInterests(stream_interests(&["fin"]))],
        );
    }
}

#[test]
fn stop_sending_does_not_cause_an_action_if_stream_is_already_reset() {
    for reset_is_acknowledged in &[true, false] {
        let mut test_env = setup_send_only_test_env();

        let error_code = ApplicationErrorCode::new(0x1234_5678).unwrap();
        let stop_sending_error_code = ApplicationErrorCode::new(0x3333_4444).unwrap();

        // Call finish and expect the frame to get emitted
        execute_instructions(
            &mut test_env,
            &[
                Instruction::Reset(error_code, true),
                Instruction::CheckInterests(stream_interests(&["tx"])),
                Instruction::CheckResetTx(error_code, pn(0), VarInt::from_u32(0)),
                Instruction::CheckInterests(stream_interests(&["ack"])),
            ],
        );

        if *reset_is_acknowledged {
            execute_instructions(
                &mut test_env,
                &[
                    Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
                    Instruction::CheckInterests(stream_interests(&["fin"])),
                ],
            );
        }

        // Inject stop sending. This should have no impact
        execute_instructions(
            &mut test_env,
            &[Instruction::StopSending(
                stop_sending_error_code,
                ExpectWakeup(Some(false)),
            )],
        );

        let expected_interest = if *reset_is_acknowledged {
            stream_interests(&["fin"])
        } else {
            stream_interests(&["ack"])
        };

        execute_instructions(
            &mut test_env,
            &[
                Instruction::CheckInterests(expected_interest),
                Instruction::CheckNoTx,
            ],
        );

        assert_matches!(
            test_env.poll_finish(),
            Poll::Ready(Err(StreamError::StreamReset { .. })),
        );

        execute_instructions(
            &mut test_env,
            &[
                Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
                Instruction::CheckInterests(stream_interests(&["fin"])),
                Instruction::CheckNoTx,
            ],
        );
    }
}

#[test]
fn stop_sending_does_not_cause_an_action_if_stream_is_finished_and_acknowledged() {
    for check_finish_flag_later in &[true, false] {
        let mut test_env = setup_send_only_test_env();
        let stop_sending_error_code = ApplicationErrorCode::new(0x3333_4444).unwrap();

        execute_instructions(
            &mut test_env,
            &[
                Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
                Instruction::Finish(false),
                Instruction::CheckInterests(stream_interests(&["tx"])),
                Instruction::CheckDataTx(VarInt::from_u32(0), 500, true, false, pn(0)),
                Instruction::CheckInterests(stream_interests(&["ack"])),
                Instruction::AckPacket(pn(0), ExpectWakeup(Some(true))),
                Instruction::CheckInterests(stream_interests(&[])),
            ],
        );

        let expected_interests = if !*check_finish_flag_later {
            execute_instructions(&mut test_env, &[Instruction::Finish(true)]);
            stream_interests(&["fin"])
        } else {
            stream_interests(&[])
        };

        execute_instructions(
            &mut test_env,
            &[
                Instruction::StopSending(stop_sending_error_code, ExpectWakeup(Some(false))),
                Instruction::CheckInterests(expected_interests),
                Instruction::CheckNoTx,
            ],
        );

        if *check_finish_flag_later {
            execute_instructions(
                &mut test_env,
                &[
                    Instruction::Finish(true),
                    Instruction::CheckInterests(stream_interests(&["fin"])),
                    Instruction::CheckNoTx,
                ],
            );
        }
    }
}

#[test]
fn reset_does_not_cause_an_action_if_stream_is_already_reset() {
    for is_internal_reset in &[true, false] {
        for acknowledge_reset in &[true, false] {
            let mut test_env = setup_send_only_test_env();

            let error_code = ApplicationErrorCode::new(0x1234_5678).unwrap();
            let reset_error_code = ApplicationErrorCode::new(0x3333_4444).unwrap();

            // Perform the initial reset
            execute_instructions(
                &mut test_env,
                &[
                    Instruction::Reset(error_code, true),
                    Instruction::CheckInterests(stream_interests(&["tx"])),
                    Instruction::CheckResetTx(error_code, pn(0), VarInt::from_u32(0)),
                    Instruction::CheckInterests(stream_interests(&["ack"])),
                ],
            );

            if *acknowledge_reset {
                execute_instructions(
                    &mut test_env,
                    &[
                        Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
                        Instruction::CheckInterests(stream_interests(&["fin"])),
                    ],
                );
            }

            if *is_internal_reset {
                let mut events = StreamEvents::new();
                test_env
                    .stream
                    .on_internal_reset(connection::Error::unspecified().into(), &mut events);
                assert!(events.write_wake.is_none());
            } else {
                execute_instructions(&mut test_env, &[Instruction::Reset(reset_error_code, true)]);
            }

            // The reset should not lead to an outgoing packet
            assert!(test_env
                .stream
                .get_stream_interests()
                .transmission
                .is_none());
            execute_instructions(&mut test_env, &[Instruction::CheckNoTx]);

            // Accessing the stream should lead to the original reset error
            assert_matches!(
                test_env.poll_finish(),
                Poll::Ready(Err(StreamError::StreamReset { .. })),
            );
        }
    }
}

#[test]
fn reset_does_not_cause_an_action_if_stream_is_finished_and_acknowledged() {
    for is_internal_reset in &[true, false] {
        for check_finish_flag_early in &[true, false] {
            let mut test_env = setup_send_only_test_env();

            let reset_error_code = ApplicationErrorCode::new(0x3333_4444).unwrap();

            execute_instructions(
                &mut test_env,
                &[
                    Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
                    Instruction::Finish(false),
                    Instruction::CheckInterests(stream_interests(&["tx"])),
                    Instruction::CheckDataTx(VarInt::from_u32(0), 500, true, false, pn(0)),
                    Instruction::CheckInterests(stream_interests(&["ack"])),
                    Instruction::AckPacket(pn(0), ExpectWakeup(Some(true))),
                    Instruction::CheckInterests(stream_interests(&[])),
                ],
            );

            if *check_finish_flag_early {
                execute_instructions(
                    &mut test_env,
                    &[
                        Instruction::Finish(true),
                        Instruction::CheckInterests(stream_interests(&["fin"])),
                    ],
                );
            }

            if *is_internal_reset {
                let mut events = StreamEvents::new();
                test_env
                    .stream
                    .on_internal_reset(connection::Error::unspecified().into(), &mut events);
                assert!(events.write_wake.is_none());
            } else {
                execute_instructions(&mut test_env, &[Instruction::Reset(reset_error_code, true)]);
            }

            // The reset should not lead to an outgoing packet
            assert!(test_env
                .stream
                .get_stream_interests()
                .transmission
                .is_none());
            execute_instructions(&mut test_env, &[Instruction::CheckNoTx]);

            // Accessing the stream should still return the finished state
            execute_instructions(
                &mut test_env,
                &[
                    Instruction::Finish(true),
                    Instruction::CheckInterests(stream_interests(&["fin"])),
                    Instruction::CheckNoTx,
                ],
            );
        }
    }
}

#[test]
fn resetting_the_stream_does_does_trigger_a_reset_frame_and_reset_errors() {
    #[derive(Copy, Clone, Debug, PartialEq)]
    enum ResetReason {
        StopSending,
        ApplicationReset,
        InternalReset,
    }

    for reset_reason in &[
        ResetReason::StopSending,
        ResetReason::ApplicationReset,
        ResetReason::InternalReset,
    ] {
        for is_finishing in &[true, false] {
            let test_env_config = TestEnvironmentConfig {
                max_send_buffer_size: 1000,
                stream_id: StreamId::initial(endpoint::Type::Client, StreamType::Unidirectional),
                local_endpoint_type: endpoint::Type::Client,
                ..Default::default()
            };
            let mut test_env = setup_stream_test_env_with_config(test_env_config);

            let reset_error_code = ApplicationErrorCode::new(0x3333_4444).unwrap();

            let transmitted_data = if *is_finishing {
                execute_instructions(
                    &mut test_env,
                    &[
                        Instruction::EnqueueData(VarInt::from_u32(0), 500, true),
                        Instruction::Finish(false),
                        Instruction::CheckInterests(stream_interests(&["tx"])),
                        Instruction::CheckDataTx(VarInt::from_u32(0), 500, true, false, pn(0)),
                        Instruction::CheckInterests(stream_interests(&["ack"])),
                    ],
                );
                500
            } else {
                execute_instructions(
                    &mut test_env,
                    &[
                        // Queue enough data to that a write gets blocked
                        Instruction::EnqueueData(VarInt::from_u32(0), 2000, true),
                        Instruction::EnqueueData(VarInt::from_u32(2000), 2000, false),
                        Instruction::CheckInterests(stream_interests(&["tx"])),
                        Instruction::CheckDataTx(VarInt::from_u32(0), 2000, false, false, pn(0)),
                        Instruction::CheckInterests(stream_interests(&["ack"])),
                    ],
                );
                2000
            };

            match *reset_reason {
                ResetReason::InternalReset => {
                    let mut events = StreamEvents::new();
                    test_env
                        .stream
                        .on_internal_reset(connection::Error::unspecified().into(), &mut events);
                    assert!(events.write_wake.is_some());

                    // No RESET frame should be transitted due to an internal reset
                    execute_instructions(
                        &mut test_env,
                        &[
                            Instruction::CheckNoTx,
                            Instruction::CheckInterests(stream_interests(&[])),
                        ],
                    );

                    assert_matches!(
                        test_env.poll_finish(),
                        Poll::Ready(Err(StreamError::ConnectionError {
                            error: connection::Error::Unspecified { .. },
                            ..
                        })),
                    );
                }
                ResetReason::ApplicationReset => {
                    // The reset should lead to an outgoing packet
                    execute_instructions(
                        &mut test_env,
                        &[
                            Instruction::Reset(reset_error_code, true),
                            Instruction::CheckInterests(stream_interests(&["tx"])),
                            Instruction::CheckResetTx(
                                reset_error_code,
                                pn(1),
                                VarInt::from_u32(transmitted_data),
                            ),
                            Instruction::CheckInterests(stream_interests(&["ack"])),
                        ],
                    );

                    // Accessing the stream should lead to the reset error
                    assert_matches!(
                        test_env.poll_finish(),
                        Poll::Ready(Err(StreamError::StreamReset { .. })),
                    );
                }
                ResetReason::StopSending => {
                    // Stop sending should lead to an outgoing packet
                    execute_instructions(
                        &mut test_env,
                        &[
                            Instruction::StopSending(reset_error_code, ExpectWakeup(Some(true))),
                            Instruction::CheckInterests(stream_interests(&["tx"])),
                            Instruction::CheckResetTx(
                                reset_error_code,
                                pn(1),
                                VarInt::from_u32(transmitted_data),
                            ),
                            Instruction::CheckInterests(stream_interests(&["ack"])),
                        ],
                    );

                    // Accessing the stream should lead to the reset error
                    assert_matches!(
                        test_env.poll_finish(),
                        Poll::Ready(Err(StreamError::StreamReset { .. })),
                    );
                }
            };

            if *reset_reason != ResetReason::InternalReset {
                // If the Reset was not caused internally, it needs to get
                // acknowledged before finalization
                execute_instructions(
                    &mut test_env,
                    &[
                        Instruction::CheckInterests(stream_interests(&["ack"])),
                        Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
                    ],
                );
            }

            execute_instructions(
                &mut test_env,
                &[Instruction::CheckInterests(stream_interests(&["fin"]))],
            );
        }
    }
}

#[test]
fn stream_does_not_try_to_acquire_connection_flow_control_credits_after_reset() {
    #[derive(Copy, Clone, Debug, PartialEq)]
    enum ResetReason {
        StopSending,
        ApplicationReset,
        InternalReset,
    }

    for reset_reason in &[
        ResetReason::StopSending,
        ResetReason::ApplicationReset,
        ResetReason::InternalReset,
    ] {
        for is_finishing in &[true, false] {
            let test_env_config = TestEnvironmentConfig {
                max_send_buffer_size: 1500,
                initial_connection_send_window_size: 1000,
                stream_id: StreamId::initial(endpoint::Type::Client, StreamType::Unidirectional),
                local_endpoint_type: endpoint::Type::Client,
                ..Default::default()
            };
            let mut test_env = setup_stream_test_env_with_config(test_env_config);

            let reset_error_code = ApplicationErrorCode::new(0x3333_4444).unwrap();

            // Enqueue data and get blocked on the flow control window
            execute_instructions(
                &mut test_env,
                &[
                    Instruction::EnqueueData(VarInt::from_u32(0), 2000, true),
                    // Try to check whether we got woken up during reset
                    Instruction::EnqueueData(VarInt::from_u32(2000), 1, false),
                    Instruction::CheckInterests(stream_interests(&["tx"])),
                    Instruction::CheckDataTx(VarInt::from_u32(0), 1000, false, false, pn(0)),
                    Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
                ],
            );

            let transmitted_data = 1000;

            if *is_finishing {
                execute_instructions(
                    &mut test_env,
                    &[
                        Instruction::Finish(false),
                        Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
                    ],
                );
            }

            match *reset_reason {
                ResetReason::InternalReset => {
                    let mut events = StreamEvents::new();
                    test_env
                        .stream
                        .on_internal_reset(connection::Error::unspecified().into(), &mut events);
                    assert!(events.write_wake.is_some());

                    // No RESET frame should be transitted due to an internal reset
                    execute_instructions(
                        &mut test_env,
                        &[
                            Instruction::CheckNoTx,
                            Instruction::CheckInterests(stream_interests(&[])),
                        ],
                    );
                }
                ResetReason::ApplicationReset => {
                    // The reset should lead to an outgoing packet
                    execute_instructions(
                        &mut test_env,
                        &[
                            Instruction::Reset(reset_error_code, true),
                            Instruction::CheckInterests(stream_interests(&["tx"])),
                            Instruction::CheckResetTx(
                                reset_error_code,
                                pn(1),
                                VarInt::from_u32(transmitted_data),
                            ),
                            Instruction::CheckInterests(stream_interests(&["ack"])),
                        ],
                    );
                }
                ResetReason::StopSending => {
                    // Stop sending should lead to an outgoing packet
                    execute_instructions(
                        &mut test_env,
                        &[
                            Instruction::StopSending(reset_error_code, ExpectWakeup(Some(true))),
                            Instruction::CheckInterests(stream_interests(&["tx"])),
                            Instruction::CheckResetTx(
                                reset_error_code,
                                pn(1),
                                VarInt::from_u32(transmitted_data),
                            ),
                            Instruction::CheckInterests(stream_interests(&["ack"])),
                        ],
                    );
                }
            };

            // Increasing the flow control window should not change readiness.
            // It should also not lead the Stream to consume from our flow control
            // window - even if we query it to do so.
            assert_eq!(
                VarInt::from_u32(0),
                test_env.tx_connection_flow_controller.available_window()
            );
            let previous_readiness = test_env.stream.get_stream_interests();
            test_env.tx_connection_flow_controller.on_max_data(MaxData {
                maximum_data: VarInt::from_u32(2500),
            });
            assert_eq!(
                VarInt::from_u32(1500),
                test_env.tx_connection_flow_controller.available_window()
            );
            test_env.stream.on_connection_window_available();
            assert_eq!(previous_readiness, test_env.stream.get_stream_interests());
            assert!(
                !test_env
                    .stream
                    .get_stream_interests()
                    .connection_flow_control_credits
            );
            assert_eq!(
                VarInt::from_u32(1500),
                test_env.tx_connection_flow_controller.available_window()
            );

            // Accessing the stream should lead to the reset error
            assert_matches!(test_env.poll_finish(), Poll::Ready(Err(_)));

            if *reset_reason != ResetReason::InternalReset {
                // If the Reset was not caused internally, it needs to get
                // acknowledged before finalization
                execute_instructions(
                    &mut test_env,
                    &[
                        Instruction::CheckInterests(stream_interests(&["ack"])),
                        Instruction::AckPacket(pn(1), ExpectWakeup(Some(false))),
                    ],
                );
            }

            execute_instructions(
                &mut test_env,
                &[Instruction::CheckInterests(stream_interests(&["fin"]))],
            );
        }
    }
}

#[test]
fn stream_reports_stream_size_based_on_acquired_connection_window() {
    let test_env_config = TestEnvironmentConfig {
        max_send_buffer_size: 1500,
        initial_send_window: 100 * 1024,
        initial_connection_send_window_size: 1000,
        stream_id: StreamId::initial(endpoint::Type::Client, StreamType::Unidirectional),
        local_endpoint_type: endpoint::Type::Client,
        ..Default::default()
    };
    let mut test_env = setup_stream_test_env_with_config(test_env_config);

    let reset_error_code = ApplicationErrorCode::new(0x3333_4444).unwrap();

    // Enqueue data and get blocked on the flow control window
    execute_instructions(
        &mut test_env,
        &[
            Instruction::EnqueueData(VarInt::from_u32(0), 5000, true),
            Instruction::EnqueueData(VarInt::from_u32(2000), 1, false),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            Instruction::CheckDataTx(VarInt::from_u32(0), 1000, false, false, pn(0)),
            Instruction::CheckInterests(stream_interests(&["ack", "cf"])),
        ],
    );

    // Make more credits available - which the Stream will acquire
    assert_eq!(
        VarInt::from_u32(0),
        test_env.tx_connection_flow_controller.available_window()
    );
    test_env.tx_connection_flow_controller.on_max_data(MaxData {
        maximum_data: VarInt::from_u32(4999),
    });
    assert_eq!(
        VarInt::from_u32(3999),
        test_env.tx_connection_flow_controller.available_window()
    );
    test_env.stream.on_connection_window_available();
    assert_eq!(
        VarInt::from_u32(0),
        test_env.tx_connection_flow_controller.available_window()
    );

    execute_instructions(
        &mut test_env,
        &[
            Instruction::CheckInterests(stream_interests(&["ack", "tx"])),
            Instruction::Reset(reset_error_code, true),
            Instruction::CheckInterests(stream_interests(&["tx"])),
            // The Stream is expected to report the complete window it acquired
            Instruction::CheckResetTx(reset_error_code, pn(1), VarInt::from_u32(4999)),
            Instruction::CheckInterests(stream_interests(&["ack"])),
        ],
    );
}

#[test]
fn resetting_a_stream_takes_priority() {
    let error_code = ApplicationErrorCode::new(123).unwrap();

    for sizes in [None, Some(&[10, 10, 10])] {
        for finish in [true, false] {
            for flush in [true, false] {
                for with_context in [true, false] {
                    let mut test_env = setup_send_only_test_env();

                    let will_wake = with_context && flush;
                    dbg!(sizes);
                    dbg!(finish);
                    dbg!(flush);
                    dbg!(with_context);

                    let mut request = ops::Request::default();

                    request.reset(error_code);
                    let mut chunks =
                        sizes.map(|sizes| gen_pattern_test_chunks(VarInt::from_u8(0), sizes));

                    if let Some(chunks) = chunks.as_deref_mut() {
                        request.send(chunks);
                    }

                    if finish {
                        request.finish();
                    }

                    if flush {
                        request.flush();
                    }

                    assert_eq!(
                        test_env.run_request(&mut request, with_context),
                        Ok(ops::Response {
                            tx: Some(ops::tx::Response {
                                status: ops::Status::Resetting,
                                will_wake,
                                ..Default::default()
                            }),
                            rx: None,
                        }),
                    );

                    execute_instructions(
                        &mut test_env,
                        &[
                            Instruction::CheckInterests(stream_interests(&["tx"])),
                            Instruction::CheckResetTx(error_code, pn(0), VarInt::from_u8(0)),
                            Instruction::CheckInterests(stream_interests(&["ack"])),
                            Instruction::AckPacket(pn(0), ExpectWakeup(Some(will_wake))),
                        ],
                    );

                    assert_matches!(
                        test_env.run_request(&mut request, with_context),
                        Ok(ops::Response {
                            tx: Some(ops::tx::Response {
                                status: ops::Status::Reset(StreamError::StreamReset { .. }),
                                ..
                            }),
                            rx: None,
                        }),
                    );

                    execute_instructions(
                        &mut test_env,
                        &[Instruction::CheckInterests(stream_interests(&["fin"]))],
                    );
                }
            }
        }
    }
}

#[test]
fn can_send_multiple_chunks() {
    let max_send_buffer_size = 3000;
    for sizes in [&[10, 10, 10][..], &[500, 500, 500], &[1000, 1000, 1000][..]] {
        for finish in [false, true] {
            for flush in [false, true] {
                for with_context in [false, true] {
                    let test_env_config = TestEnvironmentConfig {
                        max_send_buffer_size,
                        stream_id: StreamId::initial(
                            endpoint::Type::Client,
                            StreamType::Unidirectional,
                        ),
                        local_endpoint_type: endpoint::Type::Client,
                        ..Default::default()
                    };
                    let mut test_env = setup_stream_test_env_with_config(test_env_config);
                    let mut expected_buffer_size = max_send_buffer_size;

                    dbg!(sizes);
                    dbg!(finish);
                    dbg!(flush);
                    dbg!(with_context);

                    let mut chunks = gen_pattern_test_chunks(VarInt::from_u8(0), sizes);

                    let mut request = ops::Request::default();
                    request.send(&mut chunks);

                    if finish {
                        request.finish();
                    }

                    if flush {
                        request.flush();
                    }

                    let mut expected_consumed_bytes = 0;
                    let mut expected_consumed_chunks = 0;
                    for size in sizes.iter().cloned() {
                        expected_consumed_bytes += size;
                        expected_consumed_chunks += 1;
                        if let Some(size) = expected_buffer_size.checked_sub(size) {
                            expected_buffer_size = size;
                        } else {
                            expected_buffer_size = 0;
                            break;
                        }
                    }
                    let consumed_all = expected_consumed_chunks == sizes.len();

                    // finishing the buffer should end the availability
                    if finish {
                        expected_buffer_size = 0;
                    }

                    let will_wake = with_context && (!consumed_all || flush);

                    assert_eq!(
                        test_env.run_request(&mut request, with_context),
                        Ok(ops::Response {
                            tx: Some(ops::tx::Response {
                                bytes: ops::Bytes {
                                    available: expected_buffer_size,
                                    consumed: expected_consumed_bytes,
                                },
                                chunks: ops::Chunks {
                                    available: expected_buffer_size,
                                    consumed: expected_consumed_chunks,
                                },
                                status: if consumed_all && finish {
                                    ops::Status::Finishing
                                } else {
                                    ops::Status::Open
                                },
                                will_wake,
                            }),
                            rx: None,
                        }),
                    );

                    execute_instructions(
                        &mut test_env,
                        &[Instruction::CheckInterests(stream_interests(&["tx"]))],
                    );

                    let mut offset = 0;
                    let mut idx = 0;
                    while let Some(mut frame) = test_env.transmit() {
                        if let Frame::Stream(stream) = frame.as_frame() {
                            offset += stream.data.len();
                            test_env.ack_packet(pn(idx), ExpectWakeup(None));
                            idx += 1;
                        } else {
                            panic!("invalid frame");
                        }
                    }

                    assert_eq!(expected_consumed_bytes, offset);

                    execute_instructions(
                        &mut test_env,
                        &[Instruction::CheckInterests(stream_interests(&[]))],
                    );
                }
            }
        }
    }
}

#[test]
fn detach_modes() {
    for detached in [false, true] {
        let test_env_config = TestEnvironmentConfig {
            stream_id: StreamId::initial(endpoint::Type::Client, StreamType::Unidirectional),
            local_endpoint_type: endpoint::Type::Client,
            ..Default::default()
        };
        let mut test_env = setup_stream_test_env_with_config(test_env_config);

        let mut chunks = gen_pattern_test_chunks(VarInt::from_u8(0), &[16]);
        let mut request = ops::Request::default();

        request.send(&mut chunks).finish();

        if detached {
            request.detach_tx();
        }

        test_env
            .run_request(&mut request, false)
            .expect("request should succeed");

        execute_instructions(
            &mut test_env,
            &[
                Instruction::CheckInterests(stream_interests(&["tx"])),
                Instruction::CheckDataTx(VarInt::from_u32(0), 16, true, false, pn(0)),
                Instruction::CheckInterests(stream_interests(&["ack"])),
                Instruction::AckPacket(pn(0), ExpectWakeup(Some(false))),
                Instruction::CheckInterests(if detached {
                    stream_interests(&["fin"])
                } else {
                    stream_interests(&[])
                }),
            ],
        );
    }
}

#[test]
fn can_query_stream_readiness() {
    let max_send_buffer_size = 1500;
    for size in [None, Some(1000usize), Some(2000)] {
        for with_context in [false, true] {
            let test_env_config = TestEnvironmentConfig {
                max_send_buffer_size,
                stream_id: StreamId::initial(endpoint::Type::Client, StreamType::Unidirectional),
                local_endpoint_type: endpoint::Type::Client,
                ..Default::default()
            };
            let mut test_env = setup_stream_test_env_with_config(test_env_config);

            dbg!(size);
            dbg!(with_context);

            let mut expected_buffer_size = max_send_buffer_size;

            // potentially fill the buffer
            if let Some(size) = size {
                test_env
                    .run_request(
                        ops::Request::default()
                            .send(&mut gen_pattern_test_chunks(VarInt::from_u8(0), &[size])),
                        false,
                    )
                    .expect("request should succeed");

                expected_buffer_size = expected_buffer_size.saturating_sub(size);
            };

            assert_eq!(
                test_env.run_request(ops::Request::default().send(&mut []), with_context),
                Ok(ops::Response {
                    tx: Some(ops::tx::Response {
                        bytes: ops::Bytes {
                            available: expected_buffer_size,
                            consumed: 0
                        },
                        chunks: ops::Chunks {
                            available: expected_buffer_size,
                            consumed: 0,
                        },
                        status: ops::Status::Open,
                        will_wake: with_context && expected_buffer_size == 0,
                    }),
                    rx: None,
                }),
            );
        }
    }
}
