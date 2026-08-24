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
// This test uses quiche, which depends on BoringSSL. See the `boringssl` cfg in build.rs.
#[cfg(boringssl)]
mod zero_length_cid_client_connection_migration;

// These tests use the s2n-tls provider specifically (the ClientHelloCallback trait, mTLS
// providers). See the `s2n_tls_provider` cfg in build.rs.
#[cfg(s2n_tls_provider)]
mod ch_callback_connection_info;
#[cfg(s2n_tls_provider)]
mod chain;
#[cfg(s2n_tls_provider)]
mod client_handshake_confirm;
#[cfg(s2n_tls_provider)]
mod dc;
#[cfg(s2n_tls_provider)]
mod dc_connection_close;
// The s2n-tls `fips` feature depends on aws-lc-fips-sys, which can't be built on Windows with the
// MinGW toolchain. See: https://github.com/aws/aws-lc/issues/3207
#[cfg(unix)]
mod fips;
#[cfg(s2n_tls_provider)]
mod mtls;
#[cfg(s2n_tls_provider)]
mod signature_scheme;
// This test uses real OS sockets, which conflicts with bach's simulated time scope on Windows.
#[cfg(not(target_os = "windows"))]
mod prioritized_socket;
