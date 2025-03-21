// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

pub use bach::{ext, rand};

pub mod task {
    pub use bach::task::*;
    pub use tokio::task::yield_now;
}

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

        let default_level = if cfg!(debug_assertions) {
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
pub fn sim(f: impl FnOnce()) {
    init_tracing();

    // 1ms RTT
    let net_delay = Duration::from_micros(500);
    let queues = bach::environment::net::queue::Fixed::default().with_net_latency(net_delay);
    let mut rt = bach::environment::default::Runtime::new().with_net_queues(Some(Box::new(queues)));
    rt.run(f);
}
