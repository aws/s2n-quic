// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::pipeline::CounterRegistry;
use bytes::Bytes;
use s2n_quic_core::varint::VarInt;
use s2n_quic_dc::{
    byte_vec::ByteVec,
    datagram::batch::Batch,
    intrusive_queue::Queue,
    packet::{datagram::partial::PartialDatagram, RoutingInfo},
    path::secret::map::Entry as PathSecretEntry,
    socket::channel,
};
use std::{
    io,
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
};
use tracing::info;

// ── Main Client Entry Point ────────────────────────────────────────────────

pub async fn run(
    server: SocketAddr,
    num_sockets: usize,
    disable_gso: bool,
    config: crate::pipeline::PipelineConfig<'_>,
) -> io::Result<()> {
    info!(
        %server,
        bandwidth = ?config.overall_send_rate,
        per_socket_bandwidth = ?config.per_socket_send_rate,
        num_sockets,
        packet_size = config.packet_size,
        disable_gso,
        "Starting UDP sender client"
    );

    // Use same address family as server
    let bind_addr: SocketAddr = if server.is_ipv6() {
        "[::]:0".parse().unwrap()
    } else {
        "0.0.0.0:0".parse().unwrap()
    };

    // Create send and receive sockets
    // One send socket per requested socket, but only one recv socket per busy poll worker
    // (worker 0 is the dispatch thread, so num_recv = busy_poll.len() - 1)
    let send_sockets = crate::pipeline::create_send_sockets(num_sockets, bind_addr, disable_gso)?;
    let num_recv_sockets = config.busy_poll.len().saturating_sub(1).max(1);
    let recv_sockets = crate::pipeline::create_recv_sockets(num_recv_sockets, bind_addr)?;

    let counters = config.counters.clone();
    let packet_size = config.packet_size;

    // Set up the bidirectional pipeline
    let pipeline = crate::pipeline::setup_pipeline(config, send_sockets, recv_sockets, || {
        s2n_quic_dc::random::Random::default()
    });

    let wheel_input_tx = pipeline.wheel_input_tx;

    // Spawn datagram generators on tokio runtime
    let num_generators = 8;
    let max_inflight = 10000; // Backpressure limit

    // Create deterministic path secret entry for all datagrams
    // This must match the server's deterministic entry
    let path_secret_entry =
        PathSecretEntry::fake_deterministic(server, s2n_quic_core::endpoint::Type::Client);

    tokio::time::sleep(core::time::Duration::from_secs(5)).await;

    for gen_id in 0..num_generators {
        let wheel_tx = wheel_input_tx.clone();
        let path_secret_entry = path_secret_entry.clone();
        let counters = counters.clone();
        tokio::spawn(async move {
            let mut generator = Generator::new(
                wheel_tx,
                server,
                packet_size,
                max_inflight,
                path_secret_entry,
                counters,
            );
            info!(generator_id = gen_id, "Starting datagram generator");
            generator.run().await;
        });
    }

    // Drop the original wheel_input_tx so the wheel knows when all generators are done
    drop(wheel_input_tx);

    // Keep main task alive
    std::future::pending::<()>().await;
    Ok(())
}

// ── Datagram Generator ─────────────────────────────────────────────────────

struct Generator {
    wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
    completion_rx: channel::intrusive_queue::datagram_completion::Receiver<PartialDatagram>,
    server_addr: SocketAddr,
    packet_size: u16,
    packet_number: u64,
    inflight: usize,
    max_inflight: usize,
    path_secret_entry: Arc<PathSecretEntry>,
    // Stats
    counters: CounterRegistry,
}

impl Generator {
    fn new(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        server: SocketAddr,
        packet_size: u16,
        max_inflight: usize,
        path_secret_entry: Arc<PathSecretEntry>,
        counters: CounterRegistry,
    ) -> Self {
        let completion_rx = channel::intrusive_queue::datagram_completion::new();
        Self {
            wheel_tx,
            completion_rx,
            server_addr: server,
            packet_size,
            packet_number: 0,
            inflight: 0,
            max_inflight,
            path_secret_entry,
            counters,
        }
    }

    async fn run(&mut self) {
        use channel::Receiver;
        let generated_counter = self.counters.register("app:generated");
        let completed_counter = self.counters.register("app:completed");

        loop {
            () = std::future::poll_fn(|cx| {
                // Drain all available completion queues
                let mut completed = 0;
                loop {
                    match Receiver::<Queue<PartialDatagram>>::poll_recv(&mut self.completion_rx, cx)
                    {
                        std::task::Poll::Ready(Some(queue)) => {
                            completed += queue.len();
                        }
                        std::task::Poll::Ready(None) => {
                            // Channel closed, should not happen
                            break;
                        }
                        std::task::Poll::Pending => {
                            // Waker is now registered
                            break;
                        }
                    }
                }
                self.inflight = self.inflight.saturating_sub(completed);
                completed_counter.fetch_add(completed as _, Ordering::Relaxed);

                // Generate datagram batches until hitting max_inflight
                const DATAGRAMS_PER_BATCH: usize = 10;

                loop {
                    // Create one datagram batch (multiple datagrams to same peer)
                    let mut dgram_batch = Batch::new(None, self.server_addr);

                    // Fill the batch with datagrams
                    while self.inflight < self.max_inflight
                        && dgram_batch.len() < DATAGRAMS_PER_BATCH
                    {
                        let datagram = self.generate_datagram();
                        dgram_batch.push(datagram.into());
                        self.inflight += 1;
                        generated_counter.fetch_add(1, Ordering::Relaxed);
                    }

                    // Submit this datagram batch to the wheel
                    if !dgram_batch.is_empty() {
                        let _ = self.wheel_tx.send_entry(dgram_batch.into());
                    }

                    // Check if we should continue generating more batches
                    if self.inflight >= self.max_inflight {
                        // Hit limit, wait for completions
                        cx.waker().wake_by_ref();
                        return std::task::Poll::Pending;
                    }
                    // Otherwise loop to generate another batch
                }
            })
            .await;
        }
    }

    fn generate_datagram(&mut self) -> PartialDatagram {
        let pn = self.packet_number;
        self.packet_number += 1;

        // Create payload with packet number
        let pn_bytes = pn.to_be_bytes();
        let mut payload_data = vec![0u8; self.packet_size as usize];
        let copy_len = pn_bytes.len().min(payload_data.len());
        payload_data[..copy_len].copy_from_slice(&pn_bytes[..copy_len]);

        let mut payload = ByteVec::new();
        payload.push_back(Bytes::from(payload_data));

        // Get completion sender for this datagram
        let completion_sender = Some(self.completion_rx.sender());

        PartialDatagram::new_datagram(
            RoutingInfo::QueuePair {
                source_sender_id: VarInt::ZERO,
                source_queue_id: VarInt::ZERO,
                dest_queue_id: VarInt::ZERO,
            },
            ByteVec::new(),
            payload,
            self.path_secret_entry.clone(),
            completion_sender,
        )
    }
}
