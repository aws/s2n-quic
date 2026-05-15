// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for the stream3 Reader.
//!
//! ## Organization
//!
//! * **Synchronous unit tests** – exercise `write_data_reader` directly without
//!   an async runtime or task scheduler; these test an internal helper function
//!   in isolation.
//!
//! * **Bach async tests** – each test runs inside `crate::testing::sim` and uses
//!   **two separate primary tasks** to model how a real application and endpoint
//!   interact:
//!
//!   * **Application task** (primary) – owns the [`Reader`]; calls `read_into`
//!     and asserts on the data it receives.
//!   * **Endpoint task** (primary) – owns the [`Pusher`]; sends [`msg::Stream`]
//!     messages into the flow queue and asserts on [`Frame`]s the Reader emits
//!     (e.g. `MAX_DATA`, `STOP_SENDING`).
//!
//!   Both tasks are marked `.primary()` so the sim runs until both complete.
//!   The two sides talk over the real flow-queue / frame-submission channels,
//!   without any actual UDP sockets or cryptography.

use super::{msg, reset_error, write_data_reader, ReadToEnd, Reader};
use crate::{
    flow, intrusive_queue,
    packet::{control, datagram::ResetTarget},
    path::secret::map::Entry as PathSecretEntry,
    stream3::frame::{self, Frame, Header, PriorityStorage, SubmissionReceiver},
};
use bytes::BytesMut;
use s2n_quic_core::{
    buffer::{writer::Storage as _, Reassembler},
    endpoint,
    frame::FrameMut,
    stream::testing::Data,
    varint::VarInt,
};
use std::{net::SocketAddr, time::Duration};

// ─── Test helpers ─────────────────────────────────────────────────────────────

/// Creates a connected `(Reader, Pusher)` pair for use in tests.
///
/// * `Reader` – the component under test; owns the stream-side receive handle.
/// * `Pusher` – the mock endpoint side; can inject stream messages and receive
///   outbound frames submitted by the Reader (e.g. `MAX_DATA`).
fn make_pair() -> (Reader, Pusher) {
    make_pair_with_type(endpoint::Type::Client)
}

/// Creates a server-side `(Reader, Pusher)` pair (starts in `PendingValidation`).
fn make_server_pair() -> (Reader, Pusher) {
    make_pair_with_type(endpoint::Type::Server)
}

fn make_pair_with_type(ep_type: endpoint::Type) -> (Reader, Pusher) {
    let stream_id = VarInt::from_u8(1);
    let peer: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    let path_secret_entry = PathSecretEntry::fake_deterministic(peer, ep_type);

    let allocator = msg::queue::Allocator::new();
    let dispatcher = allocator.dispatcher();
    let handle = flow::Handle::client(stream_id, path_secret_entry.clone());
    let (_control, stream_rx) = dispatcher
        .alloc(handle, Some(VarInt::from_u8(2)))
        .expect("queue alloc should succeed");

    let queue_id = stream_rx.queue_id();
    let request = flow::Request {
        credential_id: *path_secret_entry.id(),
        stream_id,
    };

    let (frame_tx, frame_rx) = frame::submission_channel(1);

    let reader = match ep_type {
        endpoint::Type::Client => {
            Reader::new_client(frame_tx, path_secret_entry, stream_id, stream_rx)
        }
        endpoint::Type::Server => {
            Reader::new_server_pending(frame_tx, path_secret_entry, stream_id, stream_rx)
        }
    };

    let pusher = Pusher {
        dispatcher,
        queue_id,
        request,
        frame_rx,
        frame_storage: PriorityStorage::default(),
    };

    (reader, pusher)
}

#[test]
fn peer_addr_returns_handshake_addr() {
    let (reader, _) = make_pair();
    let expected: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    assert_eq!(reader.peer_addr(), expected);
}

/// Mock endpoint side of a reader test.
///
/// `push_*` injects [`msg::Stream`] messages into the flow-queue dispatcher,
/// automatically waking any waiting Reader task.  `recv_frames` asynchronously
/// waits for [`Frame`]s that the Reader submitted (e.g. `MAX_DATA`,
/// `STOP_SENDING`).
struct Pusher {
    dispatcher: msg::queue::Dispatcher,
    queue_id: VarInt,
    request: flow::Request,
    /// Outbound frames submitted by the Reader (MAX_DATA, STOP_SENDING, …).
    frame_rx: SubmissionReceiver,
    /// Reusable priority-storage buffer; avoids re-allocating the fixed-size
    /// array on every `recv_frames` call.
    frame_storage: PriorityStorage,
}

impl Pusher {
    fn push(&mut self, message: msg::Stream) {
        // `send_stream` returns an `AutoWake` that fires the registered waker
        // on drop.  Binding to `_` drops it immediately, waking a waiting
        // Reader task right away.
        let _ = self
            .dispatcher
            .send_stream(
                self.queue_id,
                None,
                &self.request,
                intrusive_queue::Entry::new(message),
            )
            .unwrap_or_else(|_| panic!("send_stream should succeed in tests"));
    }

    fn push_data(&mut self, offset: u64, data: &[u8], fin: bool) {
        self.push(msg::Stream::Data {
            offset: VarInt::new(offset).unwrap(),
            payload: BytesMut::from(data),
            fin,
        });
    }

    fn push_reset(&mut self, error_code: VarInt) {
        self.push(msg::Stream::Reset { error_code });
    }

    fn push_flow_validated(&mut self) {
        self.push(msg::Stream::FlowValidated);
    }

    /// Asynchronously wait for frames submitted by the Reader.
    ///
    /// Suspends until at least one frame (or a channel-close) is received,
    /// then returns all frames collected in that batch as a flat intrusive
    /// queue.  The `PriorityStorage` allocation is reused across calls.
    ///
    /// # Iterating the result
    ///
    /// Use [`Queue::iter`][`intrusive_queue::Queue::iter`] to borrow frames
    /// without consuming them, or iterate by value to take ownership of each
    /// `Entry<Frame>`.  Entries deref to `Frame` so you can access fields
    /// (e.g. `entry.header`) without calling `into_inner`.
    async fn recv_frames(&mut self) -> intrusive_queue::Queue<Frame> {
        core::future::poll_fn(|cx| self.frame_rx.poll_swap(cx, &mut self.frame_storage)).await;
        let mut result = intrusive_queue::Queue::default();
        for (_priority, mut queue) in self.frame_storage.drain() {
            result.append(&mut queue);
        }
        result
    }

    /// Asynchronously waits for frames up to `duration`.
    ///
    /// Returns `Some(queue)` when at least one frame is received before timeout.
    /// Returns `None` on timeout or when only an empty wake/close is observed.
    async fn recv_frames_timeout(
        &mut self,
        duration: Duration,
    ) -> Option<intrusive_queue::Queue<Frame>> {
        let queue = crate::testing::timeout(duration, self.recv_frames())
            .await
            .ok()?;
        if queue.is_empty() {
            None
        } else {
            Some(queue)
        }
    }

    fn complete_with_status(
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

fn decode_max_data_from_flow_control(frame: &Frame) -> Option<VarInt> {
    if !matches!(frame.header, Header::FlowControl { .. }) {
        return None;
    }

    let mut payload = Vec::with_capacity(frame.payload.len());
    for chunk in frame.payload.chunks() {
        payload.extend_from_slice(chunk);
    }

    let mut frames = control::decoder::ControlFramesMut::new(payload.as_mut_slice());
    let frame = frames.next()?.ok()?;
    if frames.next().is_some() {
        return None;
    }

    match frame {
        FrameMut::MaxData(max_data) => Some(max_data.maximum_data),
        _ => None,
    }
}

// ─── write_data_reader unit tests (no I/O, no tasks) ──────────────────────────

#[test]
fn write_data_reader_bypasses_reassembler_for_in_order_data() {
    let mut reassembler = Reassembler::new();
    let mut reader = Data::new(8);
    let mut app_buf: Vec<u8> = Vec::new();

    write_data_reader(&mut reassembler, &mut reader, &mut app_buf, true).unwrap();

    assert_eq!(app_buf, Data::send_one_at(0, 8));
    assert_eq!(reassembler.consumed_len(), 8);
    assert_eq!(reassembler.final_size(), Some(8));
    assert!(reassembler.is_empty());
    assert!(reassembler.is_reading_complete());
}

#[test]
fn write_data_reader_keeps_out_of_order_data_in_reassembler() {
    let mut reassembler = Reassembler::new();
    let mut reader = Data::new(8);
    let mut app_buf: Vec<u8> = Vec::new();

    reader.seek_forward(4);
    write_data_reader(&mut reassembler, &mut reader, &mut app_buf, true).unwrap();

    // Nothing was delivered to the application yet — the tail (offset 4-7) is
    // buffered in the reassembler, but there is a gap at 0-3.  `is_empty()` and
    // `total_received_len()` both report zero because they only count bytes
    // contiguous from the current read position (offset 0).  `final_size()` is
    // set, confirming the tail and FIN were recorded internally.
    assert!(app_buf.is_empty());
    assert_eq!(reassembler.consumed_len(), 0);
    assert_eq!(reassembler.total_received_len(), 0);
    assert!(reassembler.is_empty());
    assert!(!reassembler.is_reading_complete());
    assert_eq!(
        reassembler.final_size(),
        Some(8),
        "FIN should be recorded even though the head is missing"
    );

    // Once the missing head is written, all 8 bytes become available.
    reassembler
        .write_at(0u32.into(), &Data::send_one_at(0, 4))
        .unwrap();
    assert_eq!(reassembler.len(), 8);
}

#[test]
fn write_data_reader_does_not_interpose_when_reassembler_has_head_data() {
    let mut reassembler = Reassembler::new();
    let mut reader = Data::new(8);
    let mut app_buf: Vec<u8> = Vec::new();

    reassembler
        .write_at(0u32.into(), &Data::send_one_at(0, 4))
        .unwrap();
    reader.seek_forward(4);

    write_data_reader(&mut reassembler, &mut reader, &mut app_buf, true).unwrap();

    // The interposer bypass is skipped because the reassembler already holds
    // data at the head (offset 0-3).  Both head and tail (reader, offset 4-7)
    // are stored in the reassembler; all 8 bytes are contiguous so they are
    // immediately accessible without a gap.
    assert!(app_buf.is_empty());
    assert_eq!(reassembler.len(), 8);
    assert_eq!(reassembler.total_received_len(), 8);
    assert!(!reassembler.is_empty());
}

// ─── Bach async tests ─────────────────────────────────────────────────────────
//
// Each test uses two *primary* tasks:
//   • endpoint task – owns Pusher; sends stream messages and asserts on frames.
//   • app task      – owns Reader; calls read_into and asserts on received data.
//
// Both tasks are marked `.primary()` so the Bach sim runs until *both* complete,
// providing backpressure-free cooperative scheduling between the two sides.

/// Basic in-order read: endpoint sends data + FIN, application reads until EOF.
#[test]
fn basic_read() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();

        // Endpoint task: push data + FIN then exit.
        async move {
            pusher.push_data(0, b"hello world", true);
        }
        .primary()
        .spawn();

        // App task: read until EOF.
        async move {
            let mut buf = BytesMut::with_capacity(32);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete);
            assert_eq!(&buf[..], b"hello world");
            assert!(reader.0.status.is_complete());
        }
        .primary()
        .spawn();
    });
}

/// In-order read counts bytes correctly and marks the stream complete.
///
/// Mirrors `poll_read_into_counts_direct_interposer_writes` but uses the
/// proper two-task async harness instead of a noop waker.
#[test]
fn in_order_read_reports_byte_count_and_completes() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();
        let expected = Data::send_one_at(0, 8);

        async move {
            pusher.push_data(0, &expected, true);
        }
        .primary()
        .spawn();

        async move {
            let mut out = Vec::new();
            let n = reader.read_into(&mut out).await.expect("read failed");
            assert_eq!(n, 8);
            assert_eq!(out, Data::send_one_at(0, 8));
            assert!(reader.0.status.is_complete());
        }
        .primary()
        .spawn();
    });
}

/// Out-of-order delivery: endpoint pushes tail then head; app reads complete
/// data after reassembly.  Both tasks are primaries so neither holds the other
/// open artificially.
#[test]
fn out_of_order_reassembly() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();

        // Endpoint task: push tail first so the app must wait for the head.
        async move {
            pusher.push_data(5, b"world", true); // tail: out of order
            bach::task::yield_now().await; // yield so app can process the tail
            pusher.push_data(0, b"hello", false); // head: fills the gap
        }
        .primary()
        .spawn();

        // App task: read until EOF.
        async move {
            let mut buf = BytesMut::with_capacity(32);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete);
            assert_eq!(&buf[..], b"helloworld");
        }
        .primary()
        .spawn();
    });
}

/// A reset terminates a read with `ConnectionReset`.
#[test]
fn reset_terminates_read() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_reset(VarInt::from_u8(42));
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(32);
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected reset error");
            assert_eq!(err.kind(), std::io::ErrorKind::ConnectionReset);
            assert!(reader.0.status.is_reset());
            // Reassembler should be cleared on reset to free memory.
            assert!(reader.0.reassembler.is_empty());
        }
        .primary()
        .spawn();
    });
}

/// Data arrives then a reset: the stream must eventually surface the reset.
#[test]
fn reset_after_partial_data() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_data(0, b"partial", false);
            pusher.push_reset(VarInt::from_u8(1));
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(64);
            // Read until we hit the reset error.
            loop {
                match reader.read_into(&mut buf).await {
                    Ok(0) => panic!("unexpected clean EOF, expected reset"),
                    Ok(_) => {}
                    Err(e) => {
                        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset);
                        break;
                    }
                }
            }
            // The "partial" data was delivered by the interposer before the
            // Reset message was processed in the same queue batch.  TCP has
            // the same semantics: data already in the receive buffer when a
            // RST arrives may have been copied to user-space.
            assert_eq!(&buf[..], b"partial");
            assert!(reader.0.status.is_reset());
            assert!(reader.0.reassembler.is_empty());
            // Subsequent reads after a reset must return ConnectionReset,
            // not BrokenPipe or some other error.
            let err2 = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected ConnectionReset on re-read");
            assert_eq!(err2.kind(), std::io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// Reset before data in the same queue batch: reset wins and late data is not
/// delivered to the application.
#[test]
fn reset_before_data_in_same_batch_discards_data() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();

        async move {
            // Push reset first, then data in the same queue batch.
            pusher.push_reset(VarInt::from_u8(7));
            pusher.push_data(0, b"late", true);
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(16);
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected ConnectionReset");
            assert_eq!(err.kind(), std::io::ErrorKind::ConnectionReset);
            assert!(buf.is_empty(), "data after reset should not be delivered");
            assert!(reader.0.reassembler.is_empty());

            let err2 = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected sticky ConnectionReset");
            assert_eq!(err2.kind(), std::io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// If the application reads one byte at a time (yielding between reads), data
/// buffered before a reset is drained before surfacing ConnectionReset.
#[test]
fn reset_after_partial_data_byte_at_a_time_drains_before_error() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();
        let expected = b"partial";

        async move {
            pusher.push_data(0, expected, false);
            pusher.push_reset(VarInt::from_u8(9));
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(64);
            loop {
                // Model an app that reads in tiny chunks and yields.
                bach::task::yield_now().await;

                let result = {
                    let mut limited = buf.with_write_limit(1);
                    reader.read_into(&mut limited).await
                };

                match result {
                    Ok(0) => panic!("unexpected clean EOF, expected reset"),
                    Ok(n) => assert_eq!(n, 1, "expected one-byte reads"),
                    Err(e) => {
                        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset);
                        break;
                    }
                }
            }

            assert_eq!(&buf[..], expected);
            assert!(reader.0.reassembler.is_empty());

            let err2 = {
                let mut limited = buf.with_write_limit(1);
                reader
                    .read_into(&mut limited)
                    .await
                    .expect_err("expected sticky ConnectionReset")
            };
            assert_eq!(err2.kind(), std::io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// The Reader must emit a `MAX_DATA` (FlowControl) frame after the application
/// consumes enough bytes to cross the replenishment threshold (> window / 2).
///
/// The endpoint task waits for the MAX_DATA frame asynchronously — mirroring
/// how a real endpoint would receive and process such frames from the app side.
#[test]
fn max_data_sent_after_consuming() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();
        let window_size = reader.0.window_size;
        // Cross the > window/2 threshold in a single read without exceeding the
        // advertised receive window.
        let payload = vec![0xabu8; (window_size / 2 + 1) as usize];
        let payload_len = payload.len();
        let expected_max_data = VarInt::new(window_size + payload_len as u64).unwrap();

        // Endpoint task: push data, then wait for the MAX_DATA frame.
        async move {
            pusher.push_data(0, &payload, false);
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one outbound frame");
            assert_eq!(
                frames.front().and_then(decode_max_data_from_flow_control),
                Some(expected_max_data),
                "expected exactly one MAX_DATA frame with the computed limit"
            );
        }
        .primary()
        .spawn();

        // App task: read once.
        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);
            let read = reader.read_into(&mut buf).await.expect("read failed");
            assert_eq!(read, payload_len);
            assert_eq!(buf.len(), payload_len);
            // Keep the task alive long enough for the endpoint-side assertion to
            // consume this batch before Reader is dropped at task completion.
            crate::testing::sleep(Duration::from_secs(1)).await;
        }
        .primary()
        .spawn();
    });
}

#[test]
fn max_data_transmission_failure_surfaces_error() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();
        let window_size = reader.0.window_size;
        let payload = vec![0u8; (window_size / 2 + 1) as usize];
        let payload_len = payload.len();

        async move {
            pusher.push_data(0, &payload, false);

            let frames = pusher.recv_frames().await;
            pusher.complete_with_status(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);

            let read = reader.read_into(&mut buf).await.expect("first read should succeed");
            assert_eq!(read, payload_len);

            bach::task::yield_now().await;

            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected broken pipe from failed MAX_DATA transmission");
            assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}

/// If the peer sends beyond the client's advertised receive window, the Reader
/// errors and emits a FlowReset.
#[test]
fn flow_control_violation_errors_reader_and_sends_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();
        let payload = vec![0u8; reader.0.window_size as usize + 1];
        let payload_len = payload.len();

        async move {
            pusher.push_data(0, &payload, false);
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one outbound frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::FlowReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == reset_error::FLOW_CONTROL_ERROR
                ),
                "expected exactly one FlowReset(Both, FLOW_CONTROL_ERROR) frame"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected InvalidData on flow-control violation");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        }
        .primary()
        .spawn();
    });
}

/// Client-side FIN within the advertised window should not require sending
/// MAX_DATA after the final byte is consumed.
#[test]
fn client_fin_within_window_does_not_send_max_data() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();
        reader.0.window_size = 8;
        reader.0.remote_max_data = VarInt::from_u8(8);
        let payload = b"hello";

        async move {
            pusher.push_data(0, payload, true);
            let frames = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(
                frames.is_none(),
                "client-side FIN crossing the threshold should not emit outbound frames"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(32);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete);
            assert_eq!(&buf[..], payload);
        }
        .primary()
        .spawn();
    });
}

/// If FIN is observed on an out-of-order tail before the missing head arrives,
/// client readers still must not emit MAX_DATA after reassembly completes.
#[test]
fn client_fin_observed_before_gap_fill_does_not_send_max_data() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();
        reader.0.window_size = 8;
        reader.0.remote_max_data = VarInt::from_u8(8);

        async move {
            pusher.push_data(2, b"llo", true);
            // Sleep long enough to ensure the out-of-order FIN segment is
            // processed before the head segment is injected.
            crate::testing::sleep(Duration::from_secs(1)).await;
            pusher.push_data(0, b"he", false);
            let frames = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(
                frames.is_none(),
                "client should suppress all outbound frames once FIN has been observed"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(32);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete);
            assert_eq!(&buf[..], b"hello");
        }
        .primary()
        .spawn();
    });
}

/// A server-side stream starts in `PendingValidation`; calling `read_into`
/// before `validate` returns an `InvalidInput` error.
#[test]
fn server_read_before_validate_fails() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_server_pair();

        async move {
            pusher.push_data(0, b"hello", true);
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(16);
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected error before validation");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        }
        .primary()
        .spawn();
    });
}

/// A server-side stream becomes readable once `FlowValidated` is received.
#[test]
fn server_validates_then_reads() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_server_pair();
        let expected_max_data = VarInt::new(reader.0.window_size + 5).unwrap();

        async move {
            pusher.push_flow_validated();
            // Encourage task interleaving so validation/read processing can run
            // before we assert on emitted flow-control frames.
            bach::task::yield_now().await;
            pusher.push_data(0, b"hello", true);
            bach::task::yield_now().await;
            let frames = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected server flow update after validating/reading");
            assert_eq!(frames.len(), 1, "expected exactly one outbound frame");
            assert_eq!(
                frames.front().and_then(decode_max_data_from_flow_control),
                Some(expected_max_data),
                "expected exactly one MAX_DATA frame with the computed limit"
            );
        }
        .primary()
        .spawn();

        async move {
            reader.validate().await.expect("validate failed");
            let mut buf = BytesMut::with_capacity(16);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete);
            assert_eq!(&buf[..], b"hello");
        }
        .primary()
        .spawn();
    });
}

/// Dropping the Reader before a FIN is received must send a `STOP_SENDING`
/// (FlowReset) frame so the peer knows to stop.
///
/// The endpoint task waits for the frame asynchronously, mirroring how a
/// real endpoint would process control frames from the application side.
#[test]
fn drop_before_fin_sends_stop_sending() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();

        // Endpoint task: push some data (no FIN), then wait for STOP_SENDING.
        async move {
            pusher.push_data(0, b"some data", false);
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one outbound frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::FlowReset {
                        reset_target: ResetTarget::Stream,
                        error_code,
                        ..
                    } if error_code == reset_error::STOP_SENDING
                ),
                "expected exactly one FlowReset(Stream, STOP_SENDING) on drop"
            );
        }
        .primary()
        .spawn();

        // App task: do one read then drop the reader without a FIN.
        async move {
            let mut buf = BytesMut::with_capacity(64);
            let _ = reader.read_into(&mut buf).await;
            drop(reader); // no FIN received → Drop sends STOP_SENDING
        }
        .primary()
        .spawn();
    });
}

/// Dropping the Reader during panic sends ABNORMAL_TERMINATION to both sides.
#[test]
fn panic_drop_sends_abnormal_termination_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (reader, mut pusher) = make_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one outbound frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::FlowReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == reset_error::ABNORMAL_TERMINATION
                ),
                "expected exactly one FlowReset(Both, ABNORMAL_TERMINATION) on panic drop"
            );
        }
        .primary()
        .spawn();

        async move {
            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Move ownership into the panic scope so Drop runs while the
                // thread is panicking and emits ABNORMAL_TERMINATION.
                let moved_reader = reader;
                let _ = &moved_reader;
                panic!("intentional test panic while dropping reader");
            }));
            assert!(panic_result.is_err());
        }
        .primary()
        .spawn();
    });
}

/// After clean FIN completion, dropping Reader must not emit STOP_SENDING.
#[test]
fn drop_after_fin_completion_sends_no_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_data(0, b"ok", true);
            let frames = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(
                frames.is_none(),
                "no frame should be emitted after clean completion"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(16);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete);
            assert_eq!(&buf[..], b"ok");
            drop(reader);
        }
        .primary()
        .spawn();
    });
}

/// Flow-control violations should emit exactly one reset frame even if the app
/// performs additional reads after the initial error.
#[test]
fn flow_control_violation_emits_single_reset_frame() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();
        let payload = vec![0u8; reader.0.window_size as usize + 1];

        async move {
            pusher.push_data(0, &payload, false);
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one reset frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::FlowReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == reset_error::FLOW_CONTROL_ERROR
                ),
                "expected one FLOW_CONTROL_ERROR reset"
            );

            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(
                extra.is_none(),
                "reader should not emit additional frames on follow-up reads"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(32);
            let first = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected InvalidData on first violating read");
            assert_eq!(first.kind(), std::io::ErrorKind::InvalidData);

            let second = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected sticky reset on follow-up read");
            assert_eq!(second.kind(), std::io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// `read_to_end` should report `BufferFull` if the application-provided buffer
/// has no remaining capacity at call time.
#[test]
fn read_to_end_empty_buffer_returns_buffer_full() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_data(0, b"hello", true);
        }
        .primary()
        .spawn();

        async move {
            let mut backing = BytesMut::with_capacity(16);
            let mut limited = backing.with_write_limit(0);
            let outcome = reader
                .read_to_end(&mut limited)
                .await
                .expect("expected BufferFull for zero-capacity buffer");
            assert_eq!(outcome, ReadToEnd::BufferFull);
            assert!(backing.is_empty());
        }
        .primary()
        .spawn();
    });
}

/// `read_to_end` should return `BufferFull` once a fixed-size/non-growable
/// buffer is full, while preserving bytes that were already copied.
#[test]
fn read_to_end_full_buffer_returns_buffer_full() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_data(0, b"hello", true);
        }
        .primary()
        .spawn();

        async move {
            let mut backing = BytesMut::with_capacity(16);
            let outcome = {
                let mut limited = backing.with_write_limit(1);
                reader
                    .read_to_end(&mut limited)
                    .await
                    .expect("expected BufferFull once fixed-size buffer is full")
            };
            assert_eq!(outcome, ReadToEnd::BufferFull);
            assert_eq!(&backing[..], b"h");
        }
        .primary()
        .spawn();
    });
}

/// When the frame channel receiver is dropped (simulating a dead endpoint) the
/// Reader surfaces a `BrokenPipe` error when it tries to send flow-control
/// frames (e.g. `MAX_DATA`).  The Reader must not panic.
#[test]
fn broken_frame_channel_is_handled_gracefully() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut reader, pusher) = make_pair();
        let window_size = reader.0.window_size;

        // Destructure pusher to drop the original frame_rx (breaks reader's
        // frame_tx).  A fresh disconnected receiver takes its place so the
        // Pusher struct remains valid for pushing stream messages.
        let Pusher {
            dispatcher,
            queue_id,
            request,
            frame_rx: _closed,
            frame_storage,
        } = pusher;
        let mut pusher = Pusher {
            dispatcher,
            queue_id,
            request,
            // Dummy disconnected receiver — not used for assertions in this test.
            frame_rx: frame::submission_channel(1).1,
            frame_storage,
        };

        // Endpoint task: push enough data to trigger a MAX_DATA send without
        // exceeding the advertised receive window.
        let payload = vec![0u8; (window_size / 2 + 1) as usize];
        let payload_len = payload.len();
        async move {
            pusher.push_data(0, &payload, false);
        }
        .primary()
        .spawn();

        // App task: MAX_DATA cannot be sent (frame channel closed) → BrokenPipe.
        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected BrokenPipe when frame channel is closed");
            assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}
