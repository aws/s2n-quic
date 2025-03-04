// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use bach::{ext, rand};

pub mod task {
    pub use bach::task::*;
    pub use tokio::task::yield_now;
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

/// Runs a function in a deterministic, discrete event simulation environment
pub fn sim(f: impl FnOnce()) {
    init_tracing();

    bach::environment::default::Runtime::new().run(f);
}
