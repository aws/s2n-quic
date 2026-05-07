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

use s2n_quic_core::varint::VarInt;
use s2n_quic_dc::acceptor::{Acceptor, PendingAction};
use std::{self, io, net::SocketAddr, sync::Arc};
use tracing::{debug, info};

/// Server acceptor that handles incoming flow initialization requests
struct FlowAcceptor {
    runtime: tokio::runtime::Handle,
}

impl FlowAcceptor {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            runtime: tokio::runtime::Handle::current(),
        })
    }
}

impl Acceptor<s2n_quic_dc::pipeline::FlowInit> for FlowAcceptor {
    fn handle_request(&self, request: s2n_quic_dc::pipeline::FlowInit) {
        // Spawn a task on the tokio runtime to handle this flow
        self.runtime.spawn(async move {
            let stream_id = request.stream_id;
            info!(
                stream_id = stream_id.as_u64(),
                "Flow accepted, spawning handler"
            );

            // Send an empty datagram packet with FlowControl routing to acknowledge the flow
            let peer_addr = request.path_entry.data_addr();
            let mut batch_builder = s2n_quic_dc::datagram::batch::Builder::new(None, peer_addr);

            // Create an empty datagram with FlowControl routing
            let queue_pair = s2n_quic_dc::packet::datagram::QueuePair {
                source_queue_id: request.queue_control.queue_id(),
                dest_queue_id: request.peer_queue_id,
            };

            let control_datagram =
                s2n_quic_dc::packet::datagram::partial::PartialDatagram::new_datagram(
                    s2n_quic_dc::packet::datagram::RoutingInfo::FlowControl {
                        source_sender_id: VarInt::ZERO,
                        queue_pair,
                        stream_id,
                    },
                    s2n_quic_dc::byte_vec::ByteVec::new(),
                    bytes::Bytes::new().into(),
                    request.path_entry.clone(),
                    None,
                );

            // Add to batch and send via wheel
            let _ = batch_builder.try_push(control_datagram.into());
            let batch = batch_builder.finish();
            let _ = request.wheel_tx.send_entry(batch.into());

            info!(
                stream_id = stream_id.as_u64(),
                "Sent FlowControl acknowledgment"
            );

            // Read from the stream queue and drop data (for now)
            loop {
                let msg = request.queue_stream.recv().await;
                match msg {
                    Ok(msg) => match msg.into_inner() {
                        s2n_quic_dc::pipeline::StreamMsg::Data {
                            offset,
                            fin,
                            payload,
                        } => {
                            debug!(
                                stream_id = stream_id.as_u64(),
                                offset = offset.as_u64(),
                                len = payload.len(),
                                fin,
                                "Received stream data (dropping)"
                            );
                            if fin {
                                break;
                            }
                        }
                        s2n_quic_dc::pipeline::StreamMsg::FlowValidated => {
                            info!(stream_id = stream_id.as_u64(), "Flow validated");
                        }
                        s2n_quic_dc::pipeline::StreamMsg::Reset { error_code } => {
                            info!(
                                stream_id = stream_id.as_u64(),
                                error_code = error_code.as_u64(),
                                "Flow reset"
                            );
                            break;
                        }
                    },
                    Err(error) => {
                        info!(
                            stream_id = stream_id.as_u64(),
                            ?error,
                            "Stream queue closed"
                        );
                        break;
                    }
                }
            }

            info!(stream_id = stream_id.as_u64(), "Flow handler completed");
        });
    }

    fn handle_pending(&self, request: s2n_quic_dc::pipeline::FlowInit) -> PendingAction {
        // For now, accept pending requests and request retry
        info!(
            stream_id = request.stream_id.as_u64(),
            "Pending flow - accepting with retry"
        );
        self.handle_request(request);
        PendingAction::AcceptedWithRetry
    }
}

pub async fn run<S>(
    address: SocketAddr,
    num_sockets: usize,
    config: s2n_quic_dc::pipeline::PipelineConfig<'_, S>,
    provider: crate::psk::Server,
) -> io::Result<()>
where
    S: s2n_quic_dc::stream2::Spawner,
{
    info!(
        %address,
        num_sockets,
        packet_size = config.packet_size,
        "Starting UDP receiver server"
    );

    // Register the flow acceptor with ID 0
    let acceptor = FlowAcceptor::new();
    let _acceptor_handle = config
        .acceptor_registry
        .register(VarInt::ZERO, acceptor)
        .expect("Failed to register acceptor");
    info!("Registered flow acceptor with ID 0");

    // Create one receive socket per spawner worker (worker 0 is the dispatch thread)
    let num_recv_sockets = config.spawner.worker_count().saturating_sub(1).max(1);
    let recv_sockets = s2n_quic_dc::pipeline::create_recv_sockets(num_recv_sockets, address)?;
    info!(%address, num_recv_sockets, "All receive sockets bound");

    // Create send sockets for ACKs
    let send_addr: SocketAddr = if address.is_ipv6() {
        "[::]:0".parse().unwrap()
    } else {
        "0.0.0.0:0".parse().unwrap()
    };
    let send_sockets =
        s2n_quic_dc::pipeline::create_send_sockets(num_sockets, send_addr, config.gso.clone())?;

    // Set up the bidirectional pipeline
    let _pipeline =
        s2n_quic_dc::pipeline::setup_pipeline(config, send_sockets, recv_sockets, || {
            s2n_quic_dc::random::Random::default()
        });

    // Keep main task alive
    std::future::pending::<()>().await;
    let _ = provider;
    Ok(())
}
