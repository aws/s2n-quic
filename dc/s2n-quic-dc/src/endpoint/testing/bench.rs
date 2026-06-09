// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint::{assemble, combinator::AssemblerCounters, counters, frame, id::Id, inflight, send},
    intrusive::{Entry, Queue},
    path::secret::map::Entry as PathSecretEntry,
    socket::{
        channel::ImmediateQueueStatus,
        pool::{self, descriptor::UnsyncRecycler},
    },
    time::bach::Clock,
    xorshift::Rng,
};
use bytes::BytesMut;
use core::time::Duration;
use s2n_quic_core::{packet::number::PacketNumberSpace, time::Clock as _, varint::VarInt};
use s2n_quic_platform::features::{gso::MaxSegments, Gso};
use std::{rc::Rc, sync::Arc};

// Benchmarks target jumbo-frame deployments.
const TEST_MTU: u16 = 9000;
const SOURCE_CONTROL_PORT: u16 = 4433;

pub struct AssembleBenchmark {
    context: send::Context,
    clock: Clock,
    send_counters: Rc<counters::Send>,
    assembler_counters: AssemblerCounters,
    pool: pool::Pool,
    header_buf: Vec<u8>,
    cancelled: Queue<frame::Frame>,
    ack_completions: Queue<crate::endpoint::msg::Sender>,
    freed_batch_tx: crate::queue::FreedBatchTx,
    _freed_batch_rx: crate::queue::FreedBatchRx,
    recycle_pool: pool::UnsyncReusePool,
    gso: Gso,
}

impl AssembleBenchmark {
    pub fn new(packets: usize, frames_per_packet: usize, payload_len: usize) -> Self {
        let registry = crate::counter::Registry::default();
        let clock = Clock::default();
        let entry = test_path_secret_entry();
        let mut context = make_context(&entry, &registry, &clock);
        let send_counters =
            counters::Send::new(&registry, crate::endpoint::id::LocalSenderId::from_index(0));
        let assembler_counters = AssemblerCounters::new(&registry);
        let pool = pool::Pool::new(u16::MAX);
        let header_buf = Vec::new();
        let cancelled = Queue::new();
        let ack_completions = Queue::new();
        let (freed_batch_tx, freed_batch_rx) = crate::queue::freed_batch_channel();
        let recycle_pool = pool::UnsyncReusePool::new();
        let gso: Gso = MaxSegments::try_from(1usize).unwrap().into();

        for packet_idx in 0..packets {
            for frame_idx in 0..frames_per_packet {
                context.push_back_frame(benchmark_frame(
                    &entry,
                    packet_idx * frames_per_packet + frame_idx,
                    payload_len,
                ));
            }
        }

        Self {
            context,
            clock,
            send_counters,
            assembler_counters,
            pool,
            header_buf,
            cancelled,
            ack_completions,
            freed_batch_tx,
            _freed_batch_rx: freed_batch_rx,
            recycle_pool,
            gso,
        }
    }

    pub fn run(mut self) -> u64 {
        let mut total_segments = 0u64;
        loop {
            let Some(unfilled) = self.recycle_pool.alloc_or_reuse(&self.pool) else {
                break;
            };
            let credit_pool = crate::credit::Pool::new(crate::credit::Config::default());
            let Some(segments) = assemble::assemble::<UnsyncRecycler, _>(
                &mut self.context,
                ImmediateQueueStatus::Empty,
                &self.clock,
                crate::endpoint::id::LocalSenderId::from_index(0),
                SOURCE_CONTROL_PORT,
                &self.gso,
                unfilled,
                &mut self.header_buf,
                &mut self.cancelled,
                &mut self.ack_completions,
                &mut self.freed_batch_tx,
                &self.assembler_counters,
                &self.send_counters,
                &credit_pool,
            ) else {
                break;
            };
            total_segments += segments.segment_count() as u64;
        }

        total_segments
            .saturating_add(self.cancelled.len() as u64)
            .saturating_add(self.ack_completions.len() as u64)
            .saturating_add(self.context.inflight.has_inflight() as u64)
    }
}

pub struct AckProcessingBenchmark {
    context: send::Context,
    clock: Clock,
    send_counters: Rc<counters::Send>,
    largest_acknowledged: VarInt,
}

impl AckProcessingBenchmark {
    pub fn new(
        packets: usize,
        frames_per_packet: usize,
        payload_len: usize,
        _ack_frames: usize,
    ) -> Self {
        let registry = crate::counter::Registry::default();
        let clock = Clock::default();
        let entry = test_path_secret_entry();
        let mut context = make_context(&entry, &registry, &clock);
        let send_counters =
            counters::Send::new(&registry, crate::endpoint::id::LocalSenderId::from_index(0));

        seed_inflight_packets(
            &mut context,
            &entry,
            &clock,
            packets,
            frames_per_packet,
            payload_len,
        );

        let largest_acknowledged = if packets > 0 {
            VarInt::new((packets - 1) as u64).unwrap()
        } else {
            VarInt::ZERO
        };

        Self {
            context,
            clock,
            send_counters,
            largest_acknowledged,
        }
    }

    pub fn run(mut self) -> u64 {
        let mut completed = Queue::new();
        let mut lost = Queue::new();
        let mut cancelled = Queue::new();
        let mut rng = Rng::new();
        let mut deferred = Vec::new();
        let _ = self.context.process_ack(
            self.largest_acknowledged,
            VarInt::ZERO,
            &[],
            Default::default(),
            Duration::ZERO,
            &self.send_counters,
            &mut completed,
            &mut lost,
            &mut cancelled,
            &self.clock,
            &mut rng,
            &mut deferred,
        );

        completed
            .len()
            .saturating_add(lost.len())
            .saturating_add(cancelled.len())
            .saturating_add(deferred.len())
            .saturating_add(self.context.inflight.has_inflight() as usize) as u64
    }
}

fn test_path_secret_entry() -> Arc<PathSecretEntry> {
    let peer: std::net::SocketAddr = "127.0.0.1:4433".parse().unwrap();
    let entry = PathSecretEntry::builder(peer)
        .socket_sender_count(1)
        .build();
    entry.set_peer_data_addrs(&[peer]);
    entry.update_max_datagram_size(TEST_MTU);
    entry
}

fn make_context(
    entry: &Arc<PathSecretEntry>,
    registry: &crate::counter::Registry,
    clock: &Clock,
) -> send::Context {
    let inflight_gauge = registry.register_queue_gauge("bench.inflight");
    let ack_gauge = registry.register_queue_gauge("bench.ack");
    let pending_gauge = registry.register_queue_gauge("bench.pending");
    send::Context::new(
        entry,
        inflight_gauge,
        ack_gauge,
        pending_gauge,
        crate::endpoint::id::LocalSenderId::from_index(0),
        clock,
    )
    .unwrap()
}

fn benchmark_frame(
    entry: &Arc<PathSecretEntry>,
    idx: usize,
    payload_len: usize,
) -> Entry<frame::Frame> {
    let queue_id = VarInt::new((idx as u64 % 1024) + 1).unwrap();
    Entry::new(frame::Frame {
        header: frame::Header::QueueData {
            queue_pair: crate::packet::datagram::QueuePair {
                source_queue_id: queue_id,
                dest_queue_id: queue_id,
            },
            binding_id: VarInt::from_u8(1),
            offset: VarInt::ZERO,
            is_fin: false,
            dest_acceptor_id: None,
            priority: crate::credit::Priority::default(),
        },
        payload: BytesMut::zeroed(payload_len).into(),
        path_secret_entry: entry.clone(),
        completion: None,
        status: frame::TransmissionStatus::Pending,
        ttl: frame::DEFAULT_TTL,
        enqueued_at: None,
        flow_credits: 0,
    })
}

fn seed_inflight_packets(
    context: &mut send::Context,
    entry: &Arc<PathSecretEntry>,
    clock: &Clock,
    packets: usize,
    frames_per_packet: usize,
    payload_len: usize,
) {
    let now = clock.get_time();
    for packet_idx in 0..packets {
        let rtt = context.rtt_estimator;
        let sent_bytes = ((payload_len.max(1) * frames_per_packet.max(1)) + 64) as u16;
        let cc_info = context.cca.on_packet_sent(now, sent_bytes, false, &rtt);

        let mut frames = Queue::new();
        for frame_idx in 0..frames_per_packet {
            frames.push_back(benchmark_frame(
                entry,
                packet_idx * frames_per_packet + frame_idx,
                payload_len,
            ));
        }

        let pn =
            PacketNumberSpace::Initial.new_packet_number(VarInt::new(packet_idx as u64).unwrap());
        context.inflight.insert(
            pn,
            inflight::Packet::new(
                frames,
                inflight::TransmissionInfo {
                    cc_info,
                    time_sent: now,
                    sent_bytes,
                },
            ),
        );
    }

    context.next_packet_number = VarInt::new(packets as u64).unwrap_or(VarInt::MAX);
}
