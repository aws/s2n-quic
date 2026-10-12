// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests for `ApplicationData` forwarding across the manager -> UDS
//! -> application-server boundary.
//!
//! Each test builds a real split-process pipeline: a `manager::Server` bound
//! to a Unix domain socket, an `application::Server` reading from that socket,
//! and a `stream_client` that speaks the ordinary handshake + TCP path against
//! the manager. The manager-side path-secret `Map` produces an
//! `ApplicationData` at handshake time via `register_make_application_data`
//! and, where the case says so, serializes it via
//! `register_application_data_serializer`; the application-side server owns
//! the corresponding deserializer registered through
//! `with_application_data_deserializer`.
//!
//! Every drop of application data is fail-open (the stream is still accepted)
//! and is published as an `AcceptorTcpApplicationDataDropped` event on the side
//! that dropped it. Each server gets its own [`DropRecorder`] subscriber so
//! the tests assert the exact reasons published per side. Log lines are not
//! asserted; the events are the contract.
//!
//! Test matrix:
//!
//! | Test                                | Serializer | Deserializer | `path_application_data()` | Manager events    | Application events  |
//! | ----------------------------------- | ---------- | ------------ | ------------------------- | ----------------- | ------------------- |
//! | `forwards_across_uds_boundary`      | ok         | ok           | `Some(42u64)`             | none              | none                |
//! | `no_serializer_is_v0_compatible`    | absent     | ok           | `None` (v0 on the wire)   | none              | none                |
//! | `deserializer_error_is_fail_open`   | ok         | returns Err  | `None`                    | none              | `DeserializeFailed` |
//! | `serializer_error_is_fail_open`     | returns Err| ok           | `None`                    | `SerializeFailed` | none                |
//! | `oversized_blob_is_fail_open`       | > u16::MAX | ok           | `None`                    | `PacketTooLarge`  | none                |
//! | `no_deserializer_is_fail_open`      | ok         | absent       | `None`                    | none              | `NoDeserializer`    |

use crate::{
    event::{self, api},
    path::secret::{
        map::{ApplicationData, ApplicationDataError},
        stateless_reset::Signer,
        Map,
    },
    psk::{client::Provider as ClientProvider, server::Provider as ServerProvider},
    stream::{
        application::Stream,
        client::tokio::Client as ClientTokio,
        server::{application, manager, tokio::uds::ApplicationDataDeserializer},
        Protocol,
    },
    testing::{init_tracing, server_name, NoopSubscriber, TestTlsProvider},
};
use s2n_quic_core::time::StdClock;
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

/// The concrete application-data type used across these tests. The choice is
/// arbitrary; what matters is that it survives the type-erased trip through
/// `Arc<dyn Any>` intact.
type TestValue = u64;
const TEST_VALUE: TestValue = 42;

/// Boxed serializer callback shape accepted by
/// `Map::register_application_data_serializer`.
type TestSerializer =
    Box<dyn Fn(&ApplicationData) -> Result<Option<Vec<u8>>, ApplicationDataError> + Send + Sync>;

/// Serializer used by the positive tests. It downcasts the type-erased
/// `ApplicationData` to `u64` and writes it as big-endian bytes.
fn u64_serializer() -> TestSerializer {
    Box::new(|data| {
        let value = data
            .downcast_ref::<TestValue>()
            .copied()
            .expect("the test always registers a TestValue");
        Ok(Some(value.to_be_bytes().to_vec()))
    })
}

/// Serializer that always fails. The worker must publish `SerializeFailed`
/// and forward the stream without a blob.
fn failing_serializer() -> TestSerializer {
    Box::new(|_data| {
        Err(ApplicationDataError {
            msg: "intentional serializer failure",
            inner: "boom".into(),
        })
    })
}

/// Serializer whose blob alone fills the Unix datagram limit, so the encoded
/// handoff packet (blob plus the rest of the header and payload) must exceed
/// `u16::MAX` bytes. The worker must publish `PacketTooLarge`, drop the blob,
/// and still forward the stream.
fn oversized_serializer() -> TestSerializer {
    Box::new(|_data| Ok(Some(vec![0u8; u16::MAX as usize])))
}

/// Deserializer used by the positive test. It parses 8 big-endian bytes back
/// into a `u64` and wraps the value in a fresh `Arc`.
fn u64_deserializer() -> ApplicationDataDeserializer {
    Arc::new(|bytes: &[u8]| {
        let array: [u8; 8] = bytes.try_into().map_err(|_| ApplicationDataError {
            msg: "expected 8 bytes",
            inner: "wrong length".into(),
        })?;
        let value = TestValue::from_be_bytes(array);
        let arc: ApplicationData = Arc::new(value);
        Ok(Some(arc))
    })
}

/// Deserializer that always fails. The receiver must publish
/// `DeserializeFailed` and accept the stream with no application data.
fn failing_deserializer() -> ApplicationDataDeserializer {
    Arc::new(|_bytes: &[u8]| {
        Err(ApplicationDataError {
            msg: "intentional deserializer failure",
            inner: "boom".into(),
        })
    })
}

/// Wraps a deserializer so the test can assert how many times it ran. A count
/// of zero proves no blob was on the wire; a count of one proves there was.
fn counting_deserializer(
    calls: &Arc<AtomicUsize>,
    inner: ApplicationDataDeserializer,
) -> ApplicationDataDeserializer {
    let calls = calls.clone();
    Arc::new(move |bytes: &[u8]| {
        calls.fetch_add(1, Ordering::Relaxed);
        inner(bytes)
    })
}

/// Test-local mirror of the generated drop reason. The generated `api` enum
/// carries `#[non_exhaustive]` struct-like variants and no `PartialEq`, so
/// recorded events are mapped into this comparable type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropReason {
    SerializeFailed,
    PacketTooLarge,
    DeserializeFailed,
    NoDeserializer,
}

impl From<&api::AcceptorTcpApplicationDataDropReason> for DropReason {
    fn from(reason: &api::AcceptorTcpApplicationDataDropReason) -> Self {
        use api::AcceptorTcpApplicationDataDropReason as Api;
        match reason {
            Api::SerializeFailed { .. } => Self::SerializeFailed,
            Api::PacketTooLarge { .. } => Self::PacketTooLarge,
            Api::DeserializeFailed { .. } => Self::DeserializeFailed,
            Api::NoDeserializer { .. } => Self::NoDeserializer,
        }
    }
}

/// Endpoint subscriber that records the reason of every
/// `AcceptorTcpApplicationDataDropped` event it sees. One instance is attached
/// to each side of the pipeline so assertions are per side.
#[derive(Default)]
struct DropRecorder {
    reasons: Mutex<Vec<DropReason>>,
}

impl DropRecorder {
    fn drops(&self) -> Vec<DropReason> {
        self.reasons.lock().unwrap().clone()
    }
}

impl event::Subscriber for DropRecorder {
    type ConnectionContext = ();

    fn create_connection_context(
        &self,
        _meta: &api::ConnectionMeta,
        _info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_acceptor_tcp_application_data_dropped(
        &self,
        _meta: &api::EndpointMeta,
        event: &api::AcceptorTcpApplicationDataDropped,
    ) {
        self.reasons.lock().unwrap().push((&event.reason).into());
    }
}

/// Constructs a stream client together with its client-side path-secret `Map`.
fn create_stream_client() -> ClientTokio<ClientProvider, NoopSubscriber> {
    let tls = TestTlsProvider {};
    let sub = NoopSubscriber {};
    let client_map = Map::new(
        Signer::new(b"default"),
        100,
        false,
        StdClock::default(),
        sub,
    );

    let handshake_client = ClientProvider::builder()
        .start(
            "127.0.0.1:0".parse().unwrap(),
            client_map,
            tls,
            NoopSubscriber {},
            server_name(),
        )
        .unwrap();

    ClientTokio::<ClientProvider, NoopSubscriber>::builder()
        .with_tcp(true)
        .with_default_protocol(Protocol::Tcp)
        .build(handshake_client, NoopSubscriber {})
        .unwrap()
}

/// Constructs the manager-side handshake server and returns both it and the
/// underlying path-secret `Map`. Callers register `ApplicationData` hooks on
/// the returned map before wiring up the manager acceptor.
async fn create_handshake_server() -> (ServerProvider, Map) {
    let tls = TestTlsProvider {};
    let sub = NoopSubscriber {};

    let server_map = Map::new(
        Signer::new(b"default"),
        100,
        false,
        StdClock::default(),
        sub,
    );

    let handshake_server = ServerProvider::builder()
        .start(
            "127.0.0.1:0".parse().unwrap(),
            tls,
            NoopSubscriber {},
            server_map.clone(),
        )
        .await
        .unwrap();

    (handshake_server, server_map)
}

/// Constructs the application-side server with the given event subscriber.
/// `deserializer` is optional: `None` preserves pre-existing behavior
/// (accepted streams carry no application data).
fn create_application_server<S: event::Subscriber + Clone>(
    unix_socket_path: &Path,
    deserializer: Option<ApplicationDataDeserializer>,
    subscriber: S,
) -> application::Server<S> {
    let mut builder = application::Server::<S>::builder()
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_socket_path(unix_socket_path);
    if let Some(de) = deserializer {
        builder = builder.with_application_data_deserializer(de);
    }
    builder.build(subscriber).unwrap()
}

/// Constructs the manager-side acceptor that forwards accepted TCP streams
/// over the Unix domain socket, with the given event subscriber.
fn create_manager_server<S: event::Subscriber + Clone>(
    unix_socket_path: &Path,
    handshake_server: ServerProvider,
    subscriber: S,
) -> manager::Server<ServerProvider, S> {
    manager::Server::<ServerProvider, S>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(unix_socket_path)
        .build(handshake_server, subscriber)
        .unwrap()
}

/// What one run of the pipeline produced, after exactly one stream has been
/// handed across the Unix domain socket and accepted.
struct Outcome {
    /// The client's end of the stream. Kept so the TCP connection stays open
    /// for the duration of the assertions.
    client_stream: Stream<NoopSubscriber>,
    /// The stream as accepted by the application server, post-UDS.
    server_stream: Stream<Arc<DropRecorder>>,
    /// Drop reasons published by the manager (forwarding) side.
    manager_drops: Vec<DropReason>,
    /// Drop reasons published by the application (receiving) side.
    application_drops: Vec<DropReason>,
}

/// Builds the full split-process pipeline, performs one handshake and one
/// stream connect, and returns the accepted stream together with the drop
/// events each side published. The manager-side map always produces
/// `TEST_VALUE` at handshake time; `serializer` and `deserializer` decide what
/// happens to it from there.
async fn run_pipeline(
    socket_name: &str,
    serializer: Option<TestSerializer>,
    deserializer: Option<ApplicationDataDeserializer>,
) -> Outcome {
    init_tracing();

    let unix_socket_path = PathBuf::from(format!("/tmp/{socket_name}.sock"));

    let stream_client = create_stream_client();
    let (handshake_server, server_map) = create_handshake_server().await;
    let handshake_addr = handshake_server.local_addr();

    server_map.register_make_application_data(Box::new(|_session| {
        let data: ApplicationData = Arc::new(TEST_VALUE);
        Ok(Some(data))
    }));
    if let Some(serializer) = serializer {
        server_map.register_application_data_serializer(serializer);
    }

    stream_client
        .handshake_with(handshake_addr, server_name())
        .await
        .unwrap();

    let application_recorder = Arc::new(DropRecorder::default());
    let app_server = create_application_server(
        &unix_socket_path,
        deserializer,
        application_recorder.clone(),
    );

    let manager_recorder = Arc::new(DropRecorder::default());
    let manager_server = create_manager_server(
        &unix_socket_path,
        handshake_server.clone(),
        manager_recorder.clone(),
    );
    let acceptor_addr = manager_server.acceptor_addr().unwrap();

    let (client_stream, (server_stream, _addr)) = tokio::try_join!(
        stream_client.connect(handshake_addr, acceptor_addr, server_name()),
        async {
            tokio::time::timeout(Duration::from_secs(5), app_server.accept())
                .await
                .unwrap()
        }
    )
    .expect("the stream must be accepted; application data is fail-open");

    Outcome {
        client_stream,
        server_stream,
        manager_drops: manager_recorder.drops(),
        application_drops: application_recorder.drops(),
    }
}

/// Positive path: the manager serializes the entry's `ApplicationData` into a
/// v1 UDS packet and the application server reconstructs it. Validates that
/// `Stream::path_application_data::<T>()` on the accepted UDS stream yields
/// the same value produced at handshake time, and that neither side published
/// a drop.
#[tokio::test]
async fn forwards_across_uds_boundary() {
    let outcome = run_pipeline(
        "uds_appdata_forward",
        Some(u64_serializer()),
        Some(u64_deserializer()),
    )
    .await;

    // Client stream doesn't carry application data.
    assert!(outcome.client_stream.path_application_data().is_none());

    // Server (post-UDS) stream should carry the reconstructed value.
    let app_data = outcome
        .server_stream
        .path_application_data()
        .expect("application data should be attached");
    let value = app_data
        .downcast_ref::<TestValue>()
        .expect("application data should downcast to TestValue");
    assert_eq!(*value, TEST_VALUE);

    assert!(
        outcome.manager_drops.is_empty(),
        "nothing was dropped on the manager side"
    );
    assert!(
        outcome.application_drops.is_empty(),
        "nothing was dropped on the application side"
    );
}

/// Compatibility: no serializer is registered on the manager side, so the
/// forwarding worker emits a v0 packet (no blob). Even though the application
/// side has a deserializer registered, the accepted stream must have no
/// application data (there was nothing to reconstruct), the deserializer must
/// not be invoked, and neither side publishes a drop: absence is not a drop.
#[tokio::test]
async fn no_serializer_is_v0_compatible() {
    let calls = Arc::new(AtomicUsize::new(0));
    let outcome = run_pipeline(
        "uds_appdata_v0compat",
        None,
        Some(counting_deserializer(&calls, u64_deserializer())),
    )
    .await;

    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "deserializer must not run when no blob was forwarded (v0 packet on the wire)"
    );
    assert!(
        outcome.server_stream.path_application_data().is_none(),
        "no serializer registered: nothing to reconstruct on the application side"
    );
    assert!(outcome.manager_drops.is_empty(), "absence is not a drop");
    assert!(
        outcome.application_drops.is_empty(),
        "absence is not a drop"
    );
}

/// One fail-open scenario: which hooks are installed, and what each side is
/// expected to publish. Every case shares the same pipeline and the same
/// invariants (stream accepted, no application data attached); only the hooks
/// and the expected events differ.
struct FailOpenCase {
    /// Unix socket name, unique per case so the cases can run in parallel.
    socket_name: &'static str,
    serializer: Option<TestSerializer>,
    /// The deserializer under test, or `None` to leave the receiver
    /// unregistered. It is wrapped in a call counter by the harness.
    deserializer: Option<ApplicationDataDeserializer>,
    /// How many times the deserializer must have run. Zero proves no blob
    /// reached the application side.
    expected_deserializer_calls: usize,
    expected_manager_drops: &'static [DropReason],
    expected_application_drops: &'static [DropReason],
}

/// Runs one [`FailOpenCase`] and asserts the shared fail-open invariants plus
/// the case's expected events per side.
async fn assert_fail_open(case: FailOpenCase) {
    let calls = Arc::new(AtomicUsize::new(0));
    let deserializer = case
        .deserializer
        .map(|inner| counting_deserializer(&calls, inner));

    let outcome = run_pipeline(case.socket_name, case.serializer, deserializer).await;

    assert!(
        outcome.server_stream.path_application_data().is_none(),
        "fail-open: the stream is accepted with no application data"
    );
    assert_eq!(
        calls.load(Ordering::Relaxed),
        case.expected_deserializer_calls,
        "unexpected number of deserializer invocations"
    );
    assert_eq!(
        outcome.manager_drops, case.expected_manager_drops,
        "manager-side drop events"
    );
    assert_eq!(
        outcome.application_drops, case.expected_application_drops,
        "application-side drop events"
    );
}

/// The deserializer returns `Err`. A blob was on the wire (the deserializer
/// ran once), the stream is still accepted, and the application side publishes
/// exactly one `DeserializeFailed`; the manager side publishes nothing.
#[tokio::test]
async fn deserializer_error_is_fail_open() {
    assert_fail_open(FailOpenCase {
        socket_name: "uds_appdata_deserr",
        serializer: Some(u64_serializer()),
        deserializer: Some(failing_deserializer()),
        expected_deserializer_calls: 1,
        expected_manager_drops: &[],
        expected_application_drops: &[DropReason::DeserializeFailed],
    })
    .await;
}

/// The serializer returns `Err`. The manager forwards a v0 packet (no blob),
/// so the deserializer never runs; the manager side publishes exactly one
/// `SerializeFailed` and the application side publishes nothing.
#[tokio::test]
async fn serializer_error_is_fail_open() {
    assert_fail_open(FailOpenCase {
        socket_name: "uds_appdata_sererr",
        serializer: Some(failing_serializer()),
        deserializer: Some(u64_deserializer()),
        expected_deserializer_calls: 0,
        expected_manager_drops: &[DropReason::SerializeFailed],
        expected_application_drops: &[],
    })
    .await;
}

/// The serializer returns a blob that pushes the handoff packet past the Unix
/// datagram limit. The manager drops the blob and forwards a v0 packet, so the
/// deserializer never runs; the manager side publishes exactly one
/// `PacketTooLarge` and the application side publishes nothing.
#[tokio::test]
async fn oversized_blob_is_fail_open() {
    assert_fail_open(FailOpenCase {
        socket_name: "uds_appdata_oversize",
        serializer: Some(oversized_serializer()),
        deserializer: Some(u64_deserializer()),
        expected_deserializer_calls: 0,
        expected_manager_drops: &[DropReason::PacketTooLarge],
        expected_application_drops: &[],
    })
    .await;
}

/// A blob arrives but the application server has no deserializer registered.
/// The stream is accepted with no application data, the application side
/// publishes exactly one `NoDeserializer`, and the manager side publishes
/// nothing.
#[tokio::test]
async fn no_deserializer_is_fail_open() {
    assert_fail_open(FailOpenCase {
        socket_name: "uds_appdata_nodeser",
        serializer: Some(u64_serializer()),
        deserializer: None,
        expected_deserializer_calls: 0,
        expected_manager_drops: &[],
        expected_application_drops: &[DropReason::NoDeserializer],
    })
    .await;
}
