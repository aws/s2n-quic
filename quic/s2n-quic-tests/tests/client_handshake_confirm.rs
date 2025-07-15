// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module shows an example of an event provider that blocks the client from opening any
//! streams until the handshake is completely confirmed by the server.
//!
//! When using mTLS, the client is technically allowed to start sending application data before the
//! server has actually authenticated the client. For clients that want to wait before getting a
//! confirmation of the server accepting the client's certificate, they can use the [`ClientConfirm`]
//! struct as an event provider on the [`Client`].
//!
//! Note that waiting for handshake confirmation will add another round trip to the handshake so
//! keep that in mind when deciding if this functionality is useful.
//!
//! This module includes 3 distinct tests to show the behavior:
//!
//! * The `mtls_happy_case` creates a successful connection and sends some data
//! * The `mtls_failure_no_wait` shows the case when the `wait_ready` functionality is not used.
//!   The client opens a connection and stream, sends some data, only to find that the server
//!   rejected the client's certificate when trying to receive the response.
//! * The `mtls_failure_with_wait` shows that when using the `wait_ready` functionality the client
//!   is unable to open a stream, as the task was blocked until the server either confirmed or
//!   rejected the connection attempt.

use core::task::{Context, Poll, Waker};
use s2n_quic::{
    client::Connect,
    provider::{
        event::events::{self, ConnectionInfo, ConnectionMeta, Subscriber},
        io::testing::{primary, test, Model},
    },
    Client, Server,
};
use s2n_quic_core::crypto::tls::testing::certificates;
use s2n_quic_tests::*;
use std::time::Duration;

struct ClientConfirm;

impl ClientConfirm {
    /// Blocks the task until the provided connection has either confirmed the handshake or closed
    /// with an error
    pub async fn wait_ready(conn: &mut s2n_quic::Connection) {
        futures::future::poll_fn(|cx| {
            conn.query_event_context_mut(|context: &mut ClientConfirmContext| {
                context.poll_ready(cx)
            })
            .unwrap_or(Poll::Ready(()))
        })
        .await;
    }
}

#[derive(Default)]
struct ClientConfirmContext {
    waker: Option<Waker>,
    state: State,
}

impl ClientConfirmContext {
    /// Updates the state on the context
    fn update(&mut self, state: State) {
        self.state = state;

        // notify the application that the state was updated
        self.wake();
    }

    /// Polls the context for handshake confirmation
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<()> {
        // if we're ready then let the application know
        if matches!(self.state, State::Ready) {
            return Poll::Ready(());
        }

        // store the waker so we can notify the application of state updates
        self.waker = Some(cx.waker().clone());

        Poll::Pending
    }

    /// notify the application of a state update
    fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Drop for ClientConfirmContext {
    // make sure the application is notified that we're closing the connection
    fn drop(&mut self) {
        self.wake();
    }
}

#[derive(Default)]
enum State {
    #[default]
    Waiting,
    Ready,
}

impl Subscriber for ClientConfirm {
    type ConnectionContext = ClientConfirmContext;

    #[inline]
    fn create_connection_context(
        &mut self,
        _: &ConnectionMeta,
        _info: &ConnectionInfo,
    ) -> Self::ConnectionContext {
        ClientConfirmContext::default()
    }

    #[inline]
    fn on_handshake_status_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        _: &ConnectionMeta,
        event: &events::HandshakeStatusUpdated,
    ) {
        if let events::HandshakeStatus::Confirmed { .. } = event.status {
            // notify the application that the handshake has been confirmed by the server
            context.update(State::Ready);
        }
    }

    #[inline]
    fn on_connection_closed(
        &mut self,
        context: &mut Self::ConnectionContext,
        _: &ConnectionMeta,
        _event: &events::ConnectionClosed,
    ) {
        // notify the application if we close the connection
        context.update(State::Ready);
    }
}

fn mtls_test<C>(server_cert: &str, f: fn(s2n_quic::Connection) -> C)
where
    C: 'static + core::future::Future<Output = ()> + Send,
{
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    test(model, |handle| {
        let server_tls = build_server_mtls_provider(server_cert)?;
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server_tls)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = start_server(server)?;

        let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(client_tls)?
            .with_event((ClientConfirm, tracing_events()))?
            .with_random(Random::with_seed(456))?
            .start()?;

        // show it working for several connections
        for _ in 0..10 {
            let client = client.clone();
            primary::spawn(async move {
                let connect = Connect::new(addr).with_server_name("localhost");
                let conn = client.connect(connect).await.unwrap();
                f(conn).await;
            });
        }

        Ok(addr)
    })
    .unwrap();
}

#[test]
fn mtls_happy_case() {
    const LEN: usize = 1000;

    mtls_test(certificates::MTLS_CA_CERT, |mut conn| {
        async move {
            // make sure we get confirmation of the handshake before opening a stream and sending
            // data
            ClientConfirm::wait_ready(&mut conn).await;

            let mut stream = conn.open_bidirectional_stream().await.unwrap();

            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.finish().unwrap();

            let mut recv_len = 0;
            while let Some(chunk) = stream.receive().await.unwrap() {
                recv_len += chunk.len();
            }
            assert_eq!(LEN, recv_len);
        }
    });
}

#[test]
fn mtls_failure_no_wait() {
    const LEN: usize = 1000;

    mtls_test(certificates::UNTRUSTED_CERT_PEM, |mut conn| {
        async move {
            // We don't use `wait_ready` here to show that we can open a stream that will fail
            // later

            let mut stream = conn.open_bidirectional_stream().await.unwrap();

            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.finish().unwrap();

            // we get an error only when receiving from the peer
            stream.receive().await.unwrap_err();
        }
    });
}

#[test]
fn mtls_failure_with_wait() {
    mtls_test(certificates::UNTRUSTED_CERT_PEM, |mut conn| {
        async move {
            // make sure we get confirmation of the handshake before opening a stream and sending
            // data
            ClientConfirm::wait_ready(&mut conn).await;

            // the open should fail since we waited for handshake confirmation
            conn.open_bidirectional_stream().await.unwrap_err();
        }
    });
}
