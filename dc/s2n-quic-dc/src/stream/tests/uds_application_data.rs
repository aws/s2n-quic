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
//! and (in two of the three tests) serializes it via
//! `register_application_data_serializer`; the application-side server owns
//! the corresponding deserializer registered through
//! `with_application_data_deserializer`.
//!
//! Test matrix:
//!
//! | Test                              | Serializer | Deserializer  | Expected `path_application_data()` |
//! | --------------------------------- | ---------- | ------------- | ---------------------------------- |
//! | `forwards_across_uds_boundary`    | ok         | ok            | `Some(42u64)`                      |
//! | `deserializer_error_is_fail_open` | ok         | returns `Err` | `None` (stream still accepted)     |
//! | `no_serializer_is_v0_compatible`  | absent     | ok            | `None` (v0 packet on the wire)     |

use crate::{
    path::secret::{
        map::{ApplicationData, ApplicationDataError},
        stateless_reset::Signer,
        Map,
    },
    psk::{client::Provider as ClientProvider, server::Provider as ServerProvider},
    stream::{
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
        Arc,
    },
    time::Duration,
};

/// The concrete application-data type used across these tests. The choice is
/// arbitrary; what matters is that it survives the type-erased trip through
/// `Arc<dyn Any>` intact.
type TestValue = u64;
const TEST_VALUE: TestValue = 42;

/// Serializer used by the positive and negative tests. It downcasts the
/// type-erased `ApplicationData` to `u64` and writes it as big-endian bytes.
fn u64_serializer(
) -> Box<dyn Fn(&ApplicationData) -> Result<Option<Vec<u8>>, ApplicationDataError> + Send + Sync> {
    Box::new(|data| {
        let value = data
            .downcast_ref::<TestValue>()
            .copied()
            .expect("the test always registers a TestValue");
        Ok(Some(value.to_be_bytes().to_vec()))
    })
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

/// Constructs the application-side server. `deserializer` is optional: `None`
/// preserves pre-existing behavior (accepted streams carry no application
/// data).
fn create_application_server(
    unix_socket_path: &Path,
    deserializer: Option<ApplicationDataDeserializer>,
) -> application::Server<NoopSubscriber> {
    let mut builder = application::Server::<NoopSubscriber>::builder()
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_socket_path(unix_socket_path);
    if let Some(de) = deserializer {
        builder = builder.with_application_data_deserializer(de);
    }
    builder.build(NoopSubscriber {}).unwrap()
}

/// Positive path: the manager serializes the entry's `ApplicationData` into a
/// v1 UDS packet and the application server reconstructs it. Validates that a
/// `Stream::path_application_data::<T>()` accessed on the accepted UDS stream
/// yields the same value produced at handshake time.
#[tokio::test]
async fn forwards_across_uds_boundary() {
    init_tracing();

    let unix_socket_path = PathBuf::from("/tmp/uds_appdata_forward.sock");

    let stream_client = create_stream_client();
    let (handshake_server, server_map) = create_handshake_server().await;
    let handshake_addr = handshake_server.local_addr();

    server_map.register_make_application_data(Box::new(|_session| {
        let data: ApplicationData = Arc::new(TEST_VALUE);
        Ok(Some(data))
    }));
    server_map.register_application_data_serializer(u64_serializer());

    stream_client
        .handshake_with(handshake_addr, server_name())
        .await
        .unwrap();

    let app_server = create_application_server(&unix_socket_path, Some(u64_deserializer()));
    let manager_server = manager::Server::<ServerProvider, NoopSubscriber>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(&unix_socket_path)
        .build(handshake_server.clone(), NoopSubscriber {})
        .unwrap();

    let acceptor_addr = manager_server.acceptor_addr().unwrap();

    let (client_stream, server_result) = tokio::try_join!(
        stream_client.connect(handshake_addr, acceptor_addr, server_name()),
        async {
            tokio::time::timeout(Duration::from_secs(5), app_server.accept())
                .await
                .unwrap()
        }
    )
    .expect("stream should be accepted");
    let (server_stream, _addr) = server_result;

    // Client stream doesn't carry application data.
    assert!(client_stream.path_application_data().is_none());

    // Server (post-UDS) stream should carry the reconstructed value.
    let app_data = server_stream
        .path_application_data()
        .expect("application data should be attached");
    let value = app_data
        .downcast_ref::<TestValue>()
        .expect("application data should downcast to TestValue");
    assert_eq!(*value, TEST_VALUE);
}

/// Negative path: the deserializer returns `Err`. The stream must still be
/// accepted, and `path_application_data()` must be `None` (fail-open).
#[tokio::test]
async fn deserializer_error_is_fail_open() {
    init_tracing();

    let unix_socket_path = PathBuf::from("/tmp/uds_appdata_deserr.sock");

    let stream_client = create_stream_client();
    let (handshake_server, server_map) = create_handshake_server().await;
    let handshake_addr = handshake_server.local_addr();

    server_map.register_make_application_data(Box::new(|_session| {
        let data: ApplicationData = Arc::new(TEST_VALUE);
        Ok(Some(data))
    }));
    server_map.register_application_data_serializer(u64_serializer());

    stream_client
        .handshake_with(handshake_addr, server_name())
        .await
        .unwrap();

    // Deserializer counts invocations so we can confirm it actually ran (and
    // therefore that a blob was on the wire).
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_cb = calls.clone();
    let failing_deserializer: ApplicationDataDeserializer = Arc::new(move |_bytes: &[u8]| {
        calls_in_cb.fetch_add(1, Ordering::Relaxed);
        Err(ApplicationDataError {
            msg: "intentional test failure",
            inner: "boom".into(),
        })
    });

    let app_server = create_application_server(&unix_socket_path, Some(failing_deserializer));
    let manager_server = manager::Server::<ServerProvider, NoopSubscriber>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(&unix_socket_path)
        .build(handshake_server.clone(), NoopSubscriber {})
        .unwrap();

    let acceptor_addr = manager_server.acceptor_addr().unwrap();

    let (_client_stream, server_result) = tokio::try_join!(
        stream_client.connect(handshake_addr, acceptor_addr, server_name()),
        async {
            tokio::time::timeout(Duration::from_secs(5), app_server.accept())
                .await
                .unwrap()
        }
    )
    .expect("stream should still be accepted despite deserializer failure");
    let (server_stream, _addr) = server_result;

    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "deserializer should have been invoked exactly once (a blob was on the wire)"
    );
    assert!(
        server_stream.path_application_data().is_none(),
        "fail-open: application data must be None when the deserializer returns Err"
    );
}

/// Compatibility: no serializer is registered on the manager side, so the
/// forwarding worker emits a v0 packet (no blob). Even though the application
/// side has a deserializer registered, the accepted stream must have no
/// application data (there was nothing to reconstruct), and the deserializer
/// must not be invoked.
#[tokio::test]
async fn no_serializer_is_v0_compatible() {
    init_tracing();

    let unix_socket_path = PathBuf::from("/tmp/uds_appdata_v0compat.sock");

    let stream_client = create_stream_client();
    let (handshake_server, server_map) = create_handshake_server().await;
    let handshake_addr = handshake_server.local_addr();

    // Application data is produced on the entry, but no serializer is
    // registered, so it never leaves the manager.
    server_map.register_make_application_data(Box::new(|_session| {
        let data: ApplicationData = Arc::new(TEST_VALUE);
        Ok(Some(data))
    }));

    stream_client
        .handshake_with(handshake_addr, server_name())
        .await
        .unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_cb = calls.clone();
    let deserializer: ApplicationDataDeserializer = Arc::new(move |bytes: &[u8]| {
        calls_in_cb.fetch_add(1, Ordering::Relaxed);
        let array: [u8; 8] = bytes.try_into().map_err(|_| ApplicationDataError {
            msg: "expected 8 bytes",
            inner: "wrong length".into(),
        })?;
        let arc: ApplicationData = Arc::new(TestValue::from_be_bytes(array));
        Ok(Some(arc))
    });

    let app_server = create_application_server(&unix_socket_path, Some(deserializer));
    let manager_server = manager::Server::<ServerProvider, NoopSubscriber>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(&unix_socket_path)
        .build(handshake_server.clone(), NoopSubscriber {})
        .unwrap();

    let acceptor_addr = manager_server.acceptor_addr().unwrap();

    let (_client_stream, server_result) = tokio::try_join!(
        stream_client.connect(handshake_addr, acceptor_addr, server_name()),
        async {
            tokio::time::timeout(Duration::from_secs(5), app_server.accept())
                .await
                .unwrap()
        }
    )
    .expect("stream should be accepted with a v0 packet");
    let (server_stream, _addr) = server_result;

    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "deserializer must not run when no blob was forwarded (v0 packet on the wire)"
    );
    assert!(
        server_stream.path_application_data().is_none(),
        "no serializer registered: nothing to reconstruct on the application side"
    );
}
