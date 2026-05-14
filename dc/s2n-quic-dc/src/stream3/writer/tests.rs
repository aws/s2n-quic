// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for the stream3 Writer.
//!
//! ## Organization
//!
//! * **Bach async tests** – run the Writer with two primary tasks:
//!   * **Application task** owns [`Writer`] and calls write APIs.
//!   * **Endpoint task** owns [`Pusher`] and asserts on emitted [`Frame`]s.

use super::*;
use crate::{
    flow, intrusive_queue,
    packet::datagram::ResetTarget,
    path::secret::map::Entry as PathSecretEntry,
    stream3::frame::{self, Frame, Header, PriorityStorage, SubmissionReceiver},
};
use bytes::Bytes;
use s2n_codec::EncoderValue;
use s2n_quic_core::{endpoint, frame::MaxData, varint::VarInt};
use std::{net::SocketAddr, task::Poll, time::Duration};

// ─── Test helpers ─────────────────────────────────────────────────────────────

fn make_client_pair() -> (Writer, Pusher) {
    make_pair_with_type(endpoint::Type::Client)
}

fn make_server_pair() -> (Writer, Pusher) {
    make_pair_with_type(endpoint::Type::Server)
}

fn make_pair_with_type(ep_type: endpoint::Type) -> (Writer, Pusher) {
    let stream_id = VarInt::from_u8(42);
    let acceptor_id = VarInt::from_u8(7);
    let remote_queue_id = VarInt::from_u8(2);
    let peer: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    let path_secret_entry = PathSecretEntry::fake_deterministic(peer, ep_type);

    let allocator = msg::queue::Allocator::new();
    let dispatcher = allocator.dispatcher();
    let handle = match ep_type {
        endpoint::Type::Client => flow::Handle::client(stream_id, path_secret_entry.clone()),
        endpoint::Type::Server => {
            let tracker = flow::Tracker::new(*path_secret_entry.id());
            tracker
                .try_register(stream_id, |handle| (VarInt::ZERO, handle))
                .expect("server handle registration should succeed")
        }
    };
    let (control_rx, _stream_rx) = dispatcher
        .alloc(handle, Some(remote_queue_id))
        .expect("queue alloc should succeed");

    let queue_id = control_rx.queue_id();
    let request = flow::Request {
        credential_id: *path_secret_entry.id(),
        stream_id,
    };

    let (frame_tx, frame_rx) = frame::submission_channel(1);

    let writer = match ep_type {
        endpoint::Type::Client => {
            Writer::new_client(frame_tx, path_secret_entry, stream_id, acceptor_id, control_rx)
        }
        endpoint::Type::Server => Writer::new_server(frame_tx, path_secret_entry, stream_id, control_rx),
    };

    let pusher = Pusher {
        dispatcher,
        queue_id,
        request,
        frame_rx,
        frame_storage: PriorityStorage::default(),
    };

    (writer, pusher)
}

struct Pusher {
    dispatcher: msg::queue::Dispatcher,
    queue_id: VarInt,
    request: flow::Request,
    frame_rx: SubmissionReceiver,
    frame_storage: PriorityStorage,
}

impl Pusher {
    fn push_control(&mut self, message: msg::Control) {
        if self
            .dispatcher
            .send_control(
                self.queue_id,
                None,
                &self.request,
                intrusive_queue::Entry::new(message),
            )
            .is_err()
        {
            panic!("send_control should succeed in tests");
        }
    }

    fn push_reset(&mut self, error_code: VarInt) {
        self.push_control(msg::Control::Reset { error_code });
    }

    fn push_max_data(&mut self, maximum_data: VarInt) {
        self.push_control(msg::Control::Frames {
            payload: Bytes::from(MaxData { maximum_data }.encode_to_vec()).into(),
        });
    }

    /// Receives one submitted burst.
    ///
    /// Tests that expect multiple submission cycles should call this helper
    /// again.
    async fn recv_frames(&mut self) -> intrusive_queue::Queue<Frame> {
        core::future::poll_fn(|cx| self.frame_rx.poll_swap(cx, &mut self.frame_storage)).await;
        let mut combined_frames = intrusive_queue::Queue::default();
        for (_priority, mut queue) in self.frame_storage.drain() {
            combined_frames.append(&mut queue);
        }
        combined_frames
    }

    async fn recv_frames_timeout(
        &mut self,
        duration: Duration,
    ) -> Option<intrusive_queue::Queue<Frame>> {
        crate::testing::timeout(duration, self.recv_frames()).await.ok()
    }

    fn complete_all(
        &mut self,
        mut frames: intrusive_queue::Queue<Frame>,
        status: frame::TransmissionStatus,
    ) {
        while let Some(entry) = frames.pop_front() {
            let mut completed = entry.into_inner();
            let Some(sender) = completed.completion.take() else {
                continue;
            };
            completed.status = status;

            let mut queue = intrusive_queue::Queue::new();
            queue.push_back(completed.into());
            sender
                .send_batch(queue)
                .expect("completion send should succeed in tests");
        }
    }
}

// ─── Bach async tests ─────────────────────────────────────────────────────────

#[test]
fn client_write_all_from_fin_sends_flow_init_with_early_data_and_fin() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let frames = pusher.recv_frames().await;
            let frame = frames.iter().next().expect("expected one FlowInit frame");
            assert!(frames.iter().nth(1).is_none(), "expected exactly one frame");
            assert!(matches!(frame.header, Header::FlowInit { is_fin: true, .. }));
            assert_eq!(frame.payload, &b"hello"[..]);
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer
                .write_all_from_fin(&mut payload)
                .await
                .expect("write should succeed");
            assert_eq!(written, 5);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn control_reset_terminates_write() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_client_pair();

        async move {
            pusher.push_reset(VarInt::from_u8(9));
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let err = writer
                .write_from(&mut payload)
                .await
                .expect_err("expected ConnectionReset");
            assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

#[test]
fn client_second_write_blocks_until_max_data() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let frames = pusher.recv_frames().await;
            {
                let frame = frames.iter().next().expect("expected one FlowInit frame");
                assert!(frames.iter().nth(1).is_none(), "expected exactly one frame");
                assert!(matches!(frame.header, Header::FlowInit { is_fin: false, .. }));
                assert_eq!(frame.payload, &b"hello"[..]);
            }

            // Give the app task a scheduling opportunity to attempt a second
            // write while Writer is still in `Status::FlowInitSent` (before any
            // remote MAX_DATA credit is injected).
            bach::task::yield_now().await;
            pusher.push_max_data(VarInt::from_u16(4096));

            let next = pusher.recv_frames().await;
            {
                let frame = next.iter().next().expect("expected one FlowData frame");
                assert!(next.iter().nth(1).is_none(), "expected exactly one frame");
                assert!(matches!(
                    frame.header,
                    Header::FlowData { is_fin: true, .. }
                ));
                assert_eq!(frame.payload, &b"!"[..]);
            }
        }
        .primary()
        .spawn();

        async move {
            let mut first = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut first).await.expect("first write");
            assert_eq!(written, 5);

            let mut second = Bytes::from_static(b"!");
            let write_blocked =
                core::future::poll_fn(|cx| match writer.poll_write_from(cx, &mut second, false) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                })
                .await;
            assert!(
                write_blocked,
                "expected second write to block before MAX_DATA"
            );

            let written = writer
                .write_from_fin(&mut second)
                .await
                .expect("second write");
            assert_eq!(written, 1);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_first_write_emits_flow_data_not_flow_init() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert!(
                frames.iter().next().is_some(),
                "expected at least one FlowData frame"
            );

            let mut expected_offset = 0u64;
            let mut payload = Vec::new();
            for frame in frames.iter() {
                match frame.header {
                    Header::FlowData {
                        offset, is_fin, ..
                    } => {
                        assert_eq!(offset.as_u64(), expected_offset);
                        if is_fin {
                            assert!(
                                frame.payload.is_empty(),
                                "drop-triggered FIN frame should be empty"
                            );
                        }
                    }
                    _ => panic!("server write should only emit FlowData"),
                }

                for chunk in frame.payload.chunks() {
                    payload.extend_from_slice(chunk);
                }
                expected_offset += frame.payload.len() as u64;
            }

            assert_eq!(payload, b"hello");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("write should succeed");
            assert_eq!(written, 5);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_flow_control_budget_caps_transmitted_bytes() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::from_u8(3);

        async move {
            let frames = pusher.recv_frames().await;
            {
                assert!(
                    frames.iter().next().is_some(),
                    "expected at least one FlowData frame"
                );

                let mut expected_offset = 0u64;
                let mut payload = Vec::new();
                for frame in frames.iter() {
                    match frame.header {
                        Header::FlowData {
                            offset, is_fin, ..
                        } => {
                            assert_eq!(offset.as_u64(), expected_offset);
                            if is_fin {
                                assert!(
                                    frame.payload.is_empty(),
                                    "drop-triggered FIN frame should be empty"
                                );
                            }
                        }
                        _ => panic!("server write should only emit FlowData"),
                    }

                    for chunk in frame.payload.chunks() {
                        payload.extend_from_slice(chunk);
                    }
                    expected_offset += frame.payload.len() as u64;
                }

                assert_eq!(payload, b"abc");
            }

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            let has_extra_frames = extra
                .as_ref()
                .is_some_and(|frames| frames.iter().next().is_some());
            assert!(
                !has_extra_frames,
                "expected no additional burst after exhausting remote flow budget"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"abcdef");
            let written = writer
                .write_from(&mut payload)
                .await
                .expect("write should respect remote budget");
            assert_eq!(written, 3);
            assert_eq!(payload.as_ref(), &b"def"[..]);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn client_preserves_max_data_on_out_of_order_lower_update() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let init = pusher.recv_frames().await;
            {
                let init_frame = init.iter().next().expect("expected FlowInit frame");
                assert!(
                    init.iter().nth(1).is_none(),
                    "expected exactly one FlowInit frame"
                );
                assert!(matches!(
                    init_frame.header,
                    Header::FlowInit { is_fin: false, .. }
                ));
                assert_eq!(init_frame.payload, &b"abc"[..]);
            }

            pusher.push_max_data(VarInt::from_u8(8));
            pusher.push_max_data(VarInt::from_u8(3));

            let next = pusher.recv_frames().await;
            {
                let mut expected_offset = 3u64;
                let mut payload = Vec::new();
                for frame in next.iter() {
                    match frame.header {
                        Header::FlowData {
                            offset, is_fin, ..
                        } => {
                            assert_eq!(offset.as_u64(), expected_offset);
                            if is_fin {
                                assert!(
                                    frame.payload.is_empty(),
                                    "drop-triggered FIN frame should be empty"
                                );
                            }
                        }
                        _ => panic!("expected FlowData frame"),
                    }

                    for chunk in frame.payload.chunks() {
                        payload.extend_from_slice(chunk);
                    }
                    expected_offset += frame.payload.len() as u64;
                }

                assert_eq!(payload, b"defgh");
            }
        }
        .primary()
        .spawn();

        async move {
            let mut first = Bytes::from_static(b"abc");
            let written = writer.write_from(&mut first).await.expect("first write");
            assert_eq!(written, 3);

            bach::task::yield_now().await;

            let mut second = Bytes::from_static(b"defghij");
            let written = writer.write_from(&mut second).await.expect("second write");
            assert_eq!(
                written, 5,
                "writer should keep the max observed MAX_DATA even when a smaller update arrives later"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_write_from_fin_blocks_while_budget_exhausted_then_sends_single_fin_frame() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::from_u8(1);

        async move {
            let first = pusher.recv_frames().await;
            {
                let first_frame = first.iter().next().expect("expected one first frame");
                assert!(first.iter().nth(1).is_none(), "expected exactly one frame");
                assert!(matches!(
                    first_frame.header,
                    Header::FlowData {
                        is_fin: false,
                        offset,
                        ..
                    } if offset == VarInt::ZERO
                ));
                assert_eq!(first_frame.payload, &b"a"[..]);
            }

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no frame while remote flow budget is exhausted"
            );

            pusher.push_max_data(VarInt::from_u8(2));

            let second = pusher.recv_frames().await;
            {
                let second_frame = second.iter().next().expect("expected one second frame");
                assert!(second.iter().nth(1).is_none(), "expected exactly one frame");
                assert!(matches!(
                    second_frame.header,
                    Header::FlowData {
                        is_fin: true,
                        offset,
                        ..
                    } if offset == VarInt::from_u8(1)
                ));
                assert_eq!(second_frame.payload, &b"b"[..]);
            }
        }
        .primary()
        .spawn();

        async move {
            let mut first = Bytes::from_static(b"a");
            let written = writer.write_from(&mut first).await.expect("first write");
            assert_eq!(written, 1);

            let mut second = Bytes::from_static(b"b");
            let write_blocked =
                core::future::poll_fn(|cx| match writer.poll_write_from(cx, &mut second, true) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                })
                .await;
            assert!(
                write_blocked,
                "expected write_from_fin to block while remote flow budget is exhausted"
            );

            let written = writer
                .write_from_fin(&mut second)
                .await
                .expect("second write after MAX_DATA");
            assert_eq!(written, 1);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn client_fin_write_then_drop_emits_no_extra_packet() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let first = pusher.recv_frames().await;
            {
                assert!(first.iter().next().is_some(), "expected at least one frame");
                let mut payload = Vec::new();
                let mut fin_count = 0usize;
                for frame in first.iter() {
                    match frame.header {
                        Header::FlowInit { is_fin, .. } => {
                            if is_fin {
                                fin_count += 1;
                            }
                        }
                        _ => panic!("client initial FIN write should emit FlowInit frames only"),
                    }
                    for chunk in frame.payload.chunks() {
                        payload.extend_from_slice(chunk);
                    }
                }
                assert_eq!(payload, b"hi");
                assert_eq!(fin_count, 1, "expected exactly one FIN marker");
            }

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            let has_extra_frames = extra
                .as_ref()
                .is_some_and(|frames| frames.iter().next().is_some());
            assert!(
                !has_extra_frames,
                "expected no extra frame burst after FIN was already sent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hi");
            let written = writer.write_from_fin(&mut payload).await.expect("fin write");
            assert_eq!(written, 2);
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_fin_write_then_drop_emits_no_extra_packet() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let first = pusher.recv_frames().await;
            {
                assert!(first.iter().next().is_some(), "expected at least one frame");
                let mut payload = Vec::new();
                let mut fin_count = 0usize;
                let mut expected_offset = 0u64;
                for frame in first.iter() {
                    match frame.header {
                        Header::FlowData {
                            is_fin, offset, ..
                        } => {
                            if is_fin {
                                fin_count += 1;
                            }
                            assert_eq!(offset.as_u64(), expected_offset);
                        }
                        _ => panic!("server FIN write should emit FlowData frames only"),
                    }
                    for chunk in frame.payload.chunks() {
                        payload.extend_from_slice(chunk);
                    }
                    expected_offset += frame.payload.len() as u64;
                }
                assert_eq!(payload, b"hi");
                assert_eq!(fin_count, 1, "expected exactly one FIN marker");
            }

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            let has_extra_frames = extra
                .as_ref()
                .is_some_and(|frames| frames.iter().next().is_some());
            assert!(
                !has_extra_frames,
                "expected no extra frame burst after FIN was already sent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hi");
            let written = writer.write_from_fin(&mut payload).await.expect("fin write");
            assert_eq!(written, 2);
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn transmission_error_completion_causes_broken_pipe_and_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let first = pusher.recv_frames().await;
            pusher.complete_all(
                first,
                frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
            );

            let reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected FlowReset after transmission failure");
            let has_reset = reset.iter().any(|f| {
                matches!(
                    f.header,
                    Header::FlowReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == reset_error::RETRANSMISSIONS_EXHAUSTED
                )
            });
            assert!(has_reset, "expected retransmission-exhausted FlowReset");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected BrokenPipe");
            assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn drop_open_writer_sends_fin_packet() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            let frame = frames.iter().next().expect("expected one FIN frame");
            assert!(
                frames.iter().nth(1).is_none(),
                "expected exactly one FIN frame on drop"
            );
            assert!(matches!(
                frame.header,
                Header::FlowData {
                    is_fin: true,
                    offset,
                    ..
                } if offset == VarInt::ZERO
            ));
        }
        .primary()
        .spawn();

        async move {
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn panic_drop_sends_abnormal_termination_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            let has_abnormal_reset = frames.iter().any(|f| {
                matches!(
                    f.header,
                    Header::FlowReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == reset_error::ABNORMAL_TERMINATION
                )
            });
            assert!(
                has_abnormal_reset,
                "expected FlowReset(Both, ABNORMAL_TERMINATION) when dropping during panic"
            );
        }
        .primary()
        .spawn();

        async move {
            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _writer = writer;
                panic!("intentional test panic while dropping writer");
            }));
            assert!(panic_result.is_err());
        }
        .primary()
        .spawn();
    });
}
