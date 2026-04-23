// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Cross-version testing helpers using the previous release of s2n-quic.
//!
//! This module provides mirror versions of the test helpers in `lib.rs` that use
//! `s2n_quic_prev` types instead of the current version. This enables cross-version
//! compatibility testing where one side uses the current code and the other uses
//! the last published release.
//!
//! ## IO Compatibility
//!
//! Both versions share the same `bach` simulator crate, but have separate
//! `s2n-quic-platform` crates with distinct `Handle` types. We bridge this by
//! transmuting the current `Handle` into a prev `Handle` — they have identical
//! layout since both wrap `bach::executor::Handle` + `network::Buffers`.
//!
//! Compile-time assertions verify size and alignment match. If a future change
//! breaks layout compatibility, the build will fail immediately.

use s2n_quic::provider::io::testing::{spawn, Handle, Model};
use s2n_quic_prev::provider::io::testing as prev_io;
use std::net::SocketAddr;

use s2n_quic_core_prev::crypto::tls::testing::certificates as prev_certificates;

type Result<T = (), E = Box<dyn 'static + std::error::Error>> = core::result::Result<T, E>;

pub static PREV_SERVER_CERTS: (&str, &str) =
    (prev_certificates::CERT_PEM, prev_certificates::KEY_PEM);

// =============================================================================
// IO Handle conversion
// =============================================================================

/// Converts a current-version `Handle` into a prev-version `Handle`.
///
/// Both Handle types have identical memory layout:
///   - `executor: bach::executor::Handle` (same type in both versions)
///   - `buffers: network::Buffers` (structurally identical across versions)
///
/// The compile-time assertions below ensure this remains true. If the layout
/// ever diverges, the build will fail.
pub fn to_prev_handle(handle: &Handle) -> prev_io::Handle {
    const _: () = assert!(
        std::mem::size_of::<Handle>() == std::mem::size_of::<prev_io::Handle>(),
        "Handle size mismatch between current and prev versions"
    );
    const _: () = assert!(
        std::mem::align_of::<Handle>() == std::mem::align_of::<prev_io::Handle>(),
        "Handle alignment mismatch between current and prev versions"
    );
    // SAFETY: Both Handle types have identical layout verified by the assertions above.
    // We clone first to properly increment Arc reference counts.
    unsafe { std::mem::transmute(handle.clone()) }
}

// =============================================================================
// Previous version event subscriber
// =============================================================================

/// Mirror of `BlocklistSubscriber` implementing the prev version's `event::Subscriber` trait.
pub struct PrevBlocklistSubscriber {
    blocklist_enabled: bool,
    network_env: Model,
}

impl PrevBlocklistSubscriber {
    pub fn new(blocklist_enabled: bool, network_env: Model) -> Self {
        Self {
            blocklist_enabled,
            network_env,
        }
    }

    pub fn max_udp_payload(&self) -> u16 {
        self.network_env.max_udp_payload()
    }
}

use s2n_quic_prev::provider::event as prev_event;

impl prev_event::Subscriber for PrevBlocklistSubscriber {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &prev_event::events::ConnectionMeta,
        _info: &prev_event::events::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_datagram_dropped(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &prev_event::events::ConnectionMeta,
        event: &prev_event::events::DatagramDropped,
    ) {
        if self.blocklist_enabled {
            panic!(
                "Blacklisted datagram dropped event encountered: {:?}",
                event
            );
        }
    }

    fn on_packet_dropped(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &prev_event::events::ConnectionMeta,
        event: &prev_event::events::PacketDropped,
    ) {
        if matches!(
            event,
            prev_event::events::PacketDropped {
                reason: prev_event::events::PacketDropReason::DecryptionFailed { .. }
                    | prev_event::events::PacketDropReason::UnprotectFailed { .. }
                    | prev_event::events::PacketDropReason::VersionMismatch { .. }
                    | prev_event::events::PacketDropReason::UndersizedInitialPacket { .. }
                    | prev_event::events::PacketDropReason::InitialConnectionIdInvalidSpace { .. },
                ..
            }
        ) && self.blocklist_enabled
        {
            panic!("Blocklisted packet dropped event encountered: {:?}", event);
        }
    }

    fn on_packet_lost(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &prev_event::events::ConnectionMeta,
        event: &prev_event::events::PacketLost,
    ) {
        if event.bytes_lost < self.max_udp_payload() && self.blocklist_enabled {
            panic!(
                "Bytes lost is {} and max udp payload is {}\nBlocklisted packet lost event encountered: {:?}",
                event.bytes_lost,
                self.max_udp_payload(),
                event
            );
        }
    }

    fn on_platform_tx_error(
        &mut self,
        _meta: &prev_event::events::EndpointMeta,
        event: &prev_event::events::PlatformTxError,
    ) {
        if self.blocklist_enabled {
            panic!(
                "Blocklisted platform tx error event encountered: {:?}",
                event
            );
        }
    }

    fn on_platform_rx_error(
        &mut self,
        _meta: &prev_event::events::EndpointMeta,
        event: &prev_event::events::PlatformRxError,
    ) {
        if self.blocklist_enabled {
            panic!(
                "Blocklisted platform rx error event encountered: {:?}",
                event
            );
        }
    }

    fn on_endpoint_datagram_dropped(
        &mut self,
        _meta: &prev_event::events::EndpointMeta,
        event: &prev_event::events::EndpointDatagramDropped,
    ) {
        if self.blocklist_enabled {
            panic!(
                "Blocklisted endpoint datagram dropped event encountered: {:?}",
                event
            );
        }
    }
}

pub fn prev_tracing_events(
    with_blocklist: bool,
    network_env: Model,
) -> impl prev_event::Subscriber {
    (
        prev_event::tracing::Subscriber::default(),
        PrevBlocklistSubscriber::new(with_blocklist, network_env),
    )
}

// =============================================================================
// Previous version random provider
// =============================================================================

use rand::{
    rand_core::{Rng, SeedableRng, TryRng},
    rngs::ChaCha8Rng,
    RngExt,
};

/// Mirror of `Random` implementing the prev version's random traits.
pub struct PrevRandom {
    inner: ChaCha8Rng,
}

impl PrevRandom {
    pub fn with_seed(seed: u64) -> Self {
        Self {
            inner: ChaCha8Rng::seed_from_u64(seed),
        }
    }
}

impl s2n_quic_core_prev::havoc::Random for PrevRandom {
    fn fill(&mut self, bytes: &mut [u8]) {
        Rng::fill_bytes(&mut self.inner, bytes);
    }

    fn gen_range(&mut self, range: std::ops::Range<u64>) -> u64 {
        self.inner.random_range(range)
    }
}

impl TryRng for PrevRandom {
    type Error = core::convert::Infallible;

    fn try_next_u32(&mut self) -> core::result::Result<u32, Self::Error> {
        Ok(Rng::next_u32(&mut self.inner))
    }

    fn try_next_u64(&mut self) -> core::result::Result<u64, Self::Error> {
        Ok(Rng::next_u64(&mut self.inner))
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> core::result::Result<(), Self::Error> {
        Rng::fill_bytes(&mut self.inner, dest);
        Ok(())
    }
}

impl s2n_quic_prev::provider::random::Provider for PrevRandom {
    type Generator = Self;
    type Error = core::convert::Infallible;

    fn start(self) -> core::result::Result<Self::Generator, Self::Error> {
        Ok(self)
    }
}

impl s2n_quic_prev::provider::random::Generator for PrevRandom {
    fn public_random_fill(&mut self, dest: &mut [u8]) {
        Rng::fill_bytes(self, dest);
    }

    fn private_random_fill(&mut self, dest: &mut [u8]) {
        Rng::fill_bytes(self, dest);
    }
}

// =============================================================================
// Previous version server helpers
// =============================================================================

pub fn prev_build_server(handle: &Handle, network_env: Model) -> Result<s2n_quic_prev::Server> {
    let prev_handle = to_prev_handle(handle);
    Ok(s2n_quic_prev::Server::builder()
        .with_io(prev_handle.builder().build().unwrap())?
        .with_tls(PREV_SERVER_CERTS)?
        .with_event(prev_tracing_events(true, network_env))?
        .with_random(PrevRandom::with_seed(123))?
        .start()?)
}

pub fn prev_start_server(mut server: s2n_quic_prev::Server) -> Result<SocketAddr> {
    let server_addr = server.local_addr()?;

    spawn(async move {
        while let Some(mut connection) = server.accept().await {
            spawn(async move {
                while let Ok(Some(stream)) = connection.accept().await {
                    match stream {
                        s2n_quic_prev::stream::PeerStream::Receive(mut stream) => {
                            spawn(async move {
                                while let Ok(Some(_)) = stream.receive().await {
                                    // noop
                                }
                            });
                        }
                        s2n_quic_prev::stream::PeerStream::Bidirectional(mut stream) => {
                            spawn(async move {
                                while let Ok(Some(chunk)) = stream.receive().await {
                                    let _ = stream.send(chunk).await;
                                }
                            });
                        }
                    }
                }
            });
        }
    });

    Ok(server_addr)
}

pub fn prev_server(handle: &Handle, network_env: Model) -> Result<SocketAddr> {
    let server = prev_build_server(handle, network_env)?;
    prev_start_server(server)
}

// =============================================================================
// Previous version client helpers
// =============================================================================

pub fn prev_build_client(
    handle: &Handle,
    network_env: Model,
    with_blocklist: bool,
) -> Result<s2n_quic_prev::Client> {
    let prev_handle = to_prev_handle(handle);
    Ok(s2n_quic_prev::Client::builder()
        .with_io(prev_handle.builder().build().unwrap())?
        .with_tls(prev_certificates::CERT_PEM)?
        .with_event(prev_tracing_events(with_blocklist, network_env))?
        .with_random(PrevRandom::with_seed(123))?
        .start()?)
}

pub fn prev_start_client(
    client: s2n_quic_prev::Client,
    server_addr: SocketAddr,
    data: s2n_quic_core_prev::stream::testing::Data,
) -> Result {
    use s2n_quic::provider::io::testing::primary;

    primary::spawn(async move {
        let connect =
            s2n_quic_prev::client::Connect::new(server_addr).with_server_name("localhost");
        let mut connection = client.connect(connect).await.unwrap();

        let stream = connection.open_bidirectional_stream().await.unwrap();
        let (mut recv, mut send) = stream.split();

        let mut send_data = data;
        let mut recv_data = data;

        spawn(async move {
            while let Some(chunk) = recv.receive().await.unwrap() {
                recv_data.receive(&[chunk]);
            }
            assert!(recv_data.is_finished());
        });

        while let Some(chunk) = send_data.send_one(usize::MAX) {
            send.send(chunk).await.unwrap();
        }
    });

    Ok(())
}

pub fn prev_client(
    handle: &Handle,
    server_addr: SocketAddr,
    network_env: Model,
    with_blocklist: bool,
) -> Result {
    let client = prev_build_client(handle, network_env, with_blocklist)?;
    prev_start_client(
        client,
        server_addr,
        s2n_quic_core_prev::stream::testing::Data::new(10_000),
    )
}

// =============================================================================
// Handle identity function (for compat_test! macro)
// =============================================================================

/// Returns a prev Handle from a current Handle (for cross-version server/client construction).
pub fn as_prev_handle(
    handle: &s2n_quic::provider::io::testing::Handle,
) -> s2n_quic_prev::provider::io::testing::Handle {
    to_prev_handle(handle)
}

// =============================================================================
// Previous version mTLS helpers
// =============================================================================

#[cfg(not(target_os = "windows"))]
pub mod prev_mtls {
    use s2n_quic_core_prev::crypto::tls::testing::certificates as prev_certificates;
    use s2n_quic_prev::provider::tls;

    type Result<T = (), E = Box<dyn 'static + std::error::Error>> = core::result::Result<T, E>;

    pub fn prev_build_client_mtls_provider(ca_cert: &str) -> Result<tls::default::Client> {
        let tls = tls::default::Client::builder()
            .with_certificate(ca_cert)?
            .with_client_identity(
                prev_certificates::MTLS_CLIENT_CERT,
                prev_certificates::MTLS_CLIENT_KEY,
            )?
            .build()?;
        Ok(tls)
    }

    pub fn prev_build_server_mtls_provider(ca_cert: &str) -> Result<tls::default::Server> {
        let tls = tls::default::Server::builder()
            .with_certificate(
                prev_certificates::MTLS_SERVER_CERT,
                prev_certificates::MTLS_SERVER_KEY,
            )?
            .with_client_authentication()?
            .with_trusted_certificate(ca_cert)?
            .build()?;
        Ok(tls)
    }
}

#[cfg(not(target_os = "windows"))]
pub use prev_mtls::*;
