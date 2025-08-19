// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::io::{self, HandshakeFailed};
use crate::path::secret;
use s2n_quic::{
    provider::{event::Subscriber as Sub, tls::Provider as Prov},
    Connection,
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::runtime::Runtime;
use tokio_util::sync::DropGuard;

mod builder;

pub use crate::path::secret::HandshakeKind;
pub use builder::Builder;

#[derive(Clone)]
pub struct Provider {
    state: Arc<State>,
}

struct State {
    // This is always present in production, but for testing purposes we sometimes run within the
    // deterministic simulation framework. In that case there's no runtime for us to push work
    // into.
    runtime: Option<(Arc<Runtime>, DropGuard)>,
    map: secret::Map,
    client: io::Client,
    local_addr: SocketAddr,
}

fn make_runtime() -> (Arc<Runtime>, DropGuard) {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
    );

    let token = tokio_util::sync::CancellationToken::new();
    let cancelled = token.clone().cancelled_owned();
    let rt = runtime.clone();
    std::thread::Builder::new()
        .name(String::from("hs-client"))
        .spawn(move || {
            rt.block_on(cancelled);
        })
        .unwrap();

    (runtime, token.drop_guard())
}

impl State {
    fn new_runtime<
        Provider: Prov + Clone + Send + Sync + 'static,
        Subscriber: Sub + Send + Sync + 'static,
        Event: s2n_quic::provider::event::Subscriber,
    >(
        addr: SocketAddr,
        map: secret::Map,
        tls_materials_provider: Provider,
        subscriber: Subscriber,
        builder: Builder<Event>,
    ) -> io::Result<Self> {
        let (runtime, rt_guard) = make_runtime();
        let guard = runtime.enter();
        let client = io::Client::bind::<Provider, Subscriber, Event>(
            addr,
            map.clone(),
            tls_materials_provider,
            subscriber,
            builder,
        )?;
        drop(guard);

        Ok(Self {
            map,
            runtime: Some((runtime, rt_guard)),
            local_addr: client.local_addr()?,
            client,
        })
    }
}

impl Provider {
    /// Returns a [`Builder`] which is able to configure the [`Provider`]
    pub fn builder() -> Builder<impl s2n_quic::provider::event::Subscriber> {
        Builder::default()
    }

    pub fn new<
        Provider: Prov + Clone + Send + Sync + 'static,
        Subscriber: Sub + Send + Sync + 'static,
        Event: s2n_quic::provider::event::Subscriber,
    >(
        addr: SocketAddr,
        map: secret::Map,
        tls_materials_provider: Provider,
        subscriber: Subscriber,
        query_event_callback: fn(&mut Connection, Duration),
        builder: Builder<Event>,
        server_name: String,
    ) -> io::Result<Self> {
        let state = State::new_runtime(
            addr,
            map.clone(),
            tls_materials_provider,
            subscriber,
            builder,
        )?;
        let state = Arc::new(state);

        // Avoid holding onto the state unintentionally after it's no longer needed.
        let weak = Arc::downgrade(&state);
        map.register_request_handshake(Box::new(move |peer| {
            if let Some(state) = weak.upgrade() {
                let runtime = state.runtime.as_ref().map(|v| &v.0).unwrap();
                let client = state.client.clone();

                // Avoiding lifetime and move issues
                let server_name = server_name.clone();
                // Drop the JoinHandle -- we're not actually going to block on the join handle's
                // result. The future will keep running in the background.
                runtime.spawn(async move {
                    if let Err(HandshakeFailed { .. }) = client
                        .connect(peer, query_event_callback, server_name)
                        .await
                    {
                        // failure has already been logged, no further action required.
                    }
                });
            }
        }));

        Ok(Self { state })
    }

    /// Handshake asynchronously with a peer.
    ///
    /// This method can be called with any async runtime.
    #[inline]
    pub async fn handshake_with(
        &self,
        peer: SocketAddr,
        query_event_callback: fn(&mut Connection, Duration),
        server_name: String,
    ) -> std::io::Result<HandshakeKind> {
        // Avoiding lifetime and move issues
        let server_name = server_name.clone();
        let (_peer, kind) = self
            .handshake_with_entry(peer, query_event_callback, server_name)
            .await?;
        Ok(kind)
    }

    /// Handshake asynchronously with a peer, returning an entry for secret derivation
    ///
    /// This method can be called with any async runtime.
    #[inline]
    #[doc(hidden)]
    pub async fn handshake_with_entry(
        &self,
        peer: SocketAddr,
        query_event_callback: fn(&mut Connection, Duration),
        server_name: String,
    ) -> std::io::Result<(secret::map::Peer, HandshakeKind)> {
        // Avoiding lifetime and move issues
        let server_name_clone = server_name.clone();
        // Unconditionally request a background handshake. This schedules any re-handshaking
        // needed.
        if self.state.runtime.is_some() {
            let _ = self.background_handshake_with(peer, query_event_callback, server_name_clone);
        }

        if let Some(peer) = self.state.map.get_tracked(peer) {
            return Ok((peer, HandshakeKind::Cached));
        }

        let state = self.state.clone();
        if let Some((runtime, _)) = self.state.runtime.as_ref() {
            runtime
                .spawn(async move {
                    state
                        .client
                        .connect(peer, query_event_callback, server_name)
                        .await
                })
                .await??;
        } else {
            state
                .client
                .connect(peer, query_event_callback, server_name)
                .await?;
        }

        // already recorded a metric above in get_tracked.
        let peer = self.state.map.get_untracked(peer).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("handshake failed to exchange credentials for {peer}"),
            )
        })?;

        Ok((peer, HandshakeKind::Fresh))
    }

    /// Handshake with a peer in the background.∂
    #[inline]
    pub fn background_handshake_with(
        &self,
        peer: SocketAddr,
        query_event_callback: fn(&mut Connection, Duration),
        server_name: String,
    ) -> std::io::Result<HandshakeKind> {
        if self.state.map.contains(&peer) {
            return Ok(HandshakeKind::Cached);
        }

        // Avoiding lifetime and move issues
        let server_name = server_name.clone();
        let client = self.state.client.clone();
        if let Some((runtime, _)) = self.state.runtime.as_ref() {
            // Drop the JoinHandle -- we're not actually going to block on the join handle's
            // result. The future will keep running in the background.
            runtime.spawn(async move {
                if let Err(HandshakeFailed { .. }) = client
                    .connect(peer, query_event_callback, server_name)
                    .await
                {
                    // error already logged
                }
            });
        } else {
            panic!("background_handshake_with not supported with deterministic testing");
        }

        // Technically this might not be true (the handshake may get deduplicated), but it's close
        // enough to accurate that we're OK claiming it's true.
        Ok(HandshakeKind::Fresh)
    }

    /// Handshake synchronously with a peer.
    ///
    /// This method will block the calling thread and will panic if called from within a Tokio
    /// runtime.
    // We duplicate the implementation of this method with handshake_with so that we preserve the fast
    // path (not interacting with the runtime at all) for cached handshakes.
    #[inline]
    pub fn blocking_handshake_with(
        &self,
        peer: SocketAddr,
        query_event_callback: fn(&mut Connection, Duration),
        server_name: String,
    ) -> std::io::Result<HandshakeKind> {
        // Avoiding lifetime and move issues
        let server_name_clone = server_name.clone();
        // Unconditionally request a background handshake. This schedules any re-handshaking
        // needed.
        if self.state.runtime.is_some() {
            let _ = self.background_handshake_with(peer, query_event_callback, server_name_clone);
        }

        if self.state.map.contains(&peer) {
            return Ok(HandshakeKind::Cached);
        }

        let fut = self
            .state
            .client
            .connect(peer, query_event_callback, server_name);
        if let Some((runtime, _)) = self.state.runtime.as_ref() {
            runtime.block_on(fut)?
        } else {
            panic!("blocking_handshake_with not supported with deterministic testing");
        }

        debug_assert!(self.state.map.contains(&peer));

        Ok(HandshakeKind::Fresh)
    }

    /// This forces a handshake with the given peer, ignoring whether there's already an entry or
    /// not.
    #[inline]
    #[doc(hidden)]
    pub async fn unconditionally_handshake_with_entry(
        &self,
        peer: SocketAddr,
        query_event_callback: fn(&mut Connection, Duration),
        server_name: String,
    ) -> std::io::Result<secret::map::Peer> {
        // Avoiding lifetime and move issues
        let server_name = server_name.clone();
        let state = self.state.clone();
        if let Some((runtime, _)) = self.state.runtime.as_ref() {
            runtime
                .spawn(async move {
                    state
                        .client
                        .connect(peer, query_event_callback, server_name)
                        .await
                })
                .await??;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "missing runtime for handshake client",
            ));
        }

        // Don't bother recording metrics on access.
        let peer = self.state.map.get_untracked(peer).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("handshake failed to exchange credentials for {peer}"),
            )
        })?;

        Ok(peer)
    }

    // FIXME: Remove Result (breaking change)
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        Ok(self.state.local_addr)
    }

    pub fn map(&self) -> &secret::Map {
        &self.state.map
    }
}
