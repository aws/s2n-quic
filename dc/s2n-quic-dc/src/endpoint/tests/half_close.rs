// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Behavioral tests for stream half-close semantics.
//!
//! A "half close" is when one side of a bidirectional stream signals that it
//! is done *sending*, while still being willing to *receive*.  In s2n-quic-dc
//! this is represented by calling [`Writer::shutdown`] (or dropping the
//! `Writer`), which queues a FIN frame.  The peer's [`Reader`] then drains all
//! buffered data and eventually returns `Ok(0)` (EOF) from `read_into`.
//!
//! ## Protocol note
//!
//! The *client* is always the stream initiator: the first write from the
//! client writer sends a `FlowInit` packet that establishes the stream on the
//! server.  Until `FlowInit` arrives the server acceptor never sees the
//! stream.  Every test below therefore has the client write at least one byte
//! before relying on the server to accept the stream.
//!
//! ## Tests covered
//!
//! * **Client write half-close** – client sends data + FIN while the server
//!   continues to read/write.
//! * **Server write half-close** – server sends data + FIN immediately upon
//!   accepting (using the pre-established `remote_max_data` credit), while the
//!   client's request is delivered in parallel.
//! * **Both sides half-close** – client and server each close their write side;
//!   both readers reach EOF.
//! * **Writer drop sends FIN** – dropping a `Writer` without an explicit
//!   `shutdown()` call still delivers a graceful FIN to the peer.
//! * **Reader drop before EOF sends STOP_SENDING** – dropping a `Reader` that
//!   has not yet seen the peer's FIN causes a `STOP_SENDING` reset.
//! * **Server reader drop sends STOP_SENDING** – the server-side mirror of the
//!   above.
//! * **Shutdown idempotent** – multiple `shutdown()` calls are safe.
//! * **Reader drop after EOF is clean** – dropping a `Reader` that already
//!   reached EOF does NOT send `STOP_SENDING`.
//! * **Write after shutdown returns BrokenPipe** – writes after FIN fail fast.
//! * **Known bug (ignored)** – writer drop during `FlowInitSent` leaves server
//!   reader hanging.

use bytes::{Bytes, BytesMut};
use s2n_quic_core::varint::VarInt;

/// Acceptor ID used by every test in this module.
const ACCEPTOR_ID: VarInt = VarInt::from_u32(1);

// ── helpers ───────────────────────────────────────────────────────────────────

/// Read from `reader` until EOF, accumulating bytes into `buf`.
///
/// Panics if any read returns an error.
macro_rules! read_to_eof {
    ($reader:expr, $buf:expr) => {
        loop {
            let n = $reader.read_into(&mut $buf).await.expect("read_to_eof");
            if n == 0 {
                break;
            }
        }
    };
}

// ── client_write_half_close ──────────────────────────────────────────────────

/// The client sends data and then half-closes its write side (FIN).  The
/// server reads all bytes and observes EOF, then sends a response and closes
/// its own write side.  The client reads the server response to EOF.
///
/// This is the canonical half-close pattern: the initiator signals "I am done
/// sending" without tearing down the full connection; the responder can still
/// send data before closing.
#[test]
fn client_write_half_close() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (mut reader, mut writer) = stream.into_split();

                    // Drain the client's half-closed write side.
                    let mut buf = BytesMut::with_capacity(16);
                    read_to_eof!(reader, buf);
                    assert_eq!(&buf[..], b"hello");

                    // Server write half is still open: send a response + FIN.
                    let mut resp = Bytes::from_static(b"world");
                    writer
                        .write_all_from_fin(&mut resp)
                        .await
                        .expect("server write");
                    drop(writer);
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, mut writer) = stream.into_split();

            // Half-close the client write side.
            let mut data = Bytes::from_static(b"hello");
            writer
                .write_all_from_fin(&mut data)
                .await
                .expect("client write");
            drop(writer);

            // Client read half is still open: read the server's response.
            let mut buf = BytesMut::with_capacity(16);
            read_to_eof!(reader, buf);
            assert_eq!(&buf[..], b"world");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── server_write_half_close ──────────────────────────────────────────────────

/// The server half-closes its write side immediately upon accepting the stream
/// (before reading any client data).  The client reads the server's data to
/// EOF, then sends its own request + FIN.  The server reads the request.
///
/// The expected flow:
/// 1. Client sends a small initial request + FIN (establishes the stream via
///    FlowInit).
/// 2. Server accepts and immediately sends a greeting + FIN (server write
///    half-close), then reads client data to EOF.
/// 3. Client reads the greeting to EOF.
///
/// This verifies that the server can half-close its write side independently
/// of what the client is sending, and that the client sees EOF for the
/// server's direction while the client's own write has already been delivered.
///
/// The server writes first using the initial `remote_max_data` credit from the
/// path parameters (no MAX_DATA round-trip required), which is what enables
/// the immediate server write.
#[test]
fn server_write_half_close() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (mut reader, mut writer) = stream.into_split();

                    // Server immediately half-closes its write side.
                    let mut greeting = Bytes::from_static(b"greet");
                    writer
                        .write_all_from_fin(&mut greeting)
                        .await
                        .expect("server write");
                    drop(writer);

                    // Server read half is still open: drain the client request.
                    let mut buf = BytesMut::with_capacity(8);
                    read_to_eof!(reader, buf);
                    assert_eq!(&buf[..], b"req");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, mut writer) = stream.into_split();

            // Client sends its request + FIN (establishes the stream on the
            // server side via FlowInit, marks client write side as done).
            let mut req = Bytes::from_static(b"req");
            writer.write_all_from_fin(&mut req).await.expect("req");
            drop(writer);

            // Client read half is still open: read the server's greeting to EOF.
            let mut buf = BytesMut::with_capacity(16);
            read_to_eof!(reader, buf);
            assert_eq!(&buf[..], b"greet");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── both_sides_half_close ────────────────────────────────────────────────────

/// Both client and server close their write sides independently.  Each reader
/// should drain data from the opposite direction and reach EOF.
///
/// This verifies that two concurrent half-closes compose correctly: closing
/// one write direction must not abort the peer's write direction.
#[test]
fn both_sides_half_close() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (mut reader, mut writer) = stream.into_split();

                    // Server sends its data + FIN immediately.
                    let mut data = Bytes::from_static(b"from_server");
                    writer
                        .write_all_from_fin(&mut data)
                        .await
                        .expect("server write");
                    drop(writer);

                    // Server then reads the client's data to EOF.
                    let mut buf = BytesMut::with_capacity(32);
                    read_to_eof!(reader, buf);
                    assert_eq!(&buf[..], b"from_client");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, mut writer) = stream.into_split();

            // Client sends its data + FIN (establishes the stream on the
            // server side via FlowInit).
            let mut data = Bytes::from_static(b"from_client");
            writer
                .write_all_from_fin(&mut data)
                .await
                .expect("client write");
            drop(writer);

            // Client reads server data to EOF.
            let mut buf = BytesMut::with_capacity(32);
            read_to_eof!(reader, buf);
            assert_eq!(&buf[..], b"from_server");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── writer_drop_sends_fin ────────────────────────────────────────────────────

/// Dropping a [`Writer`] without calling `shutdown()` explicitly still causes
/// a graceful FIN to be delivered to the peer.
///
/// The `Writer::drop` implementation calls `shutdown()` when the thread is not
/// panicking.  This test confirms that the server reader reaches EOF cleanly
/// even when the client simply drops the writer handle without any explicit
/// call.
///
/// ## Payload size requirement
///
/// The FlowInit early-data capacity is limited to roughly the packet MTU minus
/// header overhead (~1363 bytes).  When the payload fits entirely in the
/// FlowInit, `write_all_from` returns while the writer is still in
/// `FlowInitSent` state (waiting for MAX_DATA).  `shutdown()` only handles
/// `Init` and `Open`; it is a no-op in `FlowInitSent` (see the `#[ignore]`
/// test `writer_drop_in_flow_init_sent_hangs_server_reader`).
///
/// By sending slightly more data than the FlowInit can carry, the second write
/// in `write_all_from` suspends until MAX_DATA arrives, which advances the
/// state to `Open`.  Once `write_all_from` returns the writer is therefore in
/// `Open` state and `drop(writer)` → `shutdown()` → `send_fin_packet()` works
/// correctly.
#[test]
fn writer_drop_sends_fin() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        // Payload slightly above the FlowInit early-data MTU so the second
        // write_from call suspends in FlowInitSent until MAX_DATA arrives,
        // advancing the writer to Open state before write_all_from returns.
        const PAYLOAD_LEN: usize = 1500;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (mut reader, mut writer) = stream.into_split();

                    // Server reads client data to EOF.
                    let mut buf = BytesMut::with_capacity(PAYLOAD_LEN + 8);
                    read_to_eof!(reader, buf);
                    assert_eq!(buf.len(), PAYLOAD_LEN);

                    // Confirm the stream completed cleanly with an echo.
                    let mut echo = Bytes::from_static(b"ok");
                    writer.write_all_from_fin(&mut echo).await.expect("echo");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, mut writer) = stream.into_split();

            // Send a payload that overflows the FlowInit early-data MTU.
            // The second write_from call in write_all_from will suspend in
            // FlowInitSent until MAX_DATA arrives, leaving the writer in Open
            // state when write_all_from returns.
            let data = vec![0xAAu8; PAYLOAD_LEN];
            let mut data = Bytes::from(data);
            writer
                .write_all_from(&mut data)
                .await
                .expect("write without fin");

            // Dropping the writer (in Open state) calls shutdown() →
            // send_fin_packet() → queues a FIN FlowData to the server.
            drop(writer);

            // Read server echo to confirm the stream completed cleanly.
            let mut buf = BytesMut::with_capacity(8);
            read_to_eof!(reader, buf);
            assert_eq!(&buf[..], b"ok");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── reader_drop_before_eof_sends_stop_sending ────────────────────────────────

/// Dropping the client [`Reader`] before the server has sent all its data
/// causes the reader's `Drop` to send a `STOP_SENDING` reset to the server.
/// The server's write loop then fails with `ConnectionReset`.
///
/// The expected sequence:
/// 1. Client establishes the stream (sends a request + FIN).
/// 2. Server reads the request to EOF.
/// 3. Server sends data in a loop (no FIN) until ConnectionReset.
/// 4. After the first write yields due to flow control, the client reads
///    a chunk and drops its reader, causing STOP_SENDING.
/// 5. Server sees `ConnectionReset` once STOP_SENDING is delivered.
///
/// ## Why write more than 1 MB?
///
/// In the test sim both endpoints use `remote_max_data = local_recv_max_data =
/// 1 MiB`.  The server writer can enqueue up to 1 MiB before it hits flow
/// control and suspends.  We need the server to actually suspend so that Bach
/// can schedule the client task (which drops the reader and queues
/// STOP_SENDING) before the server loop exhausts.  Using `CHUNK_SIZE × 3000 >
/// 1 MiB` guarantees the server blocks on flow control.
#[test]
fn reader_drop_before_eof_sends_stop_sending() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        // 1 KiB chunks; 3000 × 1024 = 3 MiB >> remote_max_data (1 MiB), so
        // the server MUST block on flow control before the loop ends.
        const CHUNK_SIZE: usize = 1024;
        const MAX_WRITES: usize = 3000;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (mut reader, mut writer) = stream.into_split();

                    // Read the client's request to EOF.
                    let mut req_buf = BytesMut::with_capacity(8);
                    read_to_eof!(reader, req_buf);
                    assert_eq!(&req_buf[..], b"req");

                    // Stream back a response without FIN until ConnectionReset.
                    let chunk = vec![0xABu8; CHUNK_SIZE];
                    let mut got_stop_sending = false;
                    for _ in 0..MAX_WRITES {
                        let mut data = Bytes::from(chunk.clone());
                        match writer.write_from(&mut data).await {
                            Ok(_) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                                got_stop_sending = true;
                                break;
                            }
                            Err(e) => {
                                panic!("unexpected server write error: {e}");
                            }
                        }
                    }
                    assert!(
                        got_stop_sending,
                        "server writer should have received STOP_SENDING after client reader drop"
                    );
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, mut writer) = stream.into_split();

            // Send a small request + FIN to establish the stream.
            let mut req = Bytes::from_static(b"req");
            writer
                .write_all_from_fin(&mut req)
                .await
                .expect("req write");

            // Read one chunk from the server (establishes remote_queue_id on
            // the reader so that Drop can send STOP_SENDING), then drop.
            let mut buf = BytesMut::with_capacity(CHUNK_SIZE);
            let n = reader.read_into(&mut buf).await.expect("first read");
            assert!(n > 0, "expected at least one byte from server");

            // Dropping the reader while the server still has more to send
            // causes Reader::drop to emit a STOP_SENDING FlowReset.
            drop(reader);
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── server_reader_drop_sends_stop_sending ────────────────────────────────────

/// When the server drops its [`Reader`] before the client has finished sending,
/// the server reader's `Drop` emits a `STOP_SENDING` reset, and the client
/// writer eventually observes a `ConnectionReset`.
///
/// The expected sequence:
/// 1. Client starts sending chunks (no FIN) to establish the stream.
/// 2. Server accepts, validates, writes a one-shot response + FIN, then the
///    server block exits and `_reader` is dropped.
/// 3. The server reader's Drop sends `STOP_SENDING` to the client's writer.
/// 4. The client writer loop sees `ConnectionReset` after flow-control yields.
///
/// ## Why more than 1 MB of writes?
///
/// Same reasoning as `reader_drop_before_eof_sends_stop_sending`: with
/// `remote_max_data = 1 MiB`, 64 × 512-byte writes (32 KiB) never block, so
/// the client task never yields and STOP_SENDING is never processed.  Writing
/// `CHUNK_SIZE × MAX_WRITES > 1 MiB` forces a flow-control pause so that
/// Bach can deliver the STOP_SENDING before the loop ends.
#[test]
fn server_reader_drop_sends_stop_sending() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        const CHUNK_SIZE: usize = 1024;
        const MAX_WRITES: usize = 3000;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (_reader, mut writer) = stream.into_split();

                    // Immediately write a short response + FIN and let the
                    // block exit.  When the block exits `_reader` is dropped
                    // (client is still writing → STOP_SENDING is emitted).
                    let mut resp = Bytes::from_static(b"stop");
                    let _ = writer.write_all_from_fin(&mut resp).await;
                    // `_reader` drops here → STOP_SENDING → client's control_rx
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, mut writer) = stream.into_split();

            // Send chunks without FIN.  Once STOP_SENDING is delivered the
            // writer returns ConnectionReset.  We need enough writes to exceed
            // the 1 MiB remote_max_data window so the task yields (allowing
            // STOP_SENDING to be processed) before the loop ends.
            let chunk = vec![0xCDu8; CHUNK_SIZE];
            let mut got_stop_sending = false;
            for _ in 0..MAX_WRITES {
                let mut data = Bytes::from(chunk.clone());
                match writer.write_from(&mut data).await {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                        got_stop_sending = true;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("client write unexpected error: {e}");
                        break;
                    }
                }
            }

            // Drain server's response (may arrive before or after STOP_SENDING).
            let mut buf = BytesMut::with_capacity(16);
            loop {
                match reader.read_into(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }

            assert!(
                got_stop_sending,
                "client writer should have received STOP_SENDING after server reader drop"
            );
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── shutdown_is_idempotent ───────────────────────────────────────────────────

/// Calling [`Writer::shutdown`] multiple times must be safe: the first call
/// queues the FIN and subsequent calls are no-ops.  There must be no
/// double-FIN or any error.
#[test]
fn shutdown_is_idempotent() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (mut reader, mut writer) = stream.into_split();

                    let mut buf = BytesMut::with_capacity(16);
                    read_to_eof!(reader, buf);
                    assert_eq!(&buf[..], b"idem");

                    let mut resp = Bytes::from_static(b"ok");
                    writer.write_all_from_fin(&mut resp).await.expect("resp");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, mut writer) = stream.into_split();

            let mut data = Bytes::from_static(b"idem");
            writer.write_all_from_fin(&mut data).await.expect("write");

            // Multiple shutdown calls must all succeed without error.
            writer.shutdown().expect("shutdown 1");
            writer.shutdown().expect("shutdown 2");
            writer.shutdown().expect("shutdown 3");
            drop(writer);

            let mut buf = BytesMut::with_capacity(8);
            read_to_eof!(reader, buf);
            assert_eq!(&buf[..], b"ok");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── reader_drop_after_eof_does_not_send_stop_sending ─────────────────────────

/// Dropping a [`Reader`] that has already received and consumed the peer's FIN
/// (EOF) must NOT send `STOP_SENDING`.
///
/// The `Reader::drop` implementation guards against this: it only sends
/// `STOP_SENDING` when `is_writing_complete()` is false and the stream is not
/// already in a reset state.  After EOF, `is_writing_complete()` is true.
///
/// The test confirms the server writer completes without error even after the
/// client drops its reader post-EOF.
#[test]
fn reader_drop_after_eof_does_not_send_stop_sending() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (mut reader, mut writer) = stream.into_split();

                    // Read the client request to EOF.
                    let mut req_buf = BytesMut::with_capacity(8);
                    read_to_eof!(reader, req_buf);

                    // Send a full response + FIN.  If the client erroneously
                    // sends STOP_SENDING after reading to EOF, this write would
                    // fail with ConnectionReset.
                    let mut data = Bytes::from_static(b"full_msg");
                    writer
                        .write_all_from_fin(&mut data)
                        .await
                        .expect("server write must succeed without ConnectionReset");
                    drop(writer);
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, mut writer) = stream.into_split();

            // Establish the stream with a request + FIN.
            let mut req = Bytes::from_static(b"req");
            writer.write_all_from_fin(&mut req).await.expect("req");
            drop(writer);

            // Read the server's full response to EOF.
            let mut buf = BytesMut::with_capacity(16);
            read_to_eof!(reader, buf);
            assert_eq!(&buf[..], b"full_msg");

            // Dropping the reader after EOF should NOT send STOP_SENDING.
            drop(reader);
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── write_after_shutdown_returns_broken_pipe ─────────────────────────────────

/// After calling `shutdown()` (or `write_all_from_fin`), subsequent write
/// attempts must immediately return `BrokenPipe`.  The write side is logically
/// closed once the FIN is queued.
#[test]
fn write_after_shutdown_returns_broken_pipe() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (mut reader, mut writer) = stream.into_split();

                    let mut buf = BytesMut::with_capacity(16);
                    read_to_eof!(reader, buf);

                    let mut resp = Bytes::from_static(b"ok");
                    writer.write_all_from_fin(&mut resp).await.expect("resp");
                    drop(writer);
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, mut writer) = stream.into_split();

            // Send data + FIN.
            let mut data = Bytes::from_static(b"closed");
            writer
                .write_all_from_fin(&mut data)
                .await
                .expect("first write");

            // Any subsequent write must fail with BrokenPipe.
            let mut extra = Bytes::from_static(b"extra");
            let err = writer
                .write_from(&mut extra)
                .await
                .expect_err("write after FIN must fail");
            assert_eq!(
                err.kind(),
                std::io::ErrorKind::BrokenPipe,
                "expected BrokenPipe after shutdown, got: {err:?}"
            );
            drop(writer);

            let mut buf = BytesMut::with_capacity(8);
            read_to_eof!(reader, buf);
            assert_eq!(&buf[..], b"ok");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── writer_drop_in_flow_init_sent ──────────────────────────────────────────

/// **Known bug (ignored):** When the client writer is dropped while still in
/// `FlowInitSent` state (the `FlowInit` packet was sent but `MAX_DATA` has not
/// yet been received), `shutdown()` is called but `send_fin_packet()` is a
/// no-op for `FlowInitSent`.  No FIN or `FlowReset` reaches the server, so
/// the server reader hangs forever.
///
/// # Expected behaviour after the fix
///
/// * `send_fin_packet()` should handle `FlowInitSent` by re-sending the
///   `FlowInit` with `is_fin: true` (or sending a `FlowReset`), so that the
///   server reader is unblocked.
/// * Once fixed, change the assertion to `result.is_ok()` and remove the
///   `#[ignore]`.
///
/// See the TODO comment in `stream/writer.rs` for the full fix description.
#[test]
#[ignore = "known bug: writer drop in FlowInitSent does not notify server reader (stream/writer.rs TODO)"]
fn writer_drop_in_flow_init_sent_hangs_server_reader() {
    use std::time::Duration;

    crate::testing::sim(|| {
        use crate::stream::endpoint::testing::sim::SERVER_PORT;
        use crate::testing::ext::*;

        // Suppress the very first server→client packet (the `MAX_DATA` /
        // `FlowControl` response to `FlowInit`).  This keeps the client writer
        // in `FlowInitSent` state indefinitely.
        {
            let mut server_pkt_count = 0u32;
            bach::net::monitor::on_packet_sent(move |packet| {
                if packet.source().port() == SERVER_PORT {
                    server_pkt_count += 1;
                    if server_pkt_count == 1 {
                        return bach::net::monitor::Command::Drop;
                    }
                }
                bach::net::monitor::Command::Pass
            });
        }

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(pending) = acceptor.recv().await {
                async move {
                    let stream = pending.validate().await.expect("validate");
                    let (mut reader, _writer) = stream.into_split();

                    // Server reader should unblock (EOF or reset) after the
                    // client drops its writer.  Currently it hangs (known bug).
                    let result = bach::time::timeout(Duration::from_secs(5), async {
                        let mut buf = BytesMut::with_capacity(16);
                        read_to_eof!(reader, buf);
                    })
                    .await;

                    // TODO: flip to `result.is_ok()` once the bug is fixed.
                    assert!(
                        result.is_err(),
                        "server reader unexpectedly completed – was the FlowInitSent bug fixed?"
                    );
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (_reader, mut writer) = stream.into_split();

            // Write early data so the FlowInit is sent → writer enters
            // FlowInitSent.  MAX_DATA is suppressed, so it stays there.
            let mut data = Bytes::from_static(b"early");
            let _ = writer.write_from(&mut data).await;

            // Drop the writer while in FlowInitSent.  `shutdown()` is called
            // but `send_fin_packet()` is a no-op.  Server reader never sees FIN.
            drop(writer);

            // Give the server enough simulated time to observe (or miss) the FIN.
            bach::time::sleep(Duration::from_secs(6)).await;
        }
        .group("client")
        .primary()
        .spawn();
    });
}
