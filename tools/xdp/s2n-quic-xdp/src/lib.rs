// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

type Result<T = (), E = std::io::Error> = core::result::Result<T, E>;

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

/// Default BPF programs to direct QUIC traffic
pub mod bpf;
/// Primitive types for AF-XDP kernel APIs
pub mod if_xdp;
/// Implementations of the IO traits from [`s2n_quic_core::io`]
pub mod io;
/// Helpers for creating mmap'd regions
pub mod mmap;
/// Structures for tracking ring cursors and synchronizing with the kernel
pub mod ring;
/// Structure for opening and reference counting an AF-XDP socket
pub mod socket;
/// Helpers for making API calls to AF-XDP sockets
pub mod syscall;
/// A set of async tasks responsible for managing ring buffer and queue state
pub mod task;
/// A shared region of memory for holding frame (packet) data
pub mod umem;
