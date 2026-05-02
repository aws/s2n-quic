// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! UDP receiver server with composable packet pipeline.
//!
//! This server demonstrates a channel-based architecture for building a reliable
//! datagram protocol similar to AWS's SRD (Scalable Reliable Datagram). The receive
//! pipeline is built from composable stages connected by channels, making it easy to
//! add new functionality like ACKs, congestion control, and retransmission without
//! preserving packet ordering.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  Receive Pipeline (per socket, on busy-poll runtime)        │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │  Socket                                                      │
//! │    ↓                                                         │
//! │  SocketReceiver    ← Allocate descriptors, recv packets     │
//! │    ↓                                                         │
//! │  InspectErr        ← Log socket errors                      │
//! │    ↓                                                         │
//! │  FlattenSegments   ← Unwrap GRO batches into single packets │
//! │    ↓                                                         │
//! │  RouterAdapter     ← Dispatch to stats/ACK generator        │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Future Stages
//!
//! - **Packet Parser**: Decode packet headers, extract packet numbers
//! - **ACK Generator**: Track received packet numbers, send ACK frames
//! - **Loss Detector**: Detect gaps in packet sequence, trigger retransmits
//! - **Congestion Feedback**: Calculate congestion metrics, send to client
//! - **Application Delivery**: Pass packets directly to application (no reordering)
//!
//! # Relationship to Client
//!
//! The client (`client.rs`) has a complementary send pipeline:
//!
//! ```text
//! Generator → Wheel → RoundRobin → Paced → SocketSender
//! ```
//!
//! Together, these form the basis for a reliable datagram protocol with:
//! - **Pacing**: Token bucket rate limiting in send pipeline
//! - **Congestion Control**: Adjustable rate limits based on receiver feedback
//! - **ACK/Retransmission**: ACK generation here + retransmit logic in client
//! - **Completion Tracking**: Weak pointers notify generators when packets are ACK'd
//! - **Out-of-order Delivery**: Packets delivered as received, no reordering buffer
//!
//! See `socket::channel` module for channel primitives and adapters.

use std::{self, io, net::SocketAddr};
use tracing::info;

pub async fn run(
    address: SocketAddr,
    num_sockets: usize,
    disable_gso: bool,
    config: crate::pipeline::PipelineConfig<'_>,
) -> io::Result<()> {
    info!(
        %address,
        num_sockets,
        packet_size = config.packet_size,
        "Starting UDP receiver server"
    );

    // Create one receive socket per busy poll worker (worker 0 is the dispatch thread)
    let num_recv_sockets = config.busy_poll.len().saturating_sub(1).max(1);
    let recv_sockets = crate::pipeline::create_recv_sockets(num_recv_sockets, address)?;
    info!(%address, num_recv_sockets, "All receive sockets bound");

    // Create send sockets for ACKs
    let send_addr: SocketAddr = if address.is_ipv6() {
        "[::]:0".parse().unwrap()
    } else {
        "0.0.0.0:0".parse().unwrap()
    };
    let send_sockets = crate::pipeline::create_send_sockets(num_sockets, send_addr, disable_gso)?;

    // Set up the bidirectional pipeline
    let _pipeline = crate::pipeline::setup_pipeline(config, send_sockets, recv_sockets, || {
        s2n_quic_dc::random::Random::default()
    });

    // Keep main task alive
    std::future::pending::<()>().await;
    Ok(())
}
