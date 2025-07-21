// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

use s2n_quic::provider::tls::{
    default,
    offload::{Executor, Offload},
};

#[test]
fn tls() {
    struct BachExecutor;
    impl Executor for BachExecutor {
        fn spawn(&self, task: impl core::future::Future<Output = ()> + Send + 'static) {
            bach::spawn(task);
        }
    }

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
        let server_endpoint = Offload(server_endpoint, BachExecutor);
        let client_endpoint = Offload(client_endpoint, BachExecutor);
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
fn mtls() {
    struct BachExecutor;
    impl Executor for BachExecutor {
        fn spawn(&self, task: impl core::future::Future<Output = ()> + Send + 'static) {
            bach::spawn(task);
        }
    }

    let model = Model::default();
    test(model, |handle| {
        let server_endpoint = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
        let client_endpoint = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;

        let server_endpoint = Offload(server_endpoint, BachExecutor);
        let client_endpoint = Offload(client_endpoint, BachExecutor);

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
    use std::{
        sync::atomic::{AtomicBool, AtomicU8, Ordering},
        task::Poll,
    };

    struct BachExecutor;
    impl Executor for BachExecutor {
        fn spawn(&self, task: impl core::future::Future<Output = ()> + Send + 'static) {
            bach::spawn(task);
        }
    }

    let model = Model::default();

    pub struct MyCallbackHandler {
        done: Arc<AtomicBool>,
        wait_counter: Arc<AtomicU8>,
    }
    struct MyConnectionFuture {
        done: Arc<AtomicBool>,
        wait_counter: Arc<AtomicU8>,
    }

    impl MyCallbackHandler {
        fn new(wait_counter: u8) -> Self {
            MyCallbackHandler {
                done: Arc::new(AtomicBool::new(false)),
                wait_counter: Arc::new(AtomicU8::new(wait_counter)),
            }
        }
    }

    impl ClientHelloCallback for MyCallbackHandler {
        fn on_client_hello(
            &self,
            _connection: &mut Connection,
        ) -> Result<Option<std::pin::Pin<Box<dyn s2n_tls::callbacks::ConnectionFuture>>>, Error>
        {
            let fut = MyConnectionFuture {
                done: self.done.clone(),
                wait_counter: self.wait_counter.clone(),
            };
            Ok(Some(Box::pin(fut)))
        }
    }

    impl s2n_tls::callbacks::ConnectionFuture for MyConnectionFuture {
        fn poll(
            self: std::pin::Pin<&mut Self>,
            _connection: &mut Connection,
            _ctx: &mut core::task::Context,
        ) -> Poll<Result<(), Error>> {
            if self.wait_counter.fetch_sub(1, Ordering::SeqCst) == 0 {
                self.done.store(true, Ordering::SeqCst);
                return Poll::Ready(Ok(()));
            }

            Poll::Pending
        }
    }
    test(model, |handle| {
        let client_hello_handler = MyCallbackHandler::new(3);

        let server_endpoint = default::Server::builder()
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
            .unwrap()
            .with_client_hello_handler(client_hello_handler)
            .unwrap()
            .build()?;
        let client_endpoint = default::Client::builder()
            .with_certificate(certificates::CERT_PEM)
            .unwrap()
            .build()
            .unwrap();

        let server_endpoint = Offload(server_endpoint, BachExecutor);
        let client_endpoint = Offload(client_endpoint, BachExecutor);

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
