// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::*;
use s2n_quic::{
    client::Connect,
    provider::{
        self,
        event::events,
        io::testing::{
            self as io, network::Packet, primary, rand, spawn, test, time::delay, Model,
        },
        packet_interceptor::Loss,
        tls,
    },
    Client, Server,
};
use s2n_quic_core::{crypto::tls::testing::certificates, stream::testing::Data};

use bytes::Bytes;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

mod blackhole;
mod buffer_limit;
mod connection_limits;
mod connection_migration;
mod deduplicate;
mod endpoint_limits;
mod exporter;
mod handshake_cid_rotation;
mod initial_rtt;
mod interceptor;
mod issue_1361;
mod issue_1427;
mod issue_1464;
mod issue_1717;
mod issue_954;
mod mtu;
mod no_tls;
mod offload;
mod platform_events;
mod pto;
mod resumption;
mod self_test;
mod skip_packets;
mod slow_tls;
mod tls_context;
// quiche also depends on BoringSSL, which does not build with the Windows MinGW-family toolchains.
#[cfg(not(all(target_os = "windows", not(target_env = "msvc"))))]
mod zero_length_cid_client_connection_migration;

// These tests use the s2n-tls provider (e.g. the ClientHelloCallback trait or mTLS providers).
// s2n-tls builds on unix and on Windows with the GNU/MinGW toolchain (target_env = "gnu"), but
// not with MSVC.
#[cfg(any(unix, all(target_os = "windows", target_env = "gnu")))]
mod ch_callback_connection_info;
#[cfg(any(unix, all(target_os = "windows", target_env = "gnu")))]
mod chain;
#[cfg(any(unix, all(target_os = "windows", target_env = "gnu")))]
mod client_handshake_confirm;
#[cfg(any(unix, all(target_os = "windows", target_env = "gnu")))]
mod dc;
#[cfg(any(unix, all(target_os = "windows", target_env = "gnu")))]
mod dc_connection_close;
// s2n-tls fips feature depends on aws-lc-fips-sys which can't be built on Windows with MinGW toolchain.
// see: https://github.com/aws/aws-lc/issues/3207
#[cfg(unix)]
mod fips;
#[cfg(any(unix, all(target_os = "windows", target_env = "gnu")))]
mod mtls;
// This test uses real OS sockets, which conflicts with bach's simulated time scope on Windows.
#[cfg(not(target_os = "windows"))]
mod prioritized_socket;
