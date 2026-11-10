// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bach::time::timeout;
use bytes::Bytes;
use s2n_quic::{
    server::{self, ServerProviders},
    stream::{BidirectionalStream, SendStream},
};
use s2n_quic_core::transport::parameters::MAX_STREAMS_MAX_VALUE;
use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum concurrent bidirectional streams the server allows open at once.
const CONCURRENT_BIDI: u64 = 5;

/// Maximum concurrent unidirectional streams the server allows open at once.
const CONCURRENT_UNI: u64 = 4;

/// Maximum cumulative bidirectional streams the server will grant the client.
const TOTAL_BIDI: u64 = 12;

/// Maximum cumulative unidirectional streams the server will grant the client.
const TOTAL_UNI: u64 = 9;

/// Duration to wait for a stream open that should not complete.
/// Must be shorter than IDLE_TIMEOUT so the connection stays alive.
const BLOCKED_TIMEOUT: Duration = Duration::from_secs(100);

/// Idle timeout for both endpoints. Must exceed BLOCKED_TIMEOUT so the
/// connection does not close before we observe the blocked open.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Fires oneshots when the server receives `STREAMS_BLOCKED` frames from the
/// client — one for bidirectional and one for unidirectional, each carrying
/// the stream limit from the frame.
struct StreamsBlockedDetector {
    bidi_sender: Option<tokio::sync::oneshot::Sender<u64>>,
    uni_sender: Option<tokio::sync::oneshot::Sender<u64>>,
}

impl StreamsBlockedDetector {
    /// Returns `(detector, bidi_receiver, uni_receiver)`.
    fn new() -> (
        Self,
        tokio::sync::oneshot::Receiver<u64>,
        tokio::sync::oneshot::Receiver<u64>,
    ) {
        let (bidi_tx, bidi_rx) = tokio::sync::oneshot::channel();
        let (uni_tx, uni_rx) = tokio::sync::oneshot::channel();
        (
            Self {
                bidi_sender: Some(bidi_tx),
                uni_sender: Some(uni_tx),
            },
            bidi_rx,
            uni_rx,
        )
    }
}

impl events::Subscriber for StreamsBlockedDetector {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &events::ConnectionMeta,
        _info: &events::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_frame_received(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &events::ConnectionMeta,
        event: &events::FrameReceived,
    ) {
        match event.frame {
            events::Frame::StreamsBlocked {
                stream_type: events::StreamType::Bidirectional { .. },
                stream_limit,
                ..
            } => {
                if let Some(tx) = self.bidi_sender.take() {
                    let _ = tx.send(stream_limit);
                }
            }
            events::Frame::StreamsBlocked {
                stream_type: events::StreamType::Unidirectional { .. },
                stream_limit,
                ..
            } => {
                if let Some(tx) = self.uni_sender.take() {
                    let _ = tx.send(stream_limit);
                }
            }
            _ => {}
        }
    }
}

/// Counts every `MAX_STREAMS` frame the server sends, both directions. Clones
/// share the same counter, so the test keeps a clone to read after the
/// server consumes the original.
#[derive(Clone, Default)]
struct MaxStreamsSentCounter {
    sent: Arc<AtomicU64>,
}

impl events::Subscriber for MaxStreamsSentCounter {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &events::ConnectionMeta,
        _info: &events::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_frame_sent(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &events::ConnectionMeta,
        event: &events::FrameSent,
    ) {
        if matches!(event.frame, events::Frame::MaxStreams { .. }) {
            self.sent.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Builds a server with common defaults: IO, TLS, event subscriber, and RNG.
/// Callers chain `.with_limits(...)` to set stream caps before `.start()`.
fn base_server(
    handle: &Handle,
    model: Model,
    subscriber: impl events::Subscriber,
) -> Result<server::Builder<impl ServerProviders>> {
    Ok(Server::builder()
        .with_io(handle.builder().build()?)?
        .with_tls(SERVER_CERTS)?
        .with_event((subscriber, tracing_events(true, model)))?
        .with_random(Random::with_seed(456))?)
}

/// Builds a client with the shared IDLE_TIMEOUT. Mirrors `build_client` but
/// adds the idle timeout required by stream-limit tests.
fn test_client(handle: &Handle, model: Model) -> Result<Client> {
    Ok(Client::builder()
        .with_io(handle.builder().build()?)?
        .with_tls(certificates::CERT_PEM)?
        .with_event(tracing_events(true, model))?
        .with_random(Random::with_seed(123))?
        .with_limits(
            provider::limits::Limits::default()
                .with_max_idle_timeout(IDLE_TIMEOUT)
                .unwrap(),
        )?
        .start()?)
}

/// Opens `count` bidirectional streams, sends a payload on each, and verifies
/// the echoed data matches.
async fn open_bidi_streams(connection: &mut s2n_quic::Connection, count: u64) {
    for i in 0..count {
        let payload = Bytes::from(format!("stream {i}"));
        let result = connection.open_bidirectional_stream().await;
        assert!(
            result.is_ok(),
            "bidi stream {i} should open within cap of {count}"
        );
        let mut stream = result.unwrap();
        stream.send(payload.clone()).await.unwrap();
        stream.finish().unwrap();

        let mut received = Vec::new();
        while let Ok(Some(chunk)) = stream.receive().await {
            received.extend_from_slice(&chunk);
        }
        assert_eq!(
            received,
            payload.as_ref(),
            "echoed data mismatch on bidi stream {i}"
        );
        bach::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Opens `count` unidirectional (send-only) streams with a payload on each.
async fn open_uni_streams(connection: &mut s2n_quic::Connection, count: u64) {
    for i in 0..count {
        let result = connection.open_send_stream().await;
        assert!(
            result.is_ok(),
            "uni stream {i} should open within cap of {count}"
        );
        let mut stream = result.unwrap();
        stream
            .send(Bytes::from(format!("stream {i}")))
            .await
            .unwrap();
        stream.finish().unwrap();
        bach::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Opens `count` bidirectional streams without finishing or closing them.
/// Returns the held-open streams so callers can close them later.
async fn open_bidi_streams_held(
    connection: &mut s2n_quic::Connection,
    count: u64,
) -> Vec<BidirectionalStream> {
    let mut streams = Vec::with_capacity(count as usize);
    for i in 0..count {
        let result = connection.open_bidirectional_stream().await;
        assert!(
            result.is_ok(),
            "bidi stream {i} should open within concurrent limit of {count}"
        );
        streams.push(result.unwrap());
    }
    streams
}

/// Opens `count` unidirectional (send-only) streams without finishing them.
/// Returns the held-open streams so callers can close them later.
async fn open_uni_streams_held(
    connection: &mut s2n_quic::Connection,
    count: u64,
) -> Vec<SendStream> {
    let mut streams = Vec::with_capacity(count as usize);
    for i in 0..count {
        let result = connection.open_send_stream().await;
        assert!(
            result.is_ok(),
            "uni stream {i} should open within concurrent limit of {count}"
        );
        streams.push(result.unwrap());
    }
    streams
}

/// Asserts the next bidirectional stream open blocks (times out) and the server
/// received a `STREAMS_BLOCKED` frame at `expected_cap`.
async fn assert_stream_blocked_bidi(
    connection: &mut s2n_quic::Connection,
    blocked_rx: tokio::sync::oneshot::Receiver<u64>,
    expected_cap: u64,
) {
    match timeout(BLOCKED_TIMEOUT, connection.open_bidirectional_stream()).await {
        Err(_) => {} // timed out — stream correctly blocked
        Ok(Ok(_)) => panic!("bidi stream opened beyond cap of {expected_cap}"),
        Ok(Err(err)) => panic!("connection error before bidi block timeout: {err}"),
    }
    let limit = timeout(Duration::from_secs(5), blocked_rx)
        .await
        .expect("server should have received bidi STREAMS_BLOCKED (timed out)")
        .expect("bidi STREAMS_BLOCKED oneshot closed");
    assert_eq!(limit, expected_cap, "bidi STREAMS_BLOCKED limit");
}

/// Asserts the next unidirectional stream open blocks (times out) and the
/// server received a `STREAMS_BLOCKED` frame at `expected_cap`.
async fn assert_stream_blocked_uni(
    connection: &mut s2n_quic::Connection,
    blocked_rx: tokio::sync::oneshot::Receiver<u64>,
    expected_cap: u64,
) {
    match timeout(BLOCKED_TIMEOUT, connection.open_send_stream()).await {
        Err(_) => {} // timed out — stream correctly blocked
        Ok(Ok(_)) => panic!("uni stream opened beyond cap of {expected_cap}"),
        Ok(Err(err)) => panic!("connection error before uni block timeout: {err}"),
    }
    let limit = timeout(Duration::from_secs(5), blocked_rx)
        .await
        .expect("server should have received uni STREAMS_BLOCKED (timed out)")
        .expect("uni STREAMS_BLOCKED oneshot closed");
    assert_eq!(limit, expected_cap, "uni STREAMS_BLOCKED limit");
}

/// Proves the server stops granting MAX_STREAMS credit after the cumulative
/// cap, even when earlier streams are closed and the concurrent window has room.
#[test]
fn cumulative_stream_cap() {
    let model = Model::default();
    let (detector, bidi_blocked_rx, uni_blocked_rx) = StreamsBlockedDetector::new();

    test(model.clone(), move |handle| {
        // Concurrent limits are below total, so the cap is reached through
        // close-driven refill. The total cap is the only constraint under test.
        let server = base_server(handle, model.clone(), detector)?
            .with_limits(
                provider::limits::Limits::default()
                    .with_max_idle_timeout(IDLE_TIMEOUT)
                    .unwrap()
                    .with_max_open_remote_bidirectional_streams(CONCURRENT_BIDI)
                    .unwrap()
                    .with_max_total_remote_bidirectional_streams(TOTAL_BIDI)
                    .unwrap()
                    .with_max_open_remote_unidirectional_streams(CONCURRENT_UNI)
                    .unwrap()
                    .with_max_total_remote_unidirectional_streams(TOTAL_UNI)
                    .unwrap(),
            )?
            .start()?;
        let server_addr = start_server(server)?;

        let client = test_client(handle, model.clone())?;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await.unwrap();

            // The client sends STREAMS_BLOCKED only when other frames trigger a
            // transmission, so hold the last stream open with data sent on it.
            open_uni_streams(&mut connection, TOTAL_UNI - 1).await;
            let mut held_uni = connection.open_send_stream().await.unwrap();
            held_uni.send(Bytes::from("held uni stream")).await.unwrap();
            assert_stream_blocked_uni(&mut connection, uni_blocked_rx, TOTAL_UNI).await;
            held_uni.finish().unwrap();

            // Same pattern for bidi.
            open_bidi_streams(&mut connection, TOTAL_BIDI - 1).await;
            let mut held_bidi = connection.open_bidirectional_stream().await.unwrap();
            held_bidi
                .send(Bytes::from("held bidi stream"))
                .await
                .unwrap();
            assert_stream_blocked_bidi(&mut connection, bidi_blocked_rx, TOTAL_BIDI).await;
            held_bidi.finish().unwrap();
        });

        Ok(())
    })
    .unwrap();
}

/// Limits for one stream type across both `Limiter` stages.
struct StreamCapConfig {
    connection_open: u64,
    connection_total: u64,
    handshake_open: u64,
    handshake_total: u64,
}

impl Default for StreamCapConfig {
    fn default() -> Self {
        Self {
            connection_open: 100,
            connection_total: MAX_STREAMS_MAX_VALUE.as_u64(),
            handshake_open: 100,
            handshake_total: MAX_STREAMS_MAX_VALUE.as_u64(),
        }
    }
}

/// Sets initial stream caps in `on_connection`, raises them per-SNI in
/// `on_post_handshake`.
struct PostHandshakeLimiter {
    bidi: StreamCapConfig,
    uni: StreamCapConfig,
}

impl s2n_quic_core::connection::limits::Limiter for PostHandshakeLimiter {
    fn on_connection(
        &mut self,
        _info: &s2n_quic_core::connection::limits::ConnectionInfo,
    ) -> s2n_quic_core::connection::limits::Limits {
        s2n_quic_core::connection::limits::Limits::default()
            .with_max_idle_timeout(IDLE_TIMEOUT)
            .unwrap()
            .with_max_open_remote_bidirectional_streams(self.bidi.connection_open)
            .unwrap()
            .with_max_total_remote_bidirectional_streams(self.bidi.connection_total)
            .unwrap()
            .with_max_open_remote_unidirectional_streams(self.uni.connection_open)
            .unwrap()
            .with_max_total_remote_unidirectional_streams(self.uni.connection_total)
            .unwrap()
    }

    fn on_post_handshake(
        &mut self,
        info: &s2n_quic_core::connection::limits::HandshakeInfo,
        limits: &mut s2n_quic_core::connection::limits::UpdatableLimits,
    ) {
        if let Some(sni) = info.server_name {
            if &**sni == "localhost" {
                limits
                    .with_max_open_remote_bidirectional_streams(self.bidi.handshake_open)
                    .unwrap();
                limits
                    .with_max_total_remote_bidirectional_streams(self.bidi.handshake_total)
                    .unwrap();
                limits
                    .with_max_open_remote_unidirectional_streams(self.uni.handshake_open)
                    .unwrap();
                limits
                    .with_max_total_remote_unidirectional_streams(self.uni.handshake_total)
                    .unwrap();
            }
        }
    }
}

/// Proves total stream caps raised via `on_post_handshake` are honored: the
/// client can open more cumulative streams than `on_connection` allowed.
#[test]
fn post_handshake_raises_total_limit() {
    let connection_total_bidi = 3u64;
    let connection_total_uni = 2u64;
    let raised_total_bidi = TOTAL_BIDI;
    let raised_total_uni = TOTAL_UNI;

    let model = Model::default();
    let (detector, bidi_blocked_rx, uni_blocked_rx) = StreamsBlockedDetector::new();

    test(model.clone(), move |handle| {
        let server = base_server(handle, model.clone(), detector)?
            .with_limits(PostHandshakeLimiter {
                bidi: StreamCapConfig {
                    connection_total: connection_total_bidi,
                    handshake_total: raised_total_bidi,
                    ..StreamCapConfig::default()
                },
                uni: StreamCapConfig {
                    connection_total: connection_total_uni,
                    handshake_total: raised_total_uni,
                    ..StreamCapConfig::default()
                },
            })?
            .start()?;
        let server_addr = start_server(server)?;

        let client = test_client(handle, model.clone())?;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await.unwrap();

            // Open N-1 uni streams normally (send, finish, close), then hold
            // the Nth open so the transmit path runs and delivers
            // STREAMS_BLOCKED when the N+1th open blocks.
            open_uni_streams(&mut connection, raised_total_uni - 1).await;
            let mut held_uni = connection.open_send_stream().await.unwrap();
            held_uni.send(Bytes::from("held uni stream")).await.unwrap();
            assert_stream_blocked_uni(&mut connection, uni_blocked_rx, raised_total_uni).await;
            held_uni.finish().unwrap();

            // Same pattern for bidi.
            open_bidi_streams(&mut connection, raised_total_bidi - 1).await;
            let mut held_bidi = connection.open_bidirectional_stream().await.unwrap();
            held_bidi
                .send(Bytes::from("held bidi stream"))
                .await
                .unwrap();
            assert_stream_blocked_bidi(&mut connection, bidi_blocked_rx, raised_total_bidi).await;
            held_bidi.finish().unwrap();
        });

        Ok(())
    })
    .unwrap();
}

/// Raises only the larger of the open and total stream limits in
/// `on_post_handshake`, so the effective limit, `min(open, total)`, stays the
/// same.
///
/// Proves the server sends no `MAX_STREAMS` when a post-handshake raise leaves
/// the effective limit unchanged.
#[test]
fn post_handshake_effective_limit_unchanged() {
    let effective_limit = 5u64;
    let initial_larger_limit = 10u64;
    let raised_larger_limit = 20u64;

    for (connection_open, connection_total, handshake_open, handshake_total, label) in [
        (
            effective_limit,
            initial_larger_limit,
            effective_limit,
            raised_larger_limit,
            "total raised above the open limit",
        ),
        (
            initial_larger_limit,
            effective_limit,
            raised_larger_limit,
            effective_limit,
            "open raised above the total limit",
        ),
    ] {
        let caps = || StreamCapConfig {
            connection_open,
            connection_total,
            handshake_open,
            handshake_total,
        };
        let model = Model::default();
        let (detector, bidi_blocked_rx, uni_blocked_rx) = StreamsBlockedDetector::new();
        let counter = MaxStreamsSentCounter::default();

        test(model.clone(), move |handle| {
            let server = base_server(handle, model.clone(), (detector, counter.clone()))?
                .with_limits(PostHandshakeLimiter {
                    bidi: caps(),
                    uni: caps(),
                })?
                .start()?;
            let server_addr = start_server(server)?;

            let client = test_client(handle, model.clone())?;

            primary::spawn(async move {
                let connect = Connect::new(server_addr).with_server_name("localhost");
                let mut connection = client.connect(connect).await.unwrap();

                // The settle delay is far longer than the test network RTT, so
                // a `MAX_STREAMS` triggered by the raise has already been sent.
                bach::time::sleep(Duration::from_secs(1)).await;
                assert_eq!(
                    counter.sent.load(Ordering::Relaxed),
                    0,
                    "{label}: MAX_STREAMS sent after the post-handshake raise"
                );

                // Streams stay held: closing one would replenish credit and
                // send `MAX_STREAMS` legitimately. Sending on the last held
                // stream runs the transmit path, which delivers
                // `STREAMS_BLOCKED` when the next open blocks.
                let mut uni_streams = open_uni_streams_held(&mut connection, effective_limit).await;
                let held_uni = uni_streams
                    .last_mut()
                    .expect("effective_limit uni streams are held");
                held_uni.send(Bytes::from("held uni stream")).await.unwrap();
                assert_stream_blocked_uni(&mut connection, uni_blocked_rx, effective_limit).await;

                let mut bidi_streams =
                    open_bidi_streams_held(&mut connection, effective_limit).await;
                let held_bidi = bidi_streams
                    .last_mut()
                    .expect("effective_limit bidi streams are held");
                held_bidi
                    .send(Bytes::from("held bidi stream"))
                    .await
                    .unwrap();
                assert_stream_blocked_bidi(&mut connection, bidi_blocked_rx, effective_limit).await;
            });

            Ok(())
        })
        .unwrap_or_else(|err| panic!("{label}: {err}"));
    }
}

/// Proves a small post-handshake increase is advertised to the peer: the client
/// can open exactly one more stream of each type.
#[test]
fn post_handshake_small_raise_grants_one_stream() {
    let initial_open_limit = 20u64;
    let raised_open_limit = initial_open_limit + 1;

    let model = Model::default();
    let (detector, bidi_blocked_rx, uni_blocked_rx) = StreamsBlockedDetector::new();

    test(model.clone(), move |handle| {
        // The total limits stay at the default maximum, so the open limit is
        // the effective limit and the raise changes it.
        let caps = || StreamCapConfig {
            connection_open: initial_open_limit,
            handshake_open: raised_open_limit,
            ..StreamCapConfig::default()
        };
        let server = base_server(handle, model.clone(), detector)?
            .with_limits(PostHandshakeLimiter {
                bidi: caps(),
                uni: caps(),
            })?
            .start()?;
        let server_addr = start_server(server)?;

        let client = test_client(handle, model.clone())?;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await.unwrap();

            // Streams stay held: closing one would replenish credit and send
            // `MAX_STREAMS` legitimately. Sending on the last held stream runs
            // the transmit path, which delivers `STREAMS_BLOCKED` when the next
            // open blocks.
            let mut uni_streams = open_uni_streams_held(&mut connection, raised_open_limit).await;
            let held_uni = uni_streams
                .last_mut()
                .expect("raised_open_limit uni streams are held");
            held_uni.send(Bytes::from("held uni stream")).await.unwrap();
            assert_stream_blocked_uni(&mut connection, uni_blocked_rx, raised_open_limit).await;

            let mut bidi_streams = open_bidi_streams_held(&mut connection, raised_open_limit).await;
            let held_bidi = bidi_streams
                .last_mut()
                .expect("raised_open_limit bidi streams are held");
            held_bidi
                .send(Bytes::from("held bidi stream"))
                .await
                .unwrap();
            assert_stream_blocked_bidi(&mut connection, bidi_blocked_rx, raised_open_limit).await;
        });

        Ok(())
    })
    .unwrap();
}
