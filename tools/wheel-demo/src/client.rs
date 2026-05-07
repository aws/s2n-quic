// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::pipeline::CounterRegistry;
use bytes::Bytes;
use s2n_quic_core::varint::VarInt;
use s2n_quic_dc::{
    byte_vec::ByteVec,
    datagram::batch::Batch,
    intrusive_queue::{List, Queue},
    packet::datagram::{partial::PartialDatagram, RoutingInfo},
    path::secret::map::Entry as PathSecretEntry,
    socket::channel,
};
use s2n_quic_platform::features;
use std::{
    io,
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
};
use tracing::info;

// ── Main Client Entry Point ────────────────────────────────────────────────

pub async fn run(
    handshake_server: SocketAddr,
    num_sockets: usize,
    config: crate::pipeline::PipelineConfig<'_>,
) -> io::Result<()> {
    info!(
        handshake_server = %handshake_server,
        bandwidth = ?config.overall_send_rate,
        per_socket_bandwidth = ?config.per_socket_send_rate,
        num_sockets,
        packet_size = config.packet_size,
        max_segments_per_batch = config.gso.max_segments(),
        "Starting UDP sender client"
    );

    // Get the PSK client provider and determine the local handshake port
    let psk_client = match &config.psk_provider {
        crate::pipeline::PskProvider::Client(provider) => provider,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Client requires PSK client provider",
            ))
        }
    };

    // Get the local handshake port from the PSK client
    let handshake_local_addr = psk_client.local_addr()?;

    // Data sockets should bind to handshake_port + 1
    let mut data_bind_addr = handshake_local_addr;
    data_bind_addr.set_port(handshake_local_addr.port() + 1);

    info!(
        handshake_local = %handshake_local_addr,
        data_bind = %data_bind_addr,
        "Binding data sockets"
    );

    // Create send and receive sockets on the data port
    // One send socket per requested socket, but only one recv socket per busy poll worker
    // (worker 0 is the dispatch thread, so num_recv = busy_poll.len() - 1)
    let send_sockets =
        crate::pipeline::create_send_sockets(num_sockets, data_bind_addr, config.gso.clone())?;
    let num_recv_sockets = config.busy_poll.len().saturating_sub(1).max(1);
    let recv_sockets = crate::pipeline::create_recv_sockets(num_recv_sockets, data_bind_addr)?;

    let counters = config.counters.clone();
    let packet_size = config.packet_size;
    let gso = config.gso.clone();

    // Set up the bidirectional pipeline
    let pipeline = crate::pipeline::setup_pipeline(config, send_sockets, recv_sockets, || {
        s2n_quic_dc::random::Random::default()
    });

    let wheel_input_tx = pipeline.wheel_input_tx;

    // Spawn datagram generators on tokio runtime
    let num_generators = 1;
    let max_inflight = 1000; // Backpressure limit

    // Perform PSK handshake to get path secret entry
    info!(handshake_server = %handshake_server, "Performing PSK handshake");
    let path_secret_entry = match &pipeline.psk_provider {
        crate::pipeline::PskProvider::Client(provider) => {
            let (peer, _kind) = provider
                .handshake_with_entry(handshake_server, crate::psk::server_name())
                .await?;
            peer.into_raw()
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Client requires PSK client provider",
            ))
        }
    };
    let data_server = path_secret_entry.data_addr();
    info!(
        handshake_server = %handshake_server,
        data_server = %data_server,
        "PSK handshake complete"
    );

    for gen_id in 0..num_generators {
        let wheel_tx = wheel_input_tx.clone();
        let path_secret_entry = path_secret_entry.clone();
        let counters = counters.clone();
        let gso = gso.clone();
        tokio::spawn(async move {
            let mut generator = Generator::new(
                wheel_tx,
                packet_size,
                max_inflight,
                path_secret_entry,
                counters,
                gso,
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
    packet_size: u16,
    packet_number: u64,
    inflight: usize,
    max_inflight: usize,
    path_secret_entry: Arc<PathSecretEntry>,
    gso: features::Gso,
    counters: CounterRegistry,
}

impl Generator {
    fn new(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        packet_size: u16,
        max_inflight: usize,
        path_secret_entry: Arc<PathSecretEntry>,
        counters: CounterRegistry,
        gso: features::Gso,
    ) -> Self {
        let completion_rx = channel::intrusive_queue::datagram_completion::new();
        Self {
            wheel_tx,
            completion_rx,
            packet_size,
            packet_number: 0,
            inflight: 0,
            max_inflight,
            path_secret_entry,
            gso,
            counters,
        }
    }

    async fn run(&mut self) {
        use channel::Receiver;
        let generated_counter = self.counters.register("app:generated");
        let completed_counter = self.counters.register("app:completed");

        tracing::debug!("starting");

        loop {
            tracing::debug!("loop");
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
                            panic!("completion channel closed");
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
                let max_segments_per_batch = self.gso.max_segments();

                let mut submitted = 0;
                let mut batches = List::new();
                for _ in 0..100 {
                    // Create one datagram batch (multiple datagrams to same peer)
                    // Use data_addr from the path secret entry
                    let data_addr = self.path_secret_entry.data_addr();
                    let mut builder = s2n_quic_dc::datagram::batch::Builder::new(None, data_addr);

                    // Fill the batch with datagrams
                    while self.inflight < self.max_inflight
                        && builder.len() < max_segments_per_batch
                    {
                        let datagram = self.generate_datagram();
                        match builder.try_push(datagram.into()) {
                            Ok(()) => {
                                self.inflight += 1;
                                submitted += 1;
                            }
                            Err(_datagram) => {
                                // Batch is full, stop adding to this batch
                                break;
                            }
                        }
                    }

                    // Submit this datagram batch to the wheel
                    if !builder.is_empty() {
                        batches.push_back(builder.finish().into());
                    }

                    // Check if we should continue generating more batches
                    if self.inflight >= self.max_inflight {
                        // Hit limit, wait for completions
                        break;
                    }
                    // Otherwise loop to generate another batch
                }

                if !batches.is_empty() {
                    let _ = self.wheel_tx.send_batch(batches);
                }

                if submitted > 0 {
                    generated_counter.fetch_add(submitted, Ordering::Relaxed);
                }

                if self.inflight < self.max_inflight {
                    cx.waker().wake_by_ref();
                }
                std::task::Poll::Pending
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

        let payload = Bytes::from(payload_data).into();

        // Get completion sender for this datagram
        let completion_sender = Some(self.completion_rx.sender());

        PartialDatagram::new_datagram(
            RoutingInfo::FlowData {
                source_sender_id: VarInt::ZERO,
                queue_pair: s2n_quic_dc::packet::datagram::QueuePair {
                    source_queue_id: VarInt::ZERO,
                    dest_queue_id: VarInt::ZERO,
                },
                stream_id: VarInt::ZERO,
                offset: VarInt::ZERO,
                is_fin: false,
            },
            ByteVec::new(),
            payload,
            self.path_secret_entry.clone(),
            completion_sender,
        )
    }
}
