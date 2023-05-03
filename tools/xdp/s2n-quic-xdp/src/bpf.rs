// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use aya::include_bytes_aligned;

/// The default BPF program to direct QUIC traffic
pub static DEFAULT_PROGRAM: &[u8] = {
    #[cfg(target_endian = "little")]
    let prog = include_bytes_aligned!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/bpf/s2n-quic-xdp-bpfel.ebpf"
    ));

    #[cfg(target_endian = "big")]
    let prog = include_bytes_aligned!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/bpf/s2n-quic-xdp-bpfeb.ebpf"
    ));

    prog
};

/// The default BPF program to direct QUIC traffic with tracing enabled
pub static DEFAULT_PROGRAM_TRACE: &[u8] = {
    #[cfg(target_endian = "little")]
    let prog = include_bytes_aligned!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/bpf/s2n-quic-xdp-bpfel-trace.ebpf"
    ));

    #[cfg(target_endian = "big")]
    let prog = include_bytes_aligned!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/bpf/s2n-quic-xdp-bpfeb-trace.ebpf"
    ));

    prog
};

/// The name of the default XDP program
pub static PROGRAM_NAME: &str = "s2n_quic_xdp";

/// The name of the AF_XDP socket map
pub static XSK_MAP_NAME: &str = "S2N_QUIC_XDP_SOCKETS";

/// The name of the port map
pub static PORT_MAP_NAME: &str = "S2N_QUIC_XDP_PORTS";
