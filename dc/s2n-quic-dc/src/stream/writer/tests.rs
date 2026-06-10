// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for the stream Writer.
//!
//! ## Organization
//!
//! * **Bach async tests** – run the Writer with two primary tasks:
//!   * **Application task** owns [`Writer`] and calls write APIs.
//!   * **Endpoint task** owns [`Pusher`] and asserts on emitted [`Frame`]s.

use super::*;
use crate::{
    endpoint::frame::{self, Frame, Header, PriorityStorage, SubmissionReceiver},
    intrusive,
    packet::datagram::ResetTarget,
    path::secret::map::Entry as PathSecretEntry,
    stream::metrics::WriterMetrics,
    testing::sim,
};
use bach::{ext::*, time::timeout};
use bytes::Bytes;
use s2n_quic_core::{endpoint, stream::testing::Data, varint::VarInt};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Poll,
    time::Duration,
};

fn test_writer_metrics() -> Arc<WriterMetrics> {
    Arc::new(WriterMetrics::new(
        &crate::counter::Registry::default(),
        "test",
    ))
}

// ─── Test helpers ─────────────────────────────────────────────────────────────

struct PairBuilder {
    ep_type: endpoint::Type,
    credit_pool: Option<crate::sync::Arc<crate::credit::Pool>>,
    priority: crate::credit::Priority,
}

impl Default for PairBuilder {
    fn default() -> Self {
        Self {
            ep_type: endpoint::Type::Client,
            credit_pool: None,
            priority: crate::credit::Priority::default(),
        }
    }
}

impl PairBuilder {
    fn server() -> Self {
        Self {
            ep_type: endpoint::Type::Server,
            credit_pool: None,
            priority: crate::credit::Priority::default(),
        }
    }

    fn client_no_remote_queue_id() -> Self {
        Self::default()
    }

    /// Override the credit pool used by the writer. Allows tests to assert against pool counters
    /// (`debug_available`, `debug_parked_demand`) and to drive the pool into specific states.
    fn with_credit_pool(mut self, pool: crate::sync::Arc<crate::credit::Pool>) -> Self {
        self.credit_pool = Some(pool);
        self
    }

    fn with_priority(mut self, priority: crate::credit::Priority) -> Self {
        self.priority = priority;
        self
    }

    fn build(self) -> (Writer, Pusher) {
        let acceptor_id = VarInt::from_u8(7);
        let peer: SocketAddr = "127.0.0.1:4433".parse().unwrap();
        let path_secret_entry = PathSecretEntry::builder(peer)
            .endpoint_type(self.ep_type)
            .build();

        let client_state =
            std::sync::Arc::new(crate::queue::ClientState::new(VarInt::from_u16(100)));
        let dest_queue_id = client_state.peer_free.try_alloc().unwrap();
        let alloc = client_state.alloc_local(dest_queue_id).unwrap();
        let dispatcher = crate::queue::ClientDispatch::new(client_state);

        let queue_id = alloc.control.queue_id();
        let binding_id = alloc.control.binding_id();

        let (frame_tx, frame_rx) = frame::submission_channel(1);

        let send_credit_pool = self.credit_pool.unwrap_or_else(|| {
            crate::sync::Arc::new(crate::credit::Pool::new(crate::credit::Config::default()))
        });
        let writer = match self.ep_type {
            endpoint::Type::Client => Writer::new_client(
                frame_tx,
                path_secret_entry,
                dest_queue_id,
                acceptor_id,
                alloc.control,
                crate::time::DefaultClock::default(),
                test_writer_metrics(),
                send_credit_pool,
                self.priority,
            ),
            endpoint::Type::Server => Writer::new_server(
                frame_tx,
                path_secret_entry,
                dest_queue_id,
                acceptor_id,
                alloc.control,
                crate::time::DefaultClock::default(),
                test_writer_metrics(),
                send_credit_pool,
                self.priority,
            ),
        };

        let pusher = Pusher {
            dispatcher,
            queue_id,
            binding_id,
            frame_rx,
            frame_storage: PriorityStorage::default(),
            assembler: PayloadAssembler::default(),
        };

        (writer, pusher)
    }
}

fn make_client_pair() -> (Writer, Pusher) {
    PairBuilder::default().build()
}

fn make_server_pair() -> (Writer, Pusher) {
    PairBuilder::server().build()
}

#[test]
fn peer_addr_returns_handshake_addr() {
    let (writer, _) = make_client_pair();
    let expected: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    assert_eq!(writer.peer_addr(), expected);
}

/// Reassembles payload from a sequence of data frames, validating contiguous
/// offsets and FIN semantics along the way.
///
/// Designed to be held for the lifetime of a Pusher task so that offset tracking
/// stays consistent across multiple `recv_frames` batches.
#[derive(Default)]
struct PayloadAssembler {
    expected_offset: u64,
    payload: Vec<u8>,
    fin_count: usize,
}

impl PayloadAssembler {
    /// Feeds a batch of frames through the assembler, asserting that each frame
    /// carries a contiguous offset and valid FIN semantics.
    ///
    /// `header_filter` is called with each frame's header to perform
    /// variant-specific assertions (e.g. ensuring only QueueData is emitted).
    /// It must return `(offset, is_fin)` for the frame.
    fn push(
        &mut self,
        frames: &intrusive::Queue<Frame>,
        header_filter: impl Fn(&Header) -> (VarInt, bool),
    ) {
        for frame in frames.iter() {
            assert_eq!(
                self.fin_count, 0,
                "frame after FIN: writer must not send data after setting is_fin"
            );
            let (offset, is_fin) = header_filter(&frame.header);
            assert_eq!(offset.as_u64(), self.expected_offset);
            if is_fin {
                self.fin_count += 1;
            }
            for chunk in frame.payload.chunks() {
                self.payload.extend_from_slice(chunk);
            }
            self.expected_offset += frame.payload.len() as u64;
        }
    }

    /// Feeds frames expecting only `Header::QueueData` variants.
    fn push_queue_data(&mut self, frames: &intrusive::Queue<Frame>) {
        self.push(frames, |header| match *header {
            Header::QueueData { offset, is_fin, .. } => (offset, is_fin),
            _ => panic!("expected QueueData frame, got {header:?}"),
        });
    }

    /// Feeds frames expecting QueueData-init variants (with dest_acceptor_id).
    fn push_queue_data_init(&mut self, frames: &intrusive::Queue<Frame>) {
        self.push(frames, |header| match *header {
            Header::QueueData {
                offset,
                is_fin,
                dest_acceptor_id: Some(_),
                ..
            } => (offset, is_fin),
            _ => panic!("expected QueueData-init frame, got {header:?}"),
        });
    }

    fn assert_payload(&self, expected: &[u8]) {
        assert_eq!(self.payload, expected);
    }

    fn assert_fin_count(&self, expected: usize) {
        assert_eq!(self.fin_count, expected, "unexpected FIN count");
    }
}

struct Pusher {
    dispatcher: crate::queue::ClientDispatch,
    queue_id: VarInt,
    binding_id: VarInt,
    frame_rx: SubmissionReceiver,
    frame_storage: PriorityStorage,
    assembler: PayloadAssembler,
}

impl Pusher {
    fn push_control(&mut self, message: msg::Control) {
        if self
            .dispatcher
            .send_control(
                self.queue_id,
                self.binding_id,
                intrusive::Entry::new(message),
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
        self.push_control(msg::Control::MaxData { maximum_data });
    }

    /// Receives one submitted burst.
    ///
    /// Tests that expect multiple submission cycles should call this helper
    /// again.
    async fn recv_frames(&mut self) -> intrusive::Queue<Frame> {
        core::future::poll_fn(|cx| self.frame_rx.poll_swap(cx, &mut self.frame_storage)).await;
        let mut combined_frames = intrusive::Queue::default();
        for (_priority, mut queue) in self.frame_storage.drain() {
            combined_frames.append(&mut queue);
        }
        combined_frames
    }

    async fn recv_frames_timeout(&mut self, duration: Duration) -> Option<intrusive::Queue<Frame>> {
        let queue = timeout(duration, self.recv_frames()).await.ok()?;
        if queue.is_empty() {
            None
        } else {
            Some(queue)
        }
    }

    fn complete_all(
        &mut self,
        mut frames: intrusive::Queue<Frame>,
        status: frame::TransmissionStatus,
    ) {
        while let Some(entry) = frames.pop_front() {
            let mut completed = entry.into_inner();
            let Some(sender) = completed.completion.take() else {
                continue;
            };
            completed.status = status;

            let mut queue = intrusive::Queue::new();
            queue.push_back(completed.into());
            sender
                .send_batch(queue)
                .expect("completion send should succeed in tests");
        }
    }
}

// ─── Bach async tests ─────────────────────────────────────────────────────────

#[test]
fn client_write_all_from_fin_sends_queue_init_with_early_data_and_fin() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one frame");
            let frame = frames.front().unwrap();
            assert!(matches!(
                frame.header,
                Header::QueueData {
                    is_fin: true,
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
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
fn write_msg_preserves_is_wakeup_flag() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected at least one frame");
            let mut queue_msg_count = 0usize;
            for frame in frames.iter() {
                match frame.header {
                    Header::QueueMsg { is_wakeup, .. } => {
                        queue_msg_count += 1;
                        assert!(
                            !is_wakeup,
                            "write_msg should preserve MsgFlags.is_wakeup=false when not flow-control constrained"
                        );
                    }
                    Header::QueueData { is_fin: true, .. } => {
                        // This task exits immediately after write_msg; dropping an
                        // open writer emits a best-effort trailing FIN frame.
                    }
                    _ => panic!("unexpected frame while validating QueueMsg: {:?}", frame.header),
                }
            }
            assert!(queue_msg_count > 0, "expected at least one QueueMsg frame");
        }
        .primary()
        .spawn();

        async move {
            let payload_len = writer.0.packet_size as usize + 1;
            let mut payload = Data::new(payload_len as u64);
            let written = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should succeed");
            assert_eq!(written, payload_len);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn write_msg_large_payload_uses_multiple_msg_segments() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;
        let chunk_size = writer.0.msg_packet_size as usize;
        let first_segment_size = crate::queue::msg_entry::MAX_CHUNKS as usize * chunk_size;
        let second_segment_size = chunk_size + 17;
        let payload_len = first_segment_size + second_segment_size;

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected QueueMsg frames");

            let mut first_msg_id = None::<u64>;
            let mut second_msg_id = None::<u64>;
            let mut first_next_chunk = 0u64;
            let mut second_next_chunk = 0u64;
            let mut first_chunk_count = 0usize;
            let mut second_chunk_count = 0usize;
            let mut first_fin_seen = false;
            let mut second_segment_has_non_fin_frame = false;
            let mut expected = Data::new(payload_len as u64);

            for frame in frames.iter() {
                let (msg_id, stream_offset, message_size, frame_chunk_size, chunk_index, is_fin) =
                    match frame.header {
                        Header::QueueMsg {
                            msg_id,
                            stream_offset,
                            message_size,
                            chunk_size,
                            chunk_index,
                            is_fin,
                            ..
                        } => (
                            msg_id.as_u64(),
                            stream_offset.as_u64(),
                            message_size.as_u64(),
                            chunk_size.as_u64(),
                            chunk_index.as_u64(),
                            is_fin,
                        ),
                        _ => panic!("expected QueueMsg frame, got {:?}", frame.header),
                    };

                if first_msg_id.is_none() {
                    first_msg_id = Some(msg_id);
                }

                if Some(msg_id) == first_msg_id {
                    assert_eq!(stream_offset, 0);
                    assert_eq!(message_size, first_segment_size as u64);
                    assert_eq!(frame_chunk_size, chunk_size as u64);
                    assert_eq!(chunk_index, first_next_chunk);
                    first_next_chunk += 1;
                    first_chunk_count += 1;
                    first_fin_seen |= is_fin;
                } else {
                    if second_msg_id.is_none() {
                        second_msg_id = Some(msg_id);
                    }
                    assert_eq!(Some(msg_id), second_msg_id, "unexpected third msg_id");
                    assert_eq!(stream_offset, first_segment_size as u64);
                    assert_eq!(message_size, second_segment_size as u64);
                    assert_eq!(frame_chunk_size, chunk_size as u64);
                    assert_eq!(chunk_index, second_next_chunk);
                    second_next_chunk += 1;
                    second_chunk_count += 1;
                    second_segment_has_non_fin_frame |= !is_fin;
                }

                for chunk in frame.payload.chunks() {
                    expected.receive(std::slice::from_ref(&chunk));
                }
            }

            assert!(second_msg_id.is_some(), "expected at least two msg_ids");
            assert_eq!(
                first_chunk_count,
                crate::queue::msg_entry::MAX_CHUNKS as usize,
                "first segment should fill MAX_CHUNKS"
            );
            assert_eq!(second_chunk_count, 2, "second segment should be two chunks");
            assert!(!first_fin_seen, "first segment should not carry FIN");
            assert!(
                !second_segment_has_non_fin_frame,
                "all frames in final segment should carry FIN"
            );
            assert!(
                expected.is_finished(),
                "payload should reassemble completely"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Data::new(payload_len as u64);
            let written = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("write_msg should succeed");
            assert_eq!(written, payload_len);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn control_reset_terminates_write() {
    sim(|| {
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
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one frame");
            let frame = frames.front().unwrap();
            assert!(matches!(
                frame.header,
                Header::QueueData {
                    is_fin: false,
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
            assert_eq!(frame.payload, &b"hello"[..]);

            // Give the app task a scheduling opportunity to attempt a second
            // write while Writer is still in `Status::QueueBindSent` (before any
            // remote MAX_DATA credit is injected).
            bach::task::yield_now().await;
            pusher.push_max_data(VarInt::from_u16(4096));

            let next = pusher.recv_frames().await;
            assert_eq!(next.len(), 1, "expected exactly one frame");
            let frame = next.front().unwrap();
            assert!(matches!(
                frame.header,
                Header::QueueData { is_fin: true, .. }
            ));
            assert_eq!(frame.payload, &b"!"[..]);
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
fn server_first_write_emits_queue_data_not_queue_init() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected at least one QueueData frame");
            pusher.assembler.push_queue_data(&frames);
            pusher.assembler.assert_payload(b"hello");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer
                .write_from(&mut payload)
                .await
                .expect("write should succeed");
            assert_eq!(written, 5);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_queue_control_budget_caps_transmitted_bytes() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::from_u8(3);

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected at least one QueueData frame");
            pusher.assembler.push_queue_data(&frames);
            pusher.assembler.assert_payload(b"abc");

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
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
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let init = pusher.recv_frames().await;
            assert_eq!(init.len(), 1, "expected exactly one QueueInit frame");
            let init_frame = init.front().unwrap();
            assert!(matches!(
                init_frame.header,
                Header::QueueData {
                    is_fin: false,
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
            assert_eq!(init_frame.payload, &b"abc"[..]);

            pusher.push_max_data(VarInt::from_u8(8));
            pusher.push_max_data(VarInt::from_u8(3));

            // The QueueInit consumed 3 bytes, so the assembler starts at offset 3.
            pusher.assembler.expected_offset = 3;
            let next = pusher.recv_frames().await;
            pusher.assembler.push_queue_data(&next);
            pusher.assembler.assert_payload(b"defgh");
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
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::from_u8(1);

        async move {
            let first = pusher.recv_frames().await;
            assert_eq!(first.len(), 1, "expected exactly one frame");
            let first_frame = first.front().unwrap();
            assert!(matches!(
                first_frame.header,
                Header::QueueData {
                    is_fin: false,
                    offset,
                    ..
                } if offset == VarInt::ZERO
            ));
            assert_eq!(first_frame.payload, &b"a"[..]);

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no frame while remote flow budget is exhausted"
            );

            pusher.push_max_data(VarInt::from_u8(2));

            let second = pusher.recv_frames().await;
            assert_eq!(second.len(), 1, "expected exactly one frame");
            let second_frame = second.front().unwrap();
            assert!(matches!(
                second_frame.header,
                Header::QueueData {
                    is_fin: true,
                    offset,
                    ..
                } if offset == VarInt::from_u8(1)
            ));
            assert_eq!(second_frame.payload, &b"b"[..]);
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
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let first = pusher.recv_frames().await;
            assert!(!first.is_empty(), "expected at least one frame");
            pusher.assembler.push_queue_data_init(&first);
            pusher.assembler.assert_payload(b"hi");
            pusher.assembler.assert_fin_count(1);

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no extra frame burst after FIN was already sent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hi");
            let written = writer
                .write_from_fin(&mut payload)
                .await
                .expect("fin write");
            assert_eq!(written, 2);
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_fin_write_then_drop_emits_no_extra_packet() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let first = pusher.recv_frames().await;
            assert!(!first.is_empty(), "expected at least one frame");
            pusher.assembler.push_queue_data(&first);
            pusher.assembler.assert_payload(b"hi");
            pusher.assembler.assert_fin_count(1);

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no extra frame burst after FIN was already sent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hi");
            let written = writer
                .write_from_fin(&mut payload)
                .await
                .expect("fin write");
            assert_eq!(written, 2);
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn transmission_error_completion_causes_broken_pipe_and_reset() {
    sim(|| {
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
                .expect("expected QueueReset after transmission failure");
            assert_eq!(reset.len(), 1, "expected exactly one reset frame");
            assert!(
                matches!(
                    reset.front().unwrap().header,
                    Header::QueueReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == error::RETRANSMISSIONS_EXHAUSTED
                ),
                "expected retransmission-exhausted QueueReset"
            );
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
    sim(|| {
        let (writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one FIN frame on drop");
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData {
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
    sim(|| {
        let (writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one reset frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == error::ABNORMAL_TERMINATION
                ),
                "expected QueueReset(Both, ABNORMAL_TERMINATION) when dropping during panic"
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

// ─── QueueInitReset / QueueInitFin tests ───────────────────────────────────────
//
// The tests below document the behavior of the QueueInitReset and QueueInitFin
// messages, which are sent when the client needs to notify the server while in
// QueueBindSent state (before MAX_DATA arrives and the server queue ID becomes known).
//
// QueueInitReset is for error/abnormal termination; QueueInitFin is for graceful close.

/// Panic drop while in QueueBindSent: QueueReset is sent so the server can
/// look up the stream via binding_id and terminate both queues.
#[test]
fn client_panic_drop_during_queue_init_sent_sends_queue_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // QueueInit arrives first
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
            ));

            // QueueReset follows on panic
            let reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueReset after panic drop");
            assert_eq!(reset.len(), 1);
            assert!(
                matches!(
                    reset.front().unwrap().header,
                    Header::QueueReset { error_code, .. } if error_code == error::ABNORMAL_TERMINATION
                ),
                "expected QueueReset(ABNORMAL_TERMINATION) with non-sentinel attempt_id on panic in QueueBindSent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());

            // yield so the pusher receives QueueInit before the panic triggers QueueReset
            bach::task::yield_now().await;

            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _writer = writer;
                panic!("intentional test panic during QueueBindSent");
            }));
            assert!(panic_result.is_err());
        }
        .primary()
        .spawn();
    });
}

/// TransmissionError during QueueBindSent: QueueReset is sent so the server
/// can look up the stream by binding_id and terminate the queues.
#[test]
fn client_transmission_error_during_queue_init_sent_sends_queue_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
            );

            // QueueReset should be emitted
            let reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueReset after TransmissionError in QueueBindSent");
            assert_eq!(reset.len(), 1);
            assert!(
                matches!(
                    reset.front().unwrap().header,
                    Header::QueueReset { error_code, .. } if error_code == error::RETRANSMISSIONS_EXHAUSTED
                ),
                "expected QueueReset(RETRANSMISSIONS_EXHAUSTED) with non-sentinel attempt_id"
            );
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
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// PeerDead completion failure during QueueBindSent: no QueueReset because the
/// peer is unreachable (it makes no difference whether we send it or not).
#[test]
fn client_peer_dead_during_queue_init_sent_no_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::PeerDead),
            );

            // PeerDead: no QueueReset (peer can't receive it)
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(extra.is_none(), "no QueueReset for PeerDead");
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
                .expect_err("expected TimedOut");
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// Dropping a client writer while in QueueBindSent (before MAX_DATA arrives):
/// the drop calls shutdown() → send_fin_packet() → send QueueData FIN(),
/// which sends QueueData FIN to let the server deliver EOF to the reader.
#[test]
fn client_drop_in_queue_init_sent_sends_queue_data_fin() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // QueueInit frame arrives first
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
            ));

            // QueueData FIN arrives when the writer is dropped
            let fin_frames = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueData FIN on drop in QueueBindSent");
            assert_eq!(fin_frames.len(), 1);
            assert!(
                matches!(
                    fin_frames.front().unwrap().header,
                    Header::QueueData { offset, is_fin: true, dest_acceptor_id: Some(_), .. } if offset == VarInt::from_u8(5)
                ),
                "expected QueueData FIN(offset=5) from normal drop in QueueBindSent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());
            // yield so pusher receives QueueInit before drop
            bach::task::yield_now().await;
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

/// PeerDead completion failure during QueueBindSent: no QueueReset because the
/// peer is unreachable (it makes no difference whether we send it or not).
#[test]
fn client_peer_dead_during_queue_init_sent_no_reset_silent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::PeerDead),
            );

            // No QueueReset emitted (remote_queue_id is None)
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(
                extra.is_none(),
                "no QueueReset possible during QueueBindSent (remote_queue_id unknown)"
            );
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
                .expect_err("expected TimedOut");
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// TransmissionError during QueueBindSent (legacy test, superseded by
/// client_transmission_error_during_queue_init_sent_sends_queue_reset).
/// This test verifies the application-facing error behavior remains correct.
#[test]
fn client_transmission_error_during_queue_init_sent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
            );

            // QueueInitReset is emitted; consume it so the test doesn't leave stale frames.
            let _reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueReset");
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
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// Control channel closed while writer is in QueueBindSent (endpoint died).
/// Next write attempt returns ConnectionReset.
#[test]
fn client_control_channel_closed_during_queue_init_sent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // Wait for the QueueInit frame (proves writer reached QueueBindSent)
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData {
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
            // Close the dispatch (simulates endpoint eviction)
            pusher.dispatcher.close(&mut |_| {});
            drop(pusher);
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());

            // Next write detects closed control channel
            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected error from closed channel");
            assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// Explicit shutdown() while in QueueBindSent: send_fin_packet() now sends a
/// QueueInitFin so the server can deliver EOF to the reader at the correct offset.
#[test]
fn client_shutdown_during_queue_init_sent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
            ));

            // QueueData FIN arrives after explicit shutdown()
            let fin_frames = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueData FIN after shutdown in QueueBindSent");
            assert_eq!(fin_frames.len(), 1);
            assert!(
                matches!(
                    fin_frames.front().unwrap().header,
                    Header::QueueData { offset, is_fin: true, dest_acceptor_id: Some(_), .. } if offset == VarInt::from_u8(5)
                ),
                "expected QueueData FIN(offset=5) after explicit shutdown() in QueueBindSent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());

            // yield so pusher receives QueueInit before shutdown sends QueueInitFin
            bach::task::yield_now().await;

            writer.shutdown().expect("shutdown should succeed");
            assert!(writer.0.status.is_shutdown());

            // Writes after shutdown return BrokenPipe
            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected BrokenPipe after shutdown");
            assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}

/// Empty-buffer write_from_fin sends QueueInit with empty payload and FIN=true.
/// Tests the Init → QueueBindSent → FinSent transition in a single call.
#[test]
fn client_empty_fin_queue_init() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            let frame = frames.front().unwrap();
            assert!(matches!(
                frame.header,
                Header::QueueData {
                    is_fin: true,
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
            assert!(frame.payload.is_empty(), "expected empty payload with FIN");

            // No additional frame on drop (FIN already sent)
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(extra.is_none(), "no extra frame after FIN already sent");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::new();
            let written = writer
                .write_from_fin(&mut payload)
                .await
                .expect("empty fin write");
            assert_eq!(written, 0);
            assert!(writer.0.status.is_fin_sent());
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

/// Server writer receives Reset while in Open state (actively writing).
/// Existing test only covers client during QueueBindSent.
#[test]
fn server_reset_received_while_open() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let _first = pusher.recv_frames().await;
            // Now inject reset
            pusher.push_reset(VarInt::from_u8(33));
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut more = Bytes::from_static(b"world");
            let err = writer
                .write_from(&mut more)
                .await
                .expect_err("expected ConnectionReset");
            assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// PeerDead completion failure surfaces TimedOut error to the application.
#[test]
fn client_peer_dead_completion_surfaces_timed_out() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::PeerDead),
            );

            // PeerDead does not emit QueueReset (peer can't receive it)
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(extra.is_none(), "no QueueReset for PeerDead");
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
                .expect_err("expected TimedOut");
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// UnknownPathSecret completion failure surfaces ConnectionRefused.
#[test]
fn server_unknown_path_secret_completion_surfaces_connection_refused() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::UnknownPathSecret),
            );

            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(extra.is_none(), "no QueueReset for UnknownPathSecret");
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
                .expect_err("expected ConnectionRefused");
            assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// After explicit shutdown(), subsequent writes return BrokenPipe (not
/// ConnectionReset — no reset_error_code stored for graceful shutdown).
#[test]
fn client_write_after_shutdown_returns_broken_pipe() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            // Consume the FIN frame from shutdown
            let _frames = pusher.recv_frames().await;
        }
        .primary()
        .spawn();

        async move {
            writer.shutdown().expect("shutdown should succeed");
            assert!(writer.0.status.is_shutdown());

            let mut payload = Bytes::from_static(b"hello");
            let err = writer
                .write_from(&mut payload)
                .await
                .expect_err("expected BrokenPipe after shutdown");
            assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}

/// Local inflight budget (max_inflight_bytes) caps how much data the writer
/// can send before completions free up space.
#[test]
fn server_local_inflight_budget_blocks_write() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.max_inflight_bytes = 3;

        async move {
            let first = pusher.recv_frames().await;
            pusher.assembler.push_queue_data(&first);
            pusher.assembler.assert_payload(b"abc");

            // Writer is blocked — no more frames
            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "writer should block when local inflight budget is exhausted"
            );

            // Free up budget via completions
            pusher.complete_all(first, frame::TransmissionStatus::Acknowledged);

            // Writer unblocks and sends more data
            let second = pusher.recv_frames().await;
            pusher.assembler.push_queue_data(&second);
            pusher.assembler.assert_payload(b"abcdef");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"abcdef");
            let first = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(first, 3, "capped by local inflight budget");

            let second = writer.write_from(&mut payload).await.expect("second write");
            assert_eq!(second, 3, "freed budget allows more data");
        }
        .primary()
        .spawn();
    });
}

/// Calling shutdown() multiple times is idempotent: only one FIN frame emitted.
#[test]
fn server_shutdown_idempotent() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData {
                    is_fin: true,
                    offset,
                    ..
                } if offset == VarInt::ZERO
            ));

            // No second FIN frame
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(
                extra.is_none(),
                "no duplicate FIN from second shutdown call"
            );
        }
        .primary()
        .spawn();

        async move {
            writer.shutdown().expect("first shutdown");
            assert!(writer.0.status.is_shutdown());
            writer.shutdown().expect("second shutdown is no-op");
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// Panic drop during QueueBindSent (legacy test, superseded by
/// client_panic_drop_during_queue_init_sent_sends_queue_reset).
/// This verifies that QueueInitReset can be sent even when panicking, and that
/// the frame observable from the test matches expectations.
#[test]
fn client_panic_drop_during_queue_init_sent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // QueueInit arrives
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
            ));

            // QueueInitReset arrives after panic
            let reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueReset on panic in QueueBindSent");
            assert_eq!(reset.len(), 1);
            assert!(
                matches!(
                    reset.front().unwrap().header,
                    Header::QueueReset { error_code, .. } if error_code == error::ABNORMAL_TERMINATION
                ),
                "expected QueueReset(ABNORMAL_TERMINATION) with non-sentinel attempt_id"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());

            // yield so the pusher receives QueueInit before the panic triggers QueueReset
            bach::task::yield_now().await;

            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _writer = writer;
                panic!("intentional test panic during QueueBindSent");
            }));
            assert!(panic_result.is_err());
        }
        .primary()
        .spawn();
    });
}

/// Client sends early data in QueueBindSent then requests graceful close via
/// write_from_fin() — the second call sees QueueBindSent and emits QueueInitFin.
///
/// This covers the scenario: client writes some data in the QueueInit (is_fin=false)
/// and then signals the end of the write side before MAX_DATA arrives from the server.
#[test]
fn client_write_from_fin_in_queue_init_sent_sends_queue_data_fin() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // First batch: QueueInit with early data, no FIN
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
                ),
                "expected QueueInit(is_fin=false) with early data"
            );

            // Second batch: QueueInitFin at the correct write offset
            let fin_frames = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueData FIN after write_from_fin in QueueBindSent");
            assert_eq!(fin_frames.len(), 1);
            assert!(
                matches!(
                    fin_frames.front().unwrap().header,
                    Header::QueueData { offset, is_fin: true, dest_acceptor_id: Some(_), .. } if offset == VarInt::from_u8(5)
                ),
                "expected QueueData FIN(offset=5) — offset must equal early data length"
            );
        }
        .primary()
        .spawn();

        async move {
            // First write: early data in QueueInit, no FIN
            let mut data = Bytes::from_static(b"hello");
            let n = writer.write_from(&mut data).await.expect("first write");
            assert_eq!(n, 5);
            assert!(writer.0.status.is_init_sent());

            // yield so the pusher can receive QueueInit before we send QueueInitFin
            bach::task::yield_now().await;

            // Graceful close — MAX_DATA not received yet
            let mut empty = Bytes::new();
            let n = writer
                .write_from_fin(&mut empty)
                .await
                .expect("write_from_fin should succeed");
            assert_eq!(n, 0, "no additional payload");
            // FIN was sent; status transitions to FinSent (not Shutdown until drop)
            assert!(writer.0.status.is_fin_sent());
        }
        .primary()
        .spawn();
    });
}

// ─── write_msg tests ───────────────────────────────────────────────────────────
//
// The tests below cover the write_msg API surface:
//   * empty-buffer paths (no-FIN and FIN variants)
//   * small-payload routing to QueueData instead of QueueMsg
//   * flow-control blocking and unblocking via MAX_DATA
//   * error propagation after remote reset, explicit shutdown, and FIN-already-sent
//   * client-side init-state (QueueData-init) and subsequent blocking in InitSent
//   * cooperative yielding: write_msg yields after BUDGET consecutive completions

/// write_msg with an empty buffer and is_fin=false returns 0 without emitting
/// any frame. The caller may call again with actual data later.
#[test]
fn write_msg_empty_buffer_no_fin_returns_zero_without_frames() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;

        async move {
            // A drop-triggered FIN is the only frame that should appear.
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueData { is_fin: true, .. }
                ),
                "expected only the drop-FIN frame"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut empty: Bytes = Bytes::new();
            let written = writer
                .write_msg(
                    &mut empty,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg on empty buf should succeed");
            assert_eq!(written, 0);
        }
        .primary()
        .spawn();
    });
}

/// write_msg with an empty buffer and is_fin=true sends a QueueData FIN frame.
#[test]
fn write_msg_empty_buffer_with_fin_emits_fin_frame() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one FIN frame");
            let frame = frames.front().unwrap();
            assert!(
                matches!(
                    frame.header,
                    Header::QueueData {
                        is_fin: true,
                        offset,
                        ..
                    } if offset == VarInt::ZERO
                ),
                "expected QueueData FIN at offset 0"
            );
            assert!(
                frame.payload.is_empty(),
                "FIN frame payload should be empty"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut empty: Bytes = Bytes::new();
            let written = writer
                .write_msg(
                    &mut empty,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg with empty buf + FIN should succeed");
            assert_eq!(written, 0);
        }
        .primary()
        .spawn();
    });
}

/// A payload that fits within a single packet_size threshold is routed to a
/// QueueData frame, not a QueueMsg frame. This avoids MsgTable overhead for
/// small messages.
#[test]
fn write_msg_small_payload_routes_to_queue_data_frame() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;
        // Any payload at or below packet_size triggers the QueueData fast-path.
        let small_payload = vec![0xABu8; writer.0.packet_size as usize];

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected at least one frame");
            for frame in frames.iter() {
                assert!(
                    matches!(frame.header, Header::QueueData { .. }),
                    "small payload should produce QueueData frames, got {:?}",
                    frame.header
                );
            }
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from(small_payload);
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should succeed");
        }
        .primary()
        .spawn();
    });
}

/// write_msg blocks (returns Pending) when the remote flow budget is zero, and
/// unblocks once MAX_DATA arrives from the peer.
#[test]
fn server_write_msg_blocks_when_remote_budget_zero() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        // Override to zero so the first write can't proceed.
        writer.0.remote_max_data = VarInt::ZERO;

        async move {
            // No frame should arrive while budget is zero.
            let early = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                early.is_none(),
                "expected no frame while remote flow budget is zero"
            );

            // Grant budget; the blocked write_msg should now complete.
            pusher.push_max_data(VarInt::from_u16(4096));

            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected frames after MAX_DATA");
        }
        .primary()
        .spawn();

        async move {
            let slot = writer.0.slot_ptr();
            let mut payload = Bytes::from_static(b"hello");

            // First poll must return Pending because remote budget is zero.
            let blocked = core::future::poll_fn(|cx| {
                let mut buf = Bytes::from_static(b"hello");
                match writer.0.poll_write_msg(
                    cx,
                    slot,
                    &mut buf,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                ) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                }
            })
            .await;
            assert!(blocked, "expected write_msg to block when budget is zero");

            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should succeed after MAX_DATA");
        }
        .primary()
        .spawn();
    });
}

/// write_msg blocks when remote budget is exhausted, and resumes after MAX_DATA
/// raises the flow-control window.
#[test]
fn server_write_msg_unblocks_after_max_data() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::ZERO;

        async move {
            // First wait confirms nothing arrives while budget is 0.
            let no_frames = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(no_frames.is_none(), "no frame expected before MAX_DATA");

            pusher.push_max_data(VarInt::from_u16(8192));

            let frames = pusher.recv_frames().await;
            assert!(
                !frames.is_empty(),
                "expected frames after MAX_DATA unblocks write_msg"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"unblocked");
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should complete after MAX_DATA");
        }
        .primary()
        .spawn();
    });
}

/// write_msg returns ConnectionReset when a remote Reset arrives before the
/// write can complete.
#[test]
fn write_msg_after_remote_reset_returns_connection_reset() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::ZERO;

        async move {
            pusher.push_reset(VarInt::from_u8(42));
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"data");
            let err = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect_err("expected ConnectionReset");
            assert_eq!(
                err.kind(),
                io::ErrorKind::ConnectionReset,
                "expected ConnectionReset, got {err:?}"
            );
        }
        .primary()
        .spawn();
    });
}

/// write_msg returns BrokenPipe after the writer has been explicitly shut down.
#[test]
fn write_msg_after_shutdown_returns_broken_pipe() {
    sim(|| {
        let (mut writer, pusher) = make_server_pair();

        async move {
            // Keep the pusher alive so the frame channel stays open; shutdown()
            // sends a FIN frame and would fail with "frame channel closed" if
            // the receiver were dropped before the task runs.
            let _pusher = pusher;
            writer.shutdown().expect("shutdown should succeed");
            let mut payload = Bytes::from_static(b"data");
            let err = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect_err("expected BrokenPipe after shutdown");
            assert_eq!(
                err.kind(),
                io::ErrorKind::BrokenPipe,
                "expected BrokenPipe after shutdown, got {err:?}"
            );
        }
        .primary()
        .spawn();
    });
}

/// write_msg returns BrokenPipe after a FIN has already been sent.
#[test]
fn write_msg_after_fin_sent_returns_broken_pipe() {
    sim(|| {
        let (mut writer, _pusher) = make_server_pair();

        async move {
            // Force FinSent state by marking it directly.
            writer.0.status.on_send_fin().ok();
            assert!(writer.0.status.is_fin_sent());

            let mut payload = Bytes::from_static(b"data");
            let err = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect_err("expected BrokenPipe after fin sent");
            assert_eq!(
                err.kind(),
                io::ErrorKind::BrokenPipe,
                "expected BrokenPipe after FinSent, got {err:?}"
            );
        }
        .primary()
        .spawn();
    });
}

/// Client write_msg sends a QueueData-init frame on the first write (the
/// bootstrap message that lets the server know which acceptor queue to use and
/// triggers it to send MAX_DATA), then blocks on any subsequent write while in
/// InitSent waiting for the server to confirm the stream.
///
/// Crucially, the client starts with `remote_max_data = VarInt::ZERO` (zero
/// flow-control window), yet the first write still completes immediately — the
/// init packet is always sent unconditionally so the stream can be established.
/// See also `client_write_msg_zero_window_does_not_block_init`.
#[test]
fn client_write_msg_first_write_sends_queue_data_init_then_blocks() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();
        // Clients always start with a zero remote window — the init frame must
        // bypass this restriction.
        assert_eq!(
            writer.0.remote_max_data,
            VarInt::ZERO,
            "client must start with zero remote budget"
        );

        async move {
            // The bootstrap init frame(s) must arrive.
            let frames = pusher.recv_frames().await;
            let has_init = frames.iter().any(|f| {
                matches!(
                    f.header,
                    Header::QueueData {
                        dest_acceptor_id: Some(_),
                        ..
                    } | Header::QueueMsg {
                        dest_acceptor_id: Some(_),
                        ..
                    }
                )
            });
            assert!(
                has_init,
                "expected at least one init frame with dest_acceptor_id"
            );

            // No further frames — writer is blocked in InitSent until MAX_DATA.
            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no further frames while in InitSent"
            );
        }
        .primary()
        .spawn();

        async move {
            let slot = writer.0.slot_ptr();
            // Small payload (≤ packet_size) → send_queue_data_init fast-path.
            // The write completes immediately despite the zero window because the
            // init frame is always sent unconditionally.
            let payload_len = writer.0.packet_size as usize;
            let mut buf = Data::new(payload_len as u64);
            let result = core::future::poll_fn(|cx| {
                writer.0.poll_write_msg(
                    cx,
                    slot,
                    &mut buf,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
            })
            .await;
            assert!(result.is_ok(), "first write should succeed: {result:?}");
            assert!(
                writer.0.status.is_init_sent(),
                "writer should be in InitSent after first write"
            );

            // A second write call must block: we are now in InitSent, waiting
            // for the server's MAX_DATA before more data can be sent.
            let blocked = core::future::poll_fn(|cx| {
                let mut buf2 = Data::new(1);
                match writer.0.poll_write_msg(
                    cx,
                    slot,
                    &mut buf2,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                ) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                }
            })
            .await;
            assert!(blocked, "second write_msg should block while in InitSent");
        }
        .primary()
        .spawn();
    });
}

/// Client write_msg does NOT block on the first write even when the remote
/// flow-control window is zero.
///
/// Servers correctly block when their budget is zero (see
/// `server_write_msg_blocks_when_remote_budget_zero`), but clients in the Init
/// state must always be able to send the bootstrap packet that establishes the
/// stream and triggers the server to open a flow-control window.
#[test]
fn client_write_msg_zero_window_does_not_block_init() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();
        // Clients always start with zero remote budget.
        assert_eq!(writer.0.remote_max_data, VarInt::ZERO);

        async move {
            // The init frame must arrive despite the zero window.
            let frames = pusher.recv_frames().await;
            assert!(
                !frames.is_empty(),
                "client must send init frame even with zero window"
            );
            let has_init = frames.iter().any(|f| {
                matches!(
                    f.header,
                    Header::QueueData {
                        dest_acceptor_id: Some(_),
                        ..
                    } | Header::QueueMsg {
                        dest_acceptor_id: Some(_),
                        ..
                    }
                )
            });
            assert!(has_init, "init frame must carry dest_acceptor_id");
        }
        .primary()
        .spawn();

        async move {
            let slot = writer.0.slot_ptr();
            let mut payload = Bytes::from_static(b"hello");
            // First write should complete (not block) even with zero window.
            let result = core::future::poll_fn(|cx| {
                writer.0.poll_write_msg(
                    cx,
                    slot,
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
            })
            .await;
            assert!(
                result.is_ok(),
                "client first write must succeed despite zero window: {result:?}"
            );
        }
        .primary()
        .spawn();
    });
}

/// write_msg yields cooperatively when the inner send_msg loop exhausts the
/// coop budget, giving other tasks a chance to run even under continuous write
/// pressure with unlimited credits.
#[test]
#[ignore = "needs endpoint-level harness to exercise multi-segment coop yielding"]
fn write_msg_coop_yields_after_budget_completions() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;

        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();

        // Competing task: marks itself as having run, then keeps yielding.
        async move {
            ran_clone.store(true, Ordering::Relaxed);
            loop {
                bach::task::yield_now().await;
            }
        }
        .spawn();

        // Frame-drainer task: keeps the frame channel from filling up.
        async move {
            loop {
                let _ = pusher.recv_frames().await;
            }
        }
        .spawn();

        async move {
            // Use a payload large enough to require >128 segments so the inner
            // coop check in send_msg's loop fires. Each segment is max_segment_size
            // (~340KB at MTU 1472), so we need 128 * 340KB ≈ 43MB. Use a large
            // Data buffer to avoid actual allocation of that much memory.
            let chunk_size = writer.0.msg_packet_size as usize;
            let max_segment_size = crate::queue::msg_entry::MAX_CHUNKS as usize * chunk_size;
            let payload_len = max_segment_size * 130;
            let mut payload = Data::new(payload_len as u64);
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should succeed");
            assert!(
                ran.load(Ordering::Relaxed),
                "competing task should have run during write_msg coop yield"
            );
        }
        .primary()
        .spawn();
    });
}

/// BUG REPRODUCTION: Client write_msg with a payload just above packet_size
/// splits into a 2-chunk QueueMsg segment. During Init (force_first=true),
/// only chunk 0 is sent. After MAX_DATA unblocks the writer, `send_msg` is
/// called with the remaining bytes. If those remaining bytes fit within
/// packet_size, the size-based routing early-return at the top of `send_msg`
/// routes them to `send_data` (QueueData) instead of resuming the pending
/// QueueMsg segment. The receiver's MsgTable entry never completes because
/// chunk 1 never arrives via QueueMsg — the data arrives via QueueData at
/// the wrong offset, and the MsgTable is permanently stuck.
#[test]
fn client_write_msg_partial_segment_resume_must_use_queue_msg_not_queue_data() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();
        // Payload just above packet_size so it takes the QueueMsg path but
        // produces exactly 2 chunks: first chunk = msg_packet_size, second chunk
        // = packet_size + 1 - msg_packet_size (which is < packet_size).
        let payload_len = writer.0.packet_size as usize + 1;

        async move {
            // First batch: the init bootstrap frame(s). Should contain at least
            // one QueueMsg with chunk_index=0.
            let frames = pusher.recv_frames().await;
            let mut saw_queue_msg_chunk_0 = false;
            for frame in frames.iter() {
                match frame.header {
                    Header::QueueMsg {
                        chunk_index,
                        dest_acceptor_id: Some(_),
                        ..
                    } => {
                        assert_eq!(
                            chunk_index.as_u64(),
                            0,
                            "init should only send chunk_index=0"
                        );
                        saw_queue_msg_chunk_0 = true;
                    }
                    Header::QueueData {
                        dest_acceptor_id: Some(_),
                        ..
                    } => {
                        // Small-payload init path — this test requires a
                        // payload above packet_size, so this shouldn't happen.
                        panic!("expected QueueMsg init (payload > packet_size), got QueueData");
                    }
                    _ => panic!("unexpected frame during init: {:?}", frame.header),
                }
            }
            assert!(
                saw_queue_msg_chunk_0,
                "expected QueueMsg chunk_index=0 during init"
            );

            // Grant MAX_DATA so the writer can resume the pending segment.
            pusher.push_max_data(VarInt::from_u16(4096));

            // Second batch: the resumed segment. This MUST contain QueueMsg
            // chunk_index=1, NOT a QueueData frame.
            let resume_frames = pusher
                .recv_frames_timeout(Duration::from_secs(5))
                .await
                .expect("expected resumed frames after MAX_DATA");

            let mut saw_queue_msg_chunk_1 = false;
            for frame in resume_frames.iter() {
                match frame.header {
                    Header::QueueMsg { chunk_index, .. } => {
                        assert_eq!(
                            chunk_index.as_u64(),
                            1,
                            "resumed segment should send chunk_index=1"
                        );
                        saw_queue_msg_chunk_1 = true;
                    }
                    Header::QueueData { .. } => {
                        panic!(
                            "BUG: remaining bytes routed to QueueData instead of \
                             resuming QueueMsg segment (pending_chunk_index was ignored)"
                        );
                    }
                    _ => {}
                }
            }
            assert!(
                saw_queue_msg_chunk_1,
                "expected QueueMsg chunk_index=1 after resume, but it never arrived"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Data::new(payload_len as u64);
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("write_msg should succeed");
        }
        .primary()
        .spawn();
    });
}

/// write_msg with FIN on a large multi-segment payload marks FIN only on the
/// last chunk of the last segment, not on intermediate segments.
///
/// This complements `write_msg_large_payload_uses_multiple_msg_segments` by
/// focusing on is_fin placement rather than segment structure.
#[test]
fn write_msg_fin_only_on_last_chunk_of_last_segment() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;
        let chunk_size = writer.0.msg_packet_size as usize;
        // Two full segments: first has MAX_CHUNKS chunks, second has one extra.
        let first_segment = crate::queue::msg_entry::MAX_CHUNKS as usize * chunk_size;
        let second_segment = chunk_size;
        let payload_len = first_segment + second_segment;

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty());

            let mut fin_count = 0usize;
            let mut non_fin_before_fin = false;
            let mut last_was_fin = false;

            for frame in frames.iter() {
                match frame.header {
                    Header::QueueMsg { is_fin, msg_id, .. } => {
                        if last_was_fin {
                            panic!(
                                "frame after FIN: msg_id={} is_fin={}",
                                msg_id.as_u64(),
                                is_fin
                            );
                        }
                        if is_fin {
                            fin_count += 1;
                            last_was_fin = true;
                        } else {
                            non_fin_before_fin = true;
                        }
                    }
                    _ => panic!("expected QueueMsg, got {:?}", frame.header),
                }
            }

            assert_eq!(fin_count, 1, "exactly one FIN-bearing chunk expected");
            assert!(
                non_fin_before_fin,
                "expected non-FIN chunks before the FIN chunk"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Data::new(payload_len as u64);
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg with fin should succeed");
        }
        .primary()
        .spawn();
    });
}

// ── Credit-pool integration ─────────────────────────────────────────────────

/// Build a credit pool with a custom capacity. Per-priority caps default to capacity so a single
/// acquire can drain the whole pool.
fn test_credit_pool(capacity: u64) -> crate::sync::Arc<crate::credit::Pool> {
    crate::sync::Arc::new(crate::credit::Pool::new(crate::credit::Config {
        capacity,
        max_single_acquire: [capacity; crate::credit::Priority::LEVELS],
    }))
}

/// Sum the `flow_credits` field across every frame in `queue`.
fn sum_flow_credits(queue: &intrusive::Queue<Frame>) -> u64 {
    queue.iter().map(|f| f.flow_credits).sum()
}

/// Steady-state: every byte the writer sends is attributed to a `Frame.flow_credits` field
/// equal to its payload size. The test simulates the production assembler by releasing each
/// admitted frame's credits back to the pool, then asserts that the pool counters reconcile:
/// `acquire_bytes == release_bytes` across the lifecycle.
#[test]
fn credit_pool_round_trip_attaches_credits_and_admit_restores_capacity() {
    sim(|| {
        const CAPACITY: u64 = 16 * 1024;
        let pool = test_credit_pool(CAPACITY);
        let (mut writer, mut pusher) = PairBuilder::default()
            .with_credit_pool(pool.clone())
            .build();

        // Distributor folds `returned` back into `available` when it polls; spawning it lets us
        // assert against `debug_available` after a few yield points.
        let dist = crate::credit::Distributor::new(pool.clone());
        async move {
            use crate::socket::channel::Budget;
            dist.distribute(Budget::new(1 << 20), TestWakerSink).await;
        }
        .spawn();

        let pool_for_pusher = pool.clone();
        async move {
            // Init frame round-trip: payload is bounded by packet_size.
            let init = pusher.recv_frames().await;
            assert_eq!(init.len(), 1, "expected single init frame");
            let init_credits = sum_flow_credits(&init);
            assert_eq!(
                init_credits, 5,
                "init frame should carry exactly the bytes it transports"
            );
            // Simulate the assembler admitting the frame: release its credits back to the pool.
            pool_for_pusher.release(init_credits);

            // Steady-state writes after the server's MAX_DATA: each frame's flow_credits matches
            // its payload size; admit them all by releasing.
            pusher.push_max_data(VarInt::from_u32(8 * 1024));
            let steady = pusher.recv_frames().await;
            assert!(!steady.is_empty(), "expected steady-state frames");
            for frame in steady.iter() {
                assert_eq!(
                    frame.flow_credits,
                    frame.payload.len() as u64,
                    "every data frame must carry credits equal to its payload size"
                );
            }
            pool_for_pusher.release(sum_flow_credits(&steady));
        }
        .primary()
        .spawn();

        let pool_for_writer = pool.clone();
        async move {
            let mut hello = Bytes::from_static(b"hello");
            writer
                .write_from(&mut hello)
                .await
                .expect("init write succeeds");

            let mut bulk = Data::new(8 * 1024);
            writer.write_from(&mut bulk).await.expect("bulk write");

            drop(writer);

            // Yield so the distributor folds the released credits back into `available`.
            for _ in 0..4 {
                bach::task::yield_now().await;
            }

            // All credits acquired by the writer have been either (a) released by the simulated
            // admit path on the Pusher side, or (b) released by `WriterAllocPtr::drop` for any
            // bytes the writer was still holding when it went away.
            assert_eq!(
                pool_for_writer.debug_available(),
                CAPACITY as i64,
                "acquire/release must net to zero across the lifecycle"
            );
        }
        .primary()
        .spawn();
    });
}

/// A second poll on a writer whose pool is exhausted parks. After the pool is replenished the
/// distributor wakes the slot and the next poll proceeds. Covers the LINKED→APP grant path.
#[test]
fn credit_pool_parked_write_unblocks_after_release() {
    sim(|| {
        // Capacity holds exactly one bulk write. The first bulk drains the pool; the second
        // parks until the pusher simulates an admit by releasing credits.
        const CAPACITY: u64 = 128;
        let pool = test_credit_pool(CAPACITY);

        // Distributor task — runs in the background, must not block runtime termination.
        let dist = crate::credit::Distributor::new(pool.clone());
        async move {
            use crate::socket::channel::Budget;
            dist.distribute(Budget::new(1 << 20), TestWakerSink).await;
        }
        .spawn();

        let (mut writer, mut pusher) = PairBuilder::default()
            .with_credit_pool(pool.clone())
            .build();

        let pool_for_pusher = pool.clone();
        async move {
            // Init frame: simulate the assembler admitting it (release its credits).
            let init = pusher.recv_frames().await;
            pool_for_pusher.release(sum_flow_credits(&init));

            // Open the steady-state window.
            pusher.push_max_data(VarInt::from_u32(4 * CAPACITY as u32));

            // First bulk arrives but stays in the channel — admit only when we want to wake the
            // parked second bulk.
            let first = pusher.recv_frames().await;
            assert!(!first.is_empty(), "first bulk should produce frames");

            // Pool is now drained (debited by `first`'s flow_credits, not yet released). Release
            // them — this wakes the parked writer task.
            pool_for_pusher.release(sum_flow_credits(&first));

            let second = pusher.recv_frames().await;
            assert!(!second.is_empty(), "second batch must arrive after release");
        }
        .primary()
        .spawn();

        async move {
            let mut first = Bytes::from_static(b"hi");
            writer.write_from(&mut first).await.expect("init write");

            // First bulk: takes the entire steady-state capacity.
            let mut bulk = Data::new(CAPACITY);
            writer
                .write_from(&mut bulk)
                .await
                .expect("first bulk write");

            // Second bulk: parks because the pool is drained until the pusher releases.
            let mut more = Data::new(CAPACITY);
            writer
                .write_from(&mut more)
                .await
                .expect("second bulk write completes after release");
        }
        .primary()
        .spawn();
    });
}

/// Dropping a writer while its credit slot is parked must transfer ownership of the slot
/// allocation to the pool via `Slot::abandon`. The pool reclaims the slot on its next pass and
/// invokes `drop_writer_alloc` to free the heap. This exercises the LINKED→DEAD branch of
/// `WriterAllocPtr::drop`.
#[test]
fn parked_writer_drop_transfers_ownership_to_pool() {
    sim(|| {
        // Capacity = 2 covers the init's "hi" exactly; any subsequent acquire parks because the
        // pool is drained.
        const CAPACITY: u64 = 2;
        let pool = test_credit_pool(CAPACITY);

        let dist = crate::credit::Distributor::new(pool.clone());
        async move {
            use crate::socket::channel::Budget;
            dist.distribute(Budget::new(1 << 20), TestWakerSink).await;
        }
        .spawn();

        let (mut writer, mut pusher) = PairBuilder::default()
            .with_credit_pool(pool.clone())
            .build();

        async move {
            let _init = pusher.recv_frames().await;
            // Open the steady-state window so the writer's next acquire isn't gated by
            // `min_send_budget==0`. We do NOT release the init credits — the pool stays drained.
            pusher.push_max_data(VarInt::from_u32(1024));
            // Hold the pusher so its task keeps running while the writer parks and then drops.
            // Without this, the pusher task ends and the runtime tears down before the writer
            // gets a chance to park.
            for _ in 0..8 {
                bach::task::yield_now().await;
            }
        }
        .primary()
        .spawn();

        async move {
            let mut hello = Bytes::from_static(b"hi");
            writer.write_from(&mut hello).await.expect("init write");

            // Yield so the pusher's `push_max_data` lands and the writer's status becomes Open.
            for _ in 0..2 {
                bach::task::yield_now().await;
            }

            // The pool has zero credits available now (init took both bytes and the pusher
            // didn't release them). The next write parks the slot.
            let mut bulk = Data::new(64);
            let parked =
                core::future::poll_fn(|cx| match writer.poll_write_from(cx, &mut bulk, false) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                })
                .await;
            assert!(parked, "expected the bulk write to park on credit acquire");

            // Drop the writer while its slot is LINKED. `Slot::abandon` succeeds; the
            // distributor's next pass observes the dead slot and frees the allocation via
            // `drop_writer_alloc`. The test's success criterion is "no leak/panic at drop"; the
            // bach runtime tears down cleanly because the distributor handles the dead slot.
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

/// `set_priority` updates the writer's priority field and is reflected by the `priority()` getter.
#[test]
fn priority_getter_reflects_set_priority() {
    let (mut writer, _pusher) = PairBuilder::default()
        .with_priority(crate::credit::Priority::Low)
        .build();
    assert_eq!(writer.priority(), crate::credit::Priority::Low);
    writer.set_priority(crate::credit::Priority::High);
    assert_eq!(writer.priority(), crate::credit::Priority::High);
}

/// Inline `WakerSink` that wakes every batched waker immediately. Mirrors the production sink's
/// contract: after `append_wakers` returns, the batch must be empty.
struct TestWakerSink;

impl crate::credit::WakerSink for TestWakerSink {
    fn append_wakers(&mut self, batch: &mut std::collections::VecDeque<core::task::Waker>) {
        for w in batch.drain(..) {
            w.wake();
        }
    }
}
