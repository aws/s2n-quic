// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A set of async tasks responsible for managing ring buffer and queue state
//!
//! Fundamentally, each task takes a set of input sources and routes them to one or more output
//! queues. Each task is generic over the execution environment, meaning it can be using in
//! something driven by polling for events, like `tokio`, or spawned on its own thread in a busy
//! poll loop.
//!
//! The ordering of operations in each of the tasks is critical for correctness. It's very easy to
//! get into a deadlock if things aren't exactly right. As such, each task has a fuzz test that
//! tries to show the tasks working properly, even in extreme cases.

/// Emits a log line if the `s2n_quic_xdp_trace` cfg option is enabled. Otherwise, the trace is a
/// no-op.
macro_rules! trace {
    ($($fmt:tt)*) => {{
        if cfg!(s2n_quic_xdp_trace) {
            let args = format!($($fmt)*);
            println!("{}:{}: {}", module_path!(), line!(), args);
        }
    }}
}

pub mod completion_to_tx;
pub mod rx;
pub mod rx_to_fill;
pub mod tx;

#[cfg(test)]
mod testing;

pub use completion_to_tx::completion_to_tx;
pub use rx::rx;
pub use rx_to_fill::rx_to_fill;
pub use tx::tx;
