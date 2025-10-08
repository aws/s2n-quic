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
    fn on_tls_handshake_failed(
        &self,
        _session: &impl s2n_quic_core::crypto::tls::TlsSession,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        None
    }

    fn on_tls_exporter_ready(
        &self,
        _session: &impl s2n_quic_core::crypto::tls::TlsSession,
    ) -> Option<Box<dyn std::any::Any + Send>> {
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
            .with_event(tracing_events(true, model.max_udp_payload()))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events(true, model.max_udp_payload()))?
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
                tracing_events(true, model.max_udp_payload()),
                connection_closed_subscriber,
            ))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events(true, model.max_udp_payload()))?
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
#[cfg(unix)]
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
            .with_event(tracing_events(true, model.max_udp_payload()))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events(true, model.max_udp_payload()))?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}

#[test]
#[cfg(unix)]
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
            .with_event(tracing_events(true, model.max_udp_payload()))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events(true, model.max_udp_payload()))?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}
