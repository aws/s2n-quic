// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::io;
use crate::path::secret;
use s2n_quic::provider::{event::Subscriber as Sub, tls::Provider as Prov};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::oneshot;

mod builder;

pub use builder::Builder;

#[derive(Clone)]
pub struct Provider {
    state: Arc<State>,
}

impl Provider {
    pub fn setup<
        Provider: Prov + Send + Sync + 'static,
        Subscriber: Sub + Send + Sync + 'static,
        Event: s2n_quic::provider::event::Subscriber,
    >(
        addr: SocketAddr,
        map: secret::Map,
        tls_materials_provider: Provider,
        subscriber: Subscriber,
        builder: super::server::Builder<Event>,
    ) -> std::io::Result<(
        oneshot::Receiver<Result<SocketAddr, super::io::Error>>,
        ShutdownGuard,
    )> {
        let (tx, rx) = oneshot::channel();
        let server = io::server(
            addr,
            map.clone(),
            builder,
            tls_materials_provider,
            subscriber,
            tx,
        );
        let token = tokio_util::sync::CancellationToken::new();
        let cancelled = token.clone().cancelled_owned();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let thread = std::thread::Builder::new()
            .name(String::from("hs-server"))
            .spawn(move || {
                rt.block_on(async move {
                    tokio::select! {
                        _ = cancelled => {}
                        _ = server => {}
                    }
                });
            })?;
        Ok((
            rx,
            ShutdownGuard {
                token,
                thread: Some(thread),
            },
        ))
    }

    /// Returns a [`Builder`] which is able to configure the [`Provider`]
    pub fn builder() -> Builder<impl s2n_quic::provider::event::Subscriber> {
        Builder::default()
    }

    pub fn new(map: secret::Map, local_addr: SocketAddr, guard: ShutdownGuard) -> Self {
        let state = State {
            map,
            local_addr,
            _guard: guard,
        };
        let state = Arc::new(state);
        Self { state }
    }

    #[inline]
    pub fn local_addr(&self) -> SocketAddr {
        self.state.local_addr
    }

    pub fn map(&self) -> &secret::Map {
        &self.state.map
    }
}

struct State {
    map: secret::Map,
    local_addr: SocketAddr,
    // This shuts down the backing runtime for the server on drop.
    _guard: ShutdownGuard,
}

/// Cancels the handshake server and waits for its runtime to release the socket.
///
/// Keep this guard alive for as long as the handshake listener is needed. Provider
/// clones share one guard, so only dropping the last clone stops the listener.
pub struct ShutdownGuard {
    token: tokio_util::sync::CancellationToken,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        self.token.cancel();
        if let Some(thread) = self.thread.take() {
            // A subscriber may drop the final owner from the handshake thread.
            // Joining ourselves would deadlock; cancellation will stop that loop.
            if thread.thread().id() != std::thread::current().id() {
                // Joining also waits for runtime destruction, including endpoint
                // tasks that retain the socket. Do not panic if the thread failed.
                let _ = thread.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        path::secret::stateless_reset::Signer,
        testing::{NoopSubscriber, TestTlsProvider},
    };
    use s2n_quic_core::time::StdClock;
    use std::net::UdpSocket;

    fn map() -> secret::Map {
        secret::Map::new(
            Signer::new(b"default"),
            50_000,
            false,
            StdClock::default(),
            NoopSubscriber,
        )
    }

    #[test]
    fn drop_releases_address() {
        // Keep the credential map alive across listener restarts.
        let map = map();
        let mut addr = "127.0.0.1:0".parse().unwrap();
        for _ in 0..20 {
            let server = Provider::builder()
                .start_blocking(
                    addr,
                    TestTlsProvider {}.start_server().unwrap(),
                    NoopSubscriber,
                    map.clone(),
                )
                .unwrap();
            addr = server.local_addr();
            let clone = server.clone();
            drop(server);
            assert_eq!(
                UdpSocket::bind(addr).unwrap_err().kind(),
                std::io::ErrorKind::AddrInUse,
            );
            drop(clone);
            // An exclusive bind ensures socket reuse cannot hide a leak.
            drop(UdpSocket::bind(addr).unwrap());
        }
    }

    // Dropping inside a Tokio runtime must wait for the handshake runtime's
    // socket to close, this test confirms that doesn't panic (as some
    // synchronous operations inside Tokio can).
    #[tokio::test]
    async fn runtime_drop_releases_address() {
        let server = Provider::builder()
            .start(
                "127.0.0.1:0".parse().unwrap(),
                TestTlsProvider {}.start_server().unwrap(),
                NoopSubscriber,
                map(),
            )
            .await
            .unwrap();
        let addr = server.local_addr();
        drop(server);
        drop(UdpSocket::bind(addr).unwrap());
    }

    #[test]
    fn setup_guard_drop_releases_address() {
        let (ready, guard) = Provider::setup(
            "127.0.0.1:0".parse().unwrap(),
            map(),
            TestTlsProvider {}.start_server().unwrap(),
            NoopSubscriber,
            Builder::default(),
        )
        .unwrap();
        let addr = ready.blocking_recv().unwrap().unwrap();
        drop(guard);
        drop(UdpSocket::bind(addr).unwrap());
    }
}
