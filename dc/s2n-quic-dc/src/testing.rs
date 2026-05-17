// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    path::secret::{Map, stateless_reset::Signer},
    psk::{client, server},
};
use s2n_quic::{provider::tls::Provider, server::Name};
use s2n_quic_core::{crypto::tls::testing::certificates, time::StdClock};
use std::{
    cell::Cell,
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
    time::Duration,
};

#[cfg(test)]
use std::sync::{Arc, Mutex};

pub use bach::{ext, rand};

use s2n_quic::provider::tls::default as s2n_quic_tls_prov;

#[cfg(all(test, not(loom)))]
pub mod loom {
    pub use std::{sync, thread};

    pub mod future {
        use core::{
            future::Future,
            task::{Context, Poll},
        };
        use std::sync::Arc;

        pub fn block_on<F: Future>(future: F) -> F::Output {
            struct ThreadWaker(std::thread::Thread);

            impl std::task::Wake for ThreadWaker {
                fn wake(self: Arc<Self>) {
                    self.0.unpark();
                }

                fn wake_by_ref(self: &Arc<Self>) {
                    self.0.unpark();
                }
            }

            let mut future = std::pin::pin!(future);
            let waker = std::task::Waker::from(Arc::new(ThreadWaker(std::thread::current())));
            let mut cx = Context::from_waker(&waker);

            loop {
                match future.as_mut().poll(&mut cx) {
                    Poll::Ready(output) => return output,
                    Poll::Pending => std::thread::park(),
                }
            }
        }
    }

    pub mod hint {
        pub use core::hint::spin_loop;
    }

    pub fn model<F: 'static + FnOnce() -> R, R>(f: F) -> R {
        f()
    }
}

#[cfg(all(test, loom))]
pub use loom;

pub static SNI: OnceLock<Name> = OnceLock::new();

thread_local! {
    static TRACING_DISABLED_DEPTH: Cell<usize> = const { Cell::new(0) };
    static SNAPSHOT_DISABLED_DEPTH: Cell<usize> = const { Cell::new(0) };
}

static SNAPSHOT_MODE_DEPTH: AtomicUsize = AtomicUsize::new(0);

struct TracingDisabledGuard;

impl TracingDisabledGuard {
    fn enter() -> Self {
        TRACING_DISABLED_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for TracingDisabledGuard {
    fn drop(&mut self) {
        TRACING_DISABLED_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

struct SnapshotDisabledGuard;

impl SnapshotDisabledGuard {
    fn enter() -> Self {
        SNAPSHOT_DISABLED_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for SnapshotDisabledGuard {
    fn drop(&mut self) {
        SNAPSHOT_DISABLED_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Guard that disables tracing for its lifetime.
///
/// Obtain one with [`without_tracing`].  While the guard is live:
/// - `sim` will not produce snapshot output.
/// - A [`tracing::subscriber::NoSubscriber`] is installed on the current
///   thread so metric events are not recorded.
///
/// Dropping the guard restores the previous state.
pub struct WithoutTracingGuard {
    _depth: Option<TracingDisabledGuard>,
    _dispatch: Option<tracing::subscriber::DefaultGuard>,
}

/// Guard that suppresses sim snapshot output for its lifetime.
///
/// Obtain one with [`without_snapshots`].  Tracing remains active;
/// only the insta snapshot capture is skipped.  Dropping the guard restores
/// the previous state.
pub struct WithoutSnapshotsGuard {
    _depth: SnapshotDisabledGuard,
}

pub(crate) fn snapshots_enabled() -> bool {
    SNAPSHOT_MODE_DEPTH.load(Ordering::Relaxed) > 0
}

#[cfg(test)]
struct SnapshotModeGuard;

#[cfg(test)]
impl SnapshotModeGuard {
    fn enter() -> Self {
        SNAPSHOT_MODE_DEPTH.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

#[cfg(test)]
impl Drop for SnapshotModeGuard {
    fn drop(&mut self) {
        SNAPSHOT_MODE_DEPTH.fetch_sub(1, Ordering::Relaxed);
    }
}

#[doc(hidden)]
pub fn server_name() -> Name {
    SNI.get_or_init(|| "localhost".into()).clone()
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

/// Returns a guard that disables tracing for its lifetime.
///
/// ```rust,ignore
/// let _guard = testing::without_tracing();
/// // ... tracing-free work ...
/// // guard drops here, tracing restored
/// ```
pub fn without_tracing() -> WithoutTracingGuard {
    // make sure the global subscriber is initialized before swapping in NoSubscriber
    init_tracing();

    static FORCED: OnceLock<bool> = OnceLock::new();

    // add the option to get logs with `S2N_LOG_FORCED=1`
    if *FORCED.get_or_init(|| std::env::var("S2N_LOG_FORCED").is_ok()) {
        return WithoutTracingGuard {
            _depth: None,
            _dispatch: None,
        };
    }

    WithoutTracingGuard {
        _depth: Some(TracingDisabledGuard::enter()),
        _dispatch: Some(tracing::subscriber::set_default(
            tracing::subscriber::NoSubscriber::new(),
        )),
    }
}

/// Returns a guard that suppresses sim snapshot output for its lifetime.
///
/// Tracing events continue to be emitted to the normal test subscriber;
/// only the insta snapshot capture is skipped so the output of `sim` is not
/// compared against stored snapshots.
///
/// ```rust,ignore
/// let _guard = testing::without_snapshots();
/// testing::sim(|| { ... }); // runs, but no snapshot is taken
/// ```
pub fn without_snapshots() -> WithoutSnapshotsGuard {
    WithoutSnapshotsGuard {
        _depth: SnapshotDisabledGuard::enter(),
    }
}

#[derive(Default)]
struct Uptime(tracing_subscriber::fmt::time::SystemTime);

// Generate the timestamp from the testing IO provider rather than wall clock.
impl tracing_subscriber::fmt::time::FormatTime for Uptime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        if bach::is_active() {
            write!(w, "{}", bach::time::Instant::now())
        } else {
            self.0.format_time(w)
        }
    }
}

/// Runs a function in a deterministic, discrete event simulation environment
#[track_caller]
pub fn sim(f: impl FnOnce()) {
    init_tracing();

    #[cfg(test)]
    {
        if !is_tracing_disabled() && !is_snapshots_disabled() && !is_bolero_fuzzing() {
            return run_sim_with_snapshot(f);
        }
    }

    run_sim(f);
}

fn run_sim(f: impl FnOnce()) {
    // 1ms RTT
    let net_delay = Duration::from_micros(500);
    let queues = bach::environment::net::queue::Fixed::default().with_net_latency(net_delay);
    let mut rt = bach::environment::default::Runtime::new().with_net_queues(Some(Box::new(queues)));
    rt.run(f);
}

#[cfg(test)]
fn is_tracing_disabled() -> bool {
    TRACING_DISABLED_DEPTH.with(|depth| depth.get() > 0)
}

#[cfg(test)]
fn is_snapshots_disabled() -> bool {
    SNAPSHOT_DISABLED_DEPTH.with(|depth| depth.get() > 0)
}

#[cfg(test)]
fn is_bolero_fuzzing() -> bool {
    std::env::var_os("BOLERO_RANDOM_SEED").is_some()
}

#[cfg(test)]
#[track_caller]
fn run_sim_with_snapshot(f: impl FnOnce()) {
    // Derive a stable snapshot suffix from the test thread name (e.g.
    // "endpoint::tasks::tests::context_resolver::same_peer_reuses_context")
    // rather than from the caller file/line, which changes as code moves.
    let suffix = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .replace([':', '/', '\\', '.', ' '], "_");

    let writer = SnapshotWriter::default();
    let format = tracing_subscriber::fmt::format()
        .with_timer(Uptime::default())
        .with_target(false)
        .compact();
    let env_filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::Level::WARN.into())
        .with_env_var("S2N_LOG")
        .from_env()
        .unwrap()
        .add_directive("s2n_quic_dc::metric=trace".parse().unwrap());
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .event_format(format)
        .with_ansi(false)
        .with_writer(writer.clone())
        .finish();

    let _snapshot_mode_guard = SnapshotModeGuard::enter();
    tracing::subscriber::with_default(subscriber, || run_sim(f));

    let logs = writer.take_string();

    insta::with_settings!({snapshot_suffix => suffix}, {
        insta::assert_snapshot!(logs);
    });
}

#[cfg(test)]
#[derive(Clone, Default)]
struct SnapshotWriter(Arc<Mutex<Vec<u8>>>);

#[cfg(test)]
impl SnapshotWriter {
    fn take_string(&self) -> String {
        let bytes = self.0.lock().unwrap().clone();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[cfg(test)]
struct SnapshotWriteGuard(Arc<Mutex<Vec<u8>>>);

#[cfg(test)]
impl std::io::Write for SnapshotWriteGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SnapshotWriter {
    type Writer = SnapshotWriteGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SnapshotWriteGuard(self.0.clone())
    }
}

#[derive(Clone, Default)]
pub struct NoopSubscriber;

// Need to implement both s2n-quic-dc::event::Subscriber and s2n-quic-core::event::Subscriber
// to fulfill the trait bounds for both client::Provider and server::Provider
impl crate::event::Subscriber for NoopSubscriber {
    type ConnectionContext = ();

    fn create_connection_context(
        &self,
        _meta: &event::api::ConnectionMeta,
        _info: &event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }
}

impl s2n_quic_core::event::Subscriber for NoopSubscriber {
    type ConnectionContext = ();

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

        // Don't wait after previous handshake before trying another one.
        //
        // Primarily this is needed for restart tests, which expect to recover immediately. In
        // production we don't expect to have *just* handshaked with a peer that's restarting (or
        // at least that's uncommon) and peers rarely undergo e.g. deployment in less than 1
        // minute. So not generally an issue there.
        client = client.with_success_jitter(Duration::ZERO);

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
                    false,
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
                    false,
                    StdClock::default(),
                    test_event_subscriber.clone(),
                ),
                tls_materials_provider,
                test_event_subscriber,
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
                    false,
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
                    false,
                    StdClock::default(),
                    test_event_subscriber.clone(),
                ),
                tls_materials_provider,
                test_event_subscriber,
                server_name(),
            )
            .unwrap();

        (client, server)
    }
}

pub fn pair_sync() -> (client::Provider, server::Provider) {
    Pair::default().build_sync()
}

pub fn send_busy_poll() -> crate::busy_poll::Pool {
    static POOL: BusyPool = BusyPool::new();
    POOL.get()
}

pub fn recv_busy_poll() -> crate::busy_poll::Pool {
    static POOL: BusyPool = BusyPool::new();
    POOL.get()
}

struct BusyPool(std::sync::OnceLock<crate::busy_poll::Pool>);

impl BusyPool {
    const fn new() -> Self {
        Self(std::sync::OnceLock::new())
    }

    fn get(&self) -> crate::busy_poll::Pool {
        self.0
            .get_or_init(|| {
                let mut handles = vec![];
                for worker_id in 0..2 {
                    let (handle, runner) = crate::busy_poll::Handle::new(worker_id);
                    std::thread::spawn(move || runner.run());
                    handles.push(handle);
                }
                handles.into()
            })
            .clone()
    }
}
