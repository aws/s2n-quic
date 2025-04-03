// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    client::Connect,
    provider::{
        self,
        event::events,
        io::testing::{
            self as io, network::Packet, primary, rand, spawn, test, time::delay, Model,
        },
        packet_interceptor::Loss,
    },
    Client, Server,
};
use bytes::Bytes;
use s2n_quic_core::{crypto::tls::testing::certificates, stream::testing::Data};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

#[macro_use]
mod recorder;

mod connection_limits;
mod resumption;
mod setup;
use setup::*;

mod blackhole;
// S2N-TLS and Rustls have different set
// up for client and server. They also result in
// different error code when a gaint Client Hello is
// received by the server.
#[cfg(all(feature = "s2n-quic-tls", not(feature = "provider-tls-fips")))]
mod buffer_limit_rustls;
#[cfg(feature = "s2n-quic-tls")]
mod buffer_limit_s2n_tls;
mod connection_migration;
mod deduplicate;
mod handshake_cid_rotation;
mod interceptor;
mod mtu;
mod no_tls;
mod platform_events;
mod pto;
mod self_test;
mod skip_packets;
mod tls_context;

// TODO: https://github.com/aws/s2n-quic/issues/1726
//
// The rustls tls provider is used on windows and has different
// build options than s2n-tls. We should build the rustls provider with
// mTLS enabled and remove the `cfg(target_os("windows"))`.
#[cfg(not(target_os = "windows"))]
mod chain;
#[cfg(not(target_os = "windows"))]
mod client_handshake_confirm;
#[cfg(not(target_os = "windows"))]
mod dc;
#[cfg(not(target_os = "windows"))]
mod fips;
#[cfg(not(target_os = "windows"))]
mod mtls;

mod exporter;
mod initial_rtt;
mod issue_1361;
mod issue_1427;
mod issue_1464;
mod issue_1717;
mod issue_954;
