// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::{self, Subscriber},
    path::secret::{stateless_reset::Signer, Map},
    psk::{client, server},
};
use s2n_quic::{provider::tls::Provider, server::Name, Connection};
use s2n_quic_core::{crypto::tls::testing::certificates, time::StdClock};
use std::{sync::OnceLock, time::Duration};

pub use bach::{ext, rand};

use s2n_quic::provider::tls::default as s2n_quic_tls_prov;

pub static SNI: OnceLock<Name> = OnceLock::new();

#[doc(hidden)]
pub fn server_name() -> Name {
    SNI.get_or_init(|| "localhost".into()).clone()
}

pub mod task {
    pub use bach::task::*;
    pub use tokio::task::yield_now;

    pub fn spawn<F>(f: F)
    where
        F: core::future::Future + Send + Sync + 'static,
        F::Output: Send + 'static,
    {
        if bach::is_active() {
            bach::spawn(f);
        } else {
            tokio::spawn(f);
        }
    }

    pub fn spawn_named<F, N: core::fmt::Display>(f: F, name: N)
    where
        F: core::future::Future + Send + Sync + 'static,
        F::Output: Send + 'static,
    {
        if bach::is_active() {
            bach::task::spawn_named(f, name);
        } else {
            tokio::spawn(f);
        }
    }
}

pub use task::spawn;

pub async fn sleep(duration: Duration) {
    if bach::is_active() {
        bach::time::sleep(duration).await;
    } else {
        tokio::time::sleep(duration).await;
    }
}

pub async fn timeout<F>(duration: Duration, f: F) -> Result<F::Output, bach::time::error::Elapsed>
where
    F: core::future::Future,
{
    if bach::is_active() {
        bach::time::timeout(duration, f).await
    } else {
        Ok(tokio::time::timeout(duration, f).await?)
    }
}

pub fn assert_debug<T: core::fmt::Debug>(_v: &T) {}
pub fn assert_send<T: Send>(_v: &T) {}
pub fn assert_sync<T: Sync>(_v: &T) {}
pub fn assert_static<T: 'static>(_v: &T) {}
pub fn assert_async_read<T: tokio::io::AsyncRead>(_v: &T) {}
pub fn assert_async_write<T: tokio::io::AsyncWrite>(_v: &T) {}

pub fn init_tracing() {
    if cfg!(any(miri, fuzzing)) {
        return;
    }

    use std::sync::Once;

    static TRACING: Once = Once::new();

    // make sure this only gets initialized once
    TRACING.call_once(|| {
        let format = tracing_subscriber::fmt::format()
            //.with_level(false) // don't include levels in formatted output
            //.with_ansi(false)
            .with_timer(Uptime::default())
            .compact(); // Use a less verbose output format.

        let default_level = if std::env::var("CI").is_ok() {
            // The CI runs out of memory if we log too much tracing data
            tracing::Level::INFO
        } else if cfg!(debug_assertions) {
            tracing::Level::DEBUG
        } else {
            tracing::Level::WARN
        };

        let env_filter = tracing_subscriber::EnvFilter::builder()
            .with_default_directive(default_level.into())
            .with_env_var("S2N_LOG")
            .from_env()
            .unwrap();

        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .event_format(format)
            .with_test_writer()
            .init();
    });
}

pub fn without_tracing<F: FnOnce() -> T, T>(f: F) -> T {
    // make sure the global subscriber is initialized before setting a local one
    init_tracing();

    static FORCED: OnceLock<bool> = OnceLock::new();

    // add the option to get logs with `S2N_LOG_FORCED=1`
    if *FORCED.get_or_init(|| std::env::var("S2N_LOG_FORCED").is_ok()) {
        return f();
    }

    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::new(), f)
}

#[derive(Default)]
struct Uptime(tracing_subscriber::fmt::time::SystemTime);

// Generate the timestamp from the testing IO provider rather than wall clock.
impl tracing_subscriber::fmt::time::FormatTime for Uptime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        if bach::is_active() {
            let thread = std::thread::current();
            let name = thread.name().unwrap_or("");
            if ["main", ""].contains(&name) {
                write!(w, "{}", bach::time::Instant::now())
            } else {
                write!(w, "{} [{name}]", bach::time::Instant::now())
            }
        } else {
            self.0.format_time(w)
        }
    }
}

/// Runs a function in a deterministic, discrete event simulation environment
pub fn sim(f: impl FnOnce()) {
    init_tracing();

    // 1ms RTT
    let net_delay = Duration::from_micros(500);
    let queues = bach::environment::net::queue::Fixed::default().with_net_latency(net_delay);
    let mut rt = bach::environment::default::Runtime::new().with_net_queues(Some(Box::new(queues)));
    rt.run(f);
}

pub(crate) fn query_event(_connection: &mut Connection, _limiter_duration: Duration) {}

#[derive(Clone, Default)]
pub struct NoopSubscriber;

impl Subscriber for NoopSubscriber {
    /// The context type associated with each connection
    /// For a no-op subscriber, we can use the unit type since we don't need to store any state
    type ConnectionContext = ();

    /// Creates a context to be passed to each connection-related event
    fn create_connection_context(
        &self,
        _meta: &event::api::ConnectionMeta,
        _info: &event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }
}

impl s2n_quic_core::event::Subscriber for NoopSubscriber {
    /// The context type associated with each connection
    /// For a no-op subscriber, we can use the unit type since we don't need to store any state
    type ConnectionContext = ();

    /// Creates a context to be passed to each connection-related event
    fn create_connection_context(
        &mut self,
        _meta: &s2n_quic_core::event::api::ConnectionMeta,
        _info: &s2n_quic_core::event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }
}

#[derive(Default, Clone)]
pub struct TestTlsProvider {}

impl Provider for TestTlsProvider {
    type Server = s2n_quic_tls_prov::Server;
    type Client = s2n_quic_tls_prov::Client;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let server = s2n_quic_tls_prov::Server::builder()
            .with_application_protocols(["h3"].iter())?
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)?
            .build()?;
        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        let client = s2n_quic_tls_prov::Client::builder()
            .with_application_protocols(["h3"].iter())?
            .with_certificate(certificates::CERT_PEM)?
            .build()?;
        Ok(client)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Pair {
    pub client_mtu: Option<u16>,
    pub server_mtu: Option<u16>,
}

impl Pair {
    fn server(&self) -> server::Builder<impl s2n_quic_core::event::Subscriber> {
        let mut server = server::Provider::builder();

        if let Some(mtu) = self.server_mtu {
            server = server.with_mtu(mtu);
        }

        server
    }

    fn client(&self) -> client::Builder<impl s2n_quic_core::event::Subscriber> {
        let mut client = client::Provider::builder();

        if let Some(mtu) = self.client_mtu {
            client = client.with_mtu(mtu);
        }

        client
    }

    pub async fn build(self) -> (client::Provider, server::Provider) {
        init_tracing();

        let tls_materials_provider = TestTlsProvider {};
        let test_event_subscriber = NoopSubscriber {};

        let server = self
            .server()
            .start(
                "[::1]:0".parse().unwrap(),
                tls_materials_provider.clone(),
                test_event_subscriber.clone(),
                Map::new(
                    Signer::new(b"default"),
                    50_000,
                    StdClock::default(),
                    test_event_subscriber.clone(),
                ),
            )
            .await
            .unwrap();

        let client = self
            .client()
            .start(
                "[::]:0".parse().unwrap(),
                Map::new(
                    Signer::new(b"default"),
                    50_000,
                    StdClock::default(),
                    test_event_subscriber.clone(),
                ),
                tls_materials_provider,
                test_event_subscriber,
                query_event,
                server_name(),
            )
            .unwrap();

        (client, server)
    }

    pub fn build_sync(self) -> (client::Provider, server::Provider) {
        init_tracing();

        let tls_materials_provider = TestTlsProvider {};
        let test_event_subscriber = NoopSubscriber {};

        let server = self
            .server()
            .start_blocking(
                "[::1]:0".parse().unwrap(),
                tls_materials_provider.clone(),
                test_event_subscriber.clone(),
                Map::new(
                    Signer::new(b"default"),
                    50_000,
                    StdClock::default(),
                    test_event_subscriber.clone(),
                ),
            )
            .unwrap();

        let client = self
            .client()
            .start(
                "[::]:0".parse().unwrap(),
                Map::new(
                    Signer::new(b"default"),
                    50_000,
                    StdClock::default(),
                    test_event_subscriber.clone(),
                ),
                tls_materials_provider,
                test_event_subscriber,
                query_event,
                server_name(),
            )
            .unwrap();

        (client, server)
    }
}

pub fn pair_sync() -> (client::Provider, server::Provider) {
    Pair::default().build_sync()
}
