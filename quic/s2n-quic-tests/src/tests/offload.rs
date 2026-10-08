use std::sync::atomic::{AtomicBool, Ordering};

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use super::*;
use s2n_quic::provider::tls::{
    default,
    offload::{Executor, ExporterHandler, OffloadBuilder},
};
struct BachExecutor;
impl Executor for BachExecutor {
    fn spawn(&self, task: impl core::future::Future<Output = ()> + Send + 'static) {
        bach::spawn(task);
    }
}

#[derive(Clone)]
struct Exporter;
impl ExporterHandler for Exporter {
    fn on_tls_exporter_ready(
        &self,
        _session: &impl s2n_quic_core::crypto::tls::TlsSession,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        None
    }

    fn on_client_application_params(
        &mut self,
        _client_params: s2n_quic_core::crypto::tls::ApplicationParameters,
        _server_params: &mut Vec<u8>,
    ) -> Option<std::result::Result<(), s2n_quic_core::transport::Error>> {
        None
    }
}

#[test]
fn tls() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let server_endpoint = default::Server::builder()
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
            .unwrap()
            .build()
            .unwrap();
        let client_endpoint = default::Client::builder()
            .with_certificate(certificates::CERT_PEM)
            .unwrap()
            .build()
            .unwrap();

        let server_endpoint = OffloadBuilder::new()
            .with_endpoint(server_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();
        let client_endpoint = OffloadBuilder::new()
            .with_endpoint(client_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event(tracing_events(false, model.clone()))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events(false, model.clone()))?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}

#[test]
fn failed_tls_handshake() {
    use s2n_quic::connection::Error;
    use s2n_quic_core::{crypto::tls::Error as TlsError, transport};
    let connection_closed_subscriber = recorder::ConnectionClosed::new();
    let connection_closed_event = connection_closed_subscriber.events();

    let model = Model::default();
    test(model.clone(), |handle| {
        let server_endpoint = default::Server::builder()
            .with_certificate(
                certificates::UNTRUSTED_CERT_PEM,
                certificates::UNTRUSTED_KEY_PEM,
            )
            .unwrap()
            .build()
            .unwrap();

        let client_endpoint = default::Client::builder()
            .with_certificate(certificates::CERT_PEM)
            .unwrap()
            .build()
            .unwrap();

        let server_endpoint = OffloadBuilder::new()
            .with_endpoint(server_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();
        let client_endpoint = OffloadBuilder::new()
            .with_endpoint(client_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event((
                tracing_events(false, model.clone()),
                connection_closed_subscriber,
            ))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events(false, model.clone()))?
            .start()?;
        let addr = start_server(server)?;
        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            client.connect(connect).await.unwrap_err();
        });

        Ok(addr)
    })
    .unwrap();

    let connection_closed_handle = connection_closed_event.lock().unwrap();
    let Error::Transport { code, .. } = connection_closed_handle[0] else {
        panic!("Unexpected error type")
    };
    let expected_error = TlsError::HANDSHAKE_FAILURE;
    assert_eq!(code, transport::Error::from(expected_error).code);
}

#[test]
#[cfg(s2n_tls_provider)]
fn mtls() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let server_endpoint = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
        let client_endpoint = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;

        let server_endpoint = OffloadBuilder::new()
            .with_endpoint(server_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();
        let client_endpoint = OffloadBuilder::new()
            .with_endpoint(client_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event(tracing_events(false, model.clone()))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events(false, model.clone()))?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}

#[test]
#[cfg(s2n_tls_provider)]
fn async_client_hello() {
    use futures::{ready, FutureExt};
    use s2n_quic::provider::tls::s2n_tls::{
        self, callbacks::ClientHelloCallback, connection::Connection, error::Error,
    };
    use std::task::Poll;

    let model = Model::default();

    struct MyCallbackHandler;
    struct MyConnectionFuture {
        output: Option<bach::task::JoinHandle<()>>,
    }

    impl ClientHelloCallback for MyCallbackHandler {
        fn on_client_hello(
            &self,
            _connection: &mut Connection,
        ) -> Result<Option<std::pin::Pin<Box<dyn s2n_tls::callbacks::ConnectionFuture>>>, Error>
        {
            let fut = MyConnectionFuture { output: None };
            Ok(Some(Box::pin(fut)))
        }
    }

    impl s2n_tls::callbacks::ConnectionFuture for MyConnectionFuture {
        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            _connection: &mut Connection,
            ctx: &mut core::task::Context,
        ) -> Poll<Result<(), Error>> {
            loop {
                if let Some(handle) = &mut self.output {
                    let _ = ready!(handle.poll_unpin(ctx));
                    return Poll::Ready(Ok(()));
                } else {
                    let future = async move {
                        let timer = bach::time::sleep(Duration::from_secs(3));
                        timer.await;
                    };
                    self.output = Some(bach::spawn(future));
                }
            }
        }
    }
    test(model.clone(), |handle| {
        let server_endpoint = default::Server::builder()
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
            .unwrap()
            .with_client_hello_handler(MyCallbackHandler)
            .unwrap()
            .build()
            .unwrap();
        let client_endpoint = default::Client::builder()
            .with_certificate(certificates::CERT_PEM)
            .unwrap()
            .build()
            .unwrap();

        let server_endpoint = OffloadBuilder::new()
            .with_endpoint(server_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();
        let client_endpoint = OffloadBuilder::new()
            .with_endpoint(client_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event(tracing_events(false, model.clone()))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events(false, model.clone()))?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}

#[derive(Clone, Default)]
struct TlsRecorder {
    tls_handshake_failed_seen: Arc<AtomicBool>,
    tls_exporter_event_seen: Arc<AtomicBool>,
}

impl s2n_quic::provider::event::Subscriber for TlsRecorder {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &s2n_quic::provider::event::ConnectionMeta,
        _info: &s2n_quic::provider::event::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_tls_handshake_failed(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &s2n_quic_core::event::api::ConnectionMeta,
        event: &s2n_quic_core::event::api::TlsHandshakeFailed,
    ) {
        self.tls_handshake_failed_seen
            .store(true, Ordering::Relaxed);
        // Assert s2n-tls error is retrievable
        let err = event
            .error
            .downcast_ref::<tls::s2n_tls::error::Error>()
            .unwrap();
        assert!(err.source() == tls::s2n_tls::error::ErrorSource::Library);
        assert_eq!(err.name(), "S2N_ERR_CERT_UNTRUSTED");
    }

    fn on_tls_exporter_ready(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &s2n_quic_core::event::api::ConnectionMeta,
        event: &s2n_quic_core::event::api::TlsExporterReady,
    ) {
        self.tls_exporter_event_seen.store(true, Ordering::Relaxed);
        let _ = event.session.peer_cert_chain_der().is_ok();
    }
}

#[test]
fn offload_connection_outputs_tls_events() {
    let recorder = TlsRecorder::default();

    let model = Model::default();
    test(model.clone(), |handle| {
        let server_endpoint = default::Server::builder()
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
            .unwrap()
            .build()
            .unwrap();

        let server_endpoint = OffloadBuilder::new()
            .with_endpoint(server_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event((tracing_events(false, model.clone()), recorder.clone()))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events(false, model.clone()))?
            .start()?;

        /* Successful handshake case */
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
    assert!(recorder.tls_exporter_event_seen.load(Ordering::Relaxed));
}

#[test]
fn offload_connection_outputs_tls_failure_events() {
    let recorder = TlsRecorder::default();

    let model = Model::default();
    test(model.clone(), |handle| {
        let server_endpoint = build_server_mtls_provider(certificates::UNTRUSTED_CERT_PEM)?;
        let client_endpoint = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
        let server_endpoint = OffloadBuilder::new()
            .with_endpoint(server_endpoint)
            .with_executor(BachExecutor)
            .with_exporter(Exporter)
            .build();

        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event((tracing_events(false, model.clone()), recorder.clone()))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events(false, model.clone()))?
            .start()?;

        /* Failed handshake case */
        let addr = server.local_addr().unwrap();
        spawn(async move {
            if server.accept().await.is_some() {
                panic!("connection should not be accepted on auth failure");
            }
        });
        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client
                .connect(connect)
                .await
                .expect("client should have succeeded");
            let stream_result = conn.accept_bidirectional_stream().await;
            assert!(stream_result.is_err(), "handshake should fail");
        });

        Ok(addr)
    })
    .unwrap();
    assert!(recorder.tls_handshake_failed_seen.load(Ordering::Relaxed));
}
