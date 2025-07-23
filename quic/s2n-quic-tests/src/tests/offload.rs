// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use super::*;
use s2n_quic::provider::tls::{
    default,
    offload::{Executor, OffloadBuilder},
};
struct BachExecutor;
impl Executor for BachExecutor {
    fn spawn(&self, task: impl core::future::Future<Output = ()> + Send + 'static) {
        bach::spawn(task);
    }
}

#[test]
fn tls() {
    let model = Model::default();
    test(model, |handle| {
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
            .build();
        let client_endpoint = OffloadBuilder::new()
            .with_endpoint(client_endpoint)
            .with_executor(BachExecutor)
            .build();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event(tracing_events())?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events())?
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
    test(model, |handle| {
        let server_endpoint = default::Server::builder()
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
            .unwrap()
            .build()
            .unwrap();
        // Client has no ability to trust server, which will lead to a cert untrusted error
        let client_endpoint = default::Client::builder().build().unwrap();

        let server_endpoint = OffloadBuilder::new()
            .with_endpoint(server_endpoint)
            .with_executor(BachExecutor)
            .build();
        let client_endpoint = OffloadBuilder::new()
            .with_endpoint(client_endpoint)
            .with_executor(BachExecutor)
            .build();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event((tracing_events(), connection_closed_subscriber))?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events())?
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
    test(model, |handle| {
        let server_endpoint = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
        let client_endpoint = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;

        let server_endpoint = OffloadBuilder::new()
            .with_endpoint(server_endpoint)
            .with_executor(BachExecutor)
            .build();
        let client_endpoint = OffloadBuilder::new()
            .with_endpoint(client_endpoint)
            .with_executor(BachExecutor)
            .build();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event(tracing_events())?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events())?
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
            _ctx: &mut core::task::Context,
        ) -> Poll<Result<(), Error>> {
            if let Some(handle) = &self.output {
                if handle.is_finished() {
                    return Poll::Ready(Ok(()));
                }
            } else {
                let future = async move {
                    let timer = bach::time::sleep(Duration::from_secs(3));
                    timer.await;
                };
                self.output = Some(bach::spawn(future));
            }

            Poll::Pending
        }
    }
    test(model, |handle| {
        let server_endpoint = default::Server::builder()
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
            .unwrap()
            .with_client_hello_handler(MyCallbackHandler)
            .unwrap()
            .build()?;
        let client_endpoint = default::Client::builder()
            .with_certificate(certificates::CERT_PEM)
            .unwrap()
            .build()
            .unwrap();

        let server_endpoint = OffloadBuilder::new()
            .with_endpoint(server_endpoint)
            .with_executor(BachExecutor)
            .build();
        let client_endpoint = OffloadBuilder::new()
            .with_endpoint(client_endpoint)
            .with_executor(BachExecutor)
            .build();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event(tracing_events())?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events())?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}
