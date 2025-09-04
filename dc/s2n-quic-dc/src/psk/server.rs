// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::io;
use crate::path::secret;
use s2n_quic::provider::{event::Subscriber as Sub, tls::Provider as Prov};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::oneshot;
use tokio_util::sync::DropGuard;

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
    ) -> (
        oneshot::Receiver<Result<SocketAddr, super::io::Error>>,
        DropGuard,
    ) {
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
        std::thread::Builder::new()
            .name(String::from("hs-server"))
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(async move {
                    tokio::select! {
                        _ = cancelled => {}
                        _ = server => {}
                    }
                });
            })
            .unwrap();
        (rx, token.drop_guard())
    }

    /// Returns a [`Builder`] which is able to configure the [`Provider`]
    pub fn builder() -> Builder<impl s2n_quic::provider::event::Subscriber> {
        Builder::default()
    }

    pub fn new(
        map: secret::Map,
        local_addr: SocketAddr,
        guard: tokio_util::sync::DropGuard,
    ) -> Self {
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
    _guard: tokio_util::sync::DropGuard,
}
