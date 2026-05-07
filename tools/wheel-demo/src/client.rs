// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::Bytes;
use s2n_quic_core::varint::VarInt;
use s2n_quic_dc::{
    byte_vec::ByteVec,
    datagram::batch::Batch,
    flow,
    intrusive_queue::{List, Queue},
    packet::datagram::{partial::PartialDatagram, QueuePair, RoutingInfo},
    path::secret::map::Entry as PathSecretEntry,
    pipeline::CounterRegistry,
    socket::channel,
};
use s2n_quic_platform::features;
use std::{
    io,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tracing::info;

// ── Main Client Entry Point ────────────────────────────────────────────────

pub async fn run<S>(
    handshake_server: SocketAddr,
    num_sockets: usize,
    config: s2n_quic_dc::pipeline::PipelineConfig<'_, S>,
    provider: crate::psk::Client,
) -> io::Result<()>
where
    S: s2n_quic_dc::stream2::Spawner,
{
    info!(
        handshake_server = %handshake_server,
        bandwidth = ?config.overall_send_rate,
        per_socket_bandwidth = ?config.per_socket_send_rate,
        num_sockets,
        packet_size = config.packet_size,
        max_segments_per_batch = config.gso.max_segments(),
        "Starting UDP sender client"
    );

    // Get the local handshake port from the PSK client
    let handshake_local_addr = provider.local_addr()?;

    // Data sockets should bind to handshake_port + 1
    let mut data_bind_addr = handshake_local_addr;
    data_bind_addr.set_port(handshake_local_addr.port() + 1);

    info!(
        handshake_local = %handshake_local_addr,
        data_bind = %data_bind_addr,
        "Binding data sockets"
    );

    // Create send and receive sockets on the data port
    // One send socket per requested socket, but only one recv socket per spawner worker
    // (worker 0 is the dispatch thread, so num_recv = worker_count - 1)
    let send_sockets = s2n_quic_dc::pipeline::create_send_sockets(
        num_sockets,
        data_bind_addr,
        config.gso.clone(),
    )?;
    let num_recv_sockets = config.spawner.worker_count().saturating_sub(1).max(1);
    let recv_sockets =
        s2n_quic_dc::pipeline::create_recv_sockets(num_recv_sockets, data_bind_addr)?;

    let counters = config.counters.clone();
    let packet_size = config.packet_size;
    let gso = config.gso.clone();

    // Set up the bidirectional pipeline
    let pipeline =
        s2n_quic_dc::pipeline::setup_pipeline(config, send_sockets, recv_sockets, || {
            s2n_quic_dc::random::Random::default()
        });

    let wheel_input_tx = pipeline.wheel_input_tx;

    // Spawn datagram generators on tokio runtime
    let num_generators = 1;
    let max_inflight = 1500; // Backpressure limit

    // Perform PSK handshake to get path secret entry
    info!(handshake_server = %handshake_server, "Performing PSK handshake");
    let path_secret_entry = {
        let (peer, _kind) = provider
            .handshake_with_entry(handshake_server, crate::psk::server_name())
            .await?;
        peer.into_raw()
    };
    let data_server = path_secret_entry.data_addr();
    info!(
        handshake_server = %handshake_server,
        data_server = %data_server,
        "PSK handshake complete"
    );

    // Global stream ID counter shared across all generators
    let stream_id_counter = Arc::new(AtomicU64::new(1));

    for gen_id in 0..num_generators {
        let wheel_tx = wheel_input_tx.clone();
        let path_secret_entry = path_secret_entry.clone();
        let counters = counters.clone();
        let gso = gso.clone();
        let flow_allocator = pipeline.queue_allocator.clone();
        let stream_id_counter = stream_id_counter.clone();
        tokio::spawn(async move {
            let mut generator = Generator::new(
                wheel_tx,
                packet_size,
                max_inflight,
                path_secret_entry,
                counters,
                gso,
                flow_allocator,
                stream_id_counter,
            );
            info!(generator_id = gen_id, "Starting datagram generator");
            loop {
                generator.run().await;
            }
        });
    }

    // Drop the original wheel_input_tx so the wheel knows when all generators are done
    drop(wheel_input_tx);

    // Keep main task alive
    std::future::pending::<()>().await;
    Ok(())
}

// ── Stream Management ─────────────────────────────────────────────────────

/// Represents a single stream that manages flow initialization and data transmission
struct Stream {
    wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
    completion_rx: channel::intrusive_queue::datagram_completion::Receiver<PartialDatagram>,
    stream_rx: flow::queue::Stream<
        s2n_quic_dc::pipeline::StreamMsg,
        s2n_quic_dc::pipeline::ControlMsg,
        flow::Handle,
    >,
    control_rx: flow::queue::Control<
        s2n_quic_dc::pipeline::StreamMsg,
        s2n_quic_dc::pipeline::ControlMsg,
        flow::Handle,
    >,
    packet_size: u16,
    offset: VarInt,
    inflight: usize,
    max_inflight: usize,
    path_secret_entry: Arc<PathSecretEntry>,
    gso: features::Gso,
    stream_id: VarInt,
    local_queue_id: VarInt,
    server_queue_id: Option<VarInt>,
    flow_established: bool,
}

impl Stream {
    fn new(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        packet_size: u16,
        max_inflight: usize,
        path_secret_entry: Arc<PathSecretEntry>,
        gso: features::Gso,
        stream_id: VarInt,
        local_queue_id: VarInt,
        control_rx: flow::queue::Control<
            s2n_quic_dc::pipeline::StreamMsg,
            s2n_quic_dc::pipeline::ControlMsg,
            flow::Handle,
        >,
        stream_rx: flow::queue::Stream<
            s2n_quic_dc::pipeline::StreamMsg,
            s2n_quic_dc::pipeline::ControlMsg,
            flow::Handle,
        >,
    ) -> Self {
        let completion_rx = channel::intrusive_queue::datagram_completion::new();
        Self {
            wheel_tx,
            completion_rx,
            stream_rx,
            control_rx,
            packet_size,
            offset: VarInt::ZERO,
            inflight: 0,
            max_inflight,
            path_secret_entry,
            gso,
            stream_id,
            local_queue_id,
            server_queue_id: None,
            flow_established: false,
        }
    }

    /// Send FlowInit packet to the server
    fn send_flow_init(&mut self) {
        let data_addr = self.path_secret_entry.data_addr();
        let mut builder = s2n_quic_dc::datagram::batch::Builder::new(None, data_addr);

        // Create payload for the first packet
        let pn_bytes = 0u64.to_be_bytes();
        let mut payload_data = vec![0u8; self.packet_size as usize];
        let copy_len = pn_bytes.len().min(payload_data.len());
        payload_data[..copy_len].copy_from_slice(&pn_bytes[..copy_len]);
        let payload = Bytes::from(payload_data).into();

        let flow_init = PartialDatagram::new_datagram(
            RoutingInfo::FlowInit {
                source_sender_id: VarInt::MAX, // Sentinel - will be filled by pipeline
                source_queue_id: self.local_queue_id,
                dest_acceptor_id: VarInt::ZERO, // Server acceptor ID
                attempt_id: VarInt::MAX,        // Sentinel - will be filled by pipeline
                stream_id: self.stream_id,
                is_fin: false,
            },
            ByteVec::new(),
            payload,
            self.path_secret_entry.clone(),
            Some(self.completion_rx.sender()),
        );

        let _ = builder.try_push(flow_init.into());
        let batch = builder.finish();
        let _ = self.wheel_tx.send_entry(batch.into());
        self.inflight += 1;

        info!(stream_id = self.stream_id.as_u64(), "Sent FlowInit packet");
    }

    /// Wait for FlowControl packet from server to establish the flow
    async fn wait_for_flow_establishment(&mut self) -> Result<(), ()> {
        // Wait for FlowControl message on the control queue with a timeout
        // The server sends a FlowControl datagram which is delivered as ControlMsg::Frames
        let timeout = tokio::time::Duration::from_secs(5);

        match tokio::time::timeout(timeout, self.control_rx.recv()).await {
            Ok(Ok(msg)) => {
                let _msg = msg.into_inner();
                info!(
                    stream_id = self.stream_id.as_u64(),
                    "Received FlowControl message, flow established"
                );
                self.flow_established = true;
                Ok(())
            }
            Ok(Err(_)) => {
                info!(
                    stream_id = self.stream_id.as_u64(),
                    "Control queue closed before flow establishment"
                );
                Err(())
            }
            Err(_) => {
                info!(
                    stream_id = self.stream_id.as_u64(),
                    "Timeout waiting for flow establishment"
                );
                Err(())
            }
        }
    }

    /// Generate and send data packets
    async fn run(&mut self, generated_counter: Arc<AtomicU64>, completed_counter: Arc<AtomicU64>) {
        use channel::Receiver;

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
                            panic!("completion channel closed");
                        }
                        std::task::Poll::Pending => {
                            break;
                        }
                    }
                }
                self.inflight = self.inflight.saturating_sub(completed);
                completed_counter.fetch_add(completed as _, Ordering::Relaxed);

                // Generate data packets if flow is established
                if !self.flow_established {
                    return std::task::Poll::Pending;
                }

                let mut submitted = 0;
                let mut batches = List::new();
                let data_addr = self.path_secret_entry.data_addr();

                while self.inflight < self.max_inflight {
                    let mut builder = s2n_quic_dc::datagram::batch::Builder::new(None, data_addr);

                    let datagram = self.generate_data_datagram();
                    match builder.try_push(datagram.into()) {
                        Ok(()) => {
                            self.inflight += 1;
                            submitted += 1;
                        }
                        Err(_) => break,
                    }

                    if !builder.is_empty() {
                        batches.push_back(builder.finish().into());
                    }
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

    fn generate_data_datagram(&mut self) -> PartialDatagram {
        let offset = self.offset;
        let offset_u64 = offset.as_u64();
        self.offset = VarInt::new(offset_u64 + self.packet_size as u64).unwrap();

        // Create payload with offset
        let offset_bytes = offset_u64.to_be_bytes();
        let mut payload_data = vec![0u8; self.packet_size as usize];
        let copy_len = offset_bytes.len().min(payload_data.len());
        payload_data[..copy_len].copy_from_slice(&offset_bytes[..copy_len]);
        let payload = Bytes::from(payload_data).into();

        let queue_pair = QueuePair {
            source_queue_id: self.local_queue_id,
            dest_queue_id: self.server_queue_id.unwrap_or(VarInt::ZERO),
        };

        PartialDatagram::new_datagram(
            RoutingInfo::FlowData {
                source_sender_id: VarInt::MAX, // Sentinel - will be filled by pipeline
                queue_pair,
                stream_id: self.stream_id,
                offset,
                is_fin: false,
            },
            ByteVec::new(),
            payload,
            self.path_secret_entry.clone(),
            Some(self.completion_rx.sender()),
        )
    }
}

// ── Stream Generator ─────────────────────────────────────────────────────

struct Generator {
    wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
    packet_size: u16,
    max_inflight: usize,
    path_secret_entry: Arc<PathSecretEntry>,
    gso: features::Gso,
    counters: CounterRegistry,
    flow_allocator: flow::queue::Allocator<
        s2n_quic_dc::pipeline::StreamMsg,
        s2n_quic_dc::pipeline::ControlMsg,
        flow::Handle,
    >,
    stream_id_counter: Arc<AtomicU64>,
}

impl Generator {
    fn new(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        packet_size: u16,
        max_inflight: usize,
        path_secret_entry: Arc<PathSecretEntry>,
        counters: CounterRegistry,
        gso: features::Gso,
        flow_allocator: flow::queue::Allocator<
            s2n_quic_dc::pipeline::StreamMsg,
            s2n_quic_dc::pipeline::ControlMsg,
            flow::Handle,
        >,
        stream_id_counter: Arc<AtomicU64>,
    ) -> Self {
        Self {
            wheel_tx,
            packet_size,
            max_inflight,
            path_secret_entry,
            gso,
            counters,
            flow_allocator,
            stream_id_counter,
        }
    }

    async fn run(&mut self) {
        let generated_counter = self.counters.register("app:generated");
        let completed_counter = self.counters.register("app:completed");

        // For now, just create one stream
        let stream_id =
            VarInt::new(self.stream_id_counter.fetch_add(1, Ordering::Relaxed)).unwrap();

        // Create a client-side flow handle
        let handle = flow::Handle::client(stream_id, self.path_secret_entry.clone());

        // Allocate queue for this flow
        let (queue_control, queue_stream) = self.flow_allocator.alloc_or_grow(handle);
        let local_queue_id = queue_control.queue_id();

        info!(
            stream_id = stream_id.as_u64(),
            local_queue_id = local_queue_id.as_u64(),
            "Allocated queue for new stream"
        );

        let mut stream = Stream::new(
            self.wheel_tx.clone(),
            self.packet_size,
            self.max_inflight,
            self.path_secret_entry.clone(),
            self.gso.clone(),
            stream_id,
            local_queue_id,
            queue_control,
            queue_stream,
        );

        // Send FlowInit
        stream.send_flow_init();

        // Wait for flow establishment
        if stream.wait_for_flow_establishment().await.is_err() {
            info!(stream_id = stream_id.as_u64(), "Flow establishment failed");
            return;
        }

        // Run the stream data loop
        stream.run(generated_counter, completed_counter).await;
    }
}
