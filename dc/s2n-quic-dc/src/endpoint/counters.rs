// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Gauge, Registry, Summary, Timer, Unit},
    endpoint::{frame::Header, id::LocalSenderId},
};
use s2n_quic_core::{frame::ack::EcnCounts, inet::ExplicitCongestionNotification};
use std::{rc::Rc, sync::Arc};

pub mod os;

/// Counters for the datagram receive/dispatch pipeline.
pub(crate) struct Dispatch {
    pub rx_data_pkt: Counter,

    pub rx_init_no_acceptor: Counter,
    pub rx_init_acceptor_closed: Counter,
    pub rx_init_acceptor_no_slots: Counter,

    pub rx_data_ok: Counter,
    pub rx_data_unallocated: Counter,
    pub rx_data_half_closed: Counter,
    pub rx_data_stale_binding: Counter,
    pub rx_data_future_binding: Counter,

    pub rx_queue_control_ok: Counter,
    pub rx_queue_control_unallocated: Counter,
    pub rx_queue_control_half_closed: Counter,

    pub rx_reset_both: Counter,
    pub rx_reset_stream: Counter,
    pub rx_reset_control: Counter,

    pub queue_accepted: Counter,

    pub rx_process_err_peer_lookup: Counter,
    pub rx_process_err_decryption: Counter,
    pub rx_process_err_duplicate: Counter,
    pub rx_process_err_stale_key: Counter,
    pub rx_process_err_missing_sender_id: Counter,
    pub rx_ack_state_impossible: Counter,
    pub rx_ack_completion_impossible: Counter,
    pub rx_ack_ranges_culled: Counter,

    pub rx_peer_cache_hit: Counter,
    pub rx_peer_cache_miss: Counter,
    pub rx_peer_lookup_time: Timer,
    pub rx_decrypt_time: Timer,
    pub rx_dispatch_time: Timer,
    pub rx_frames_per_packet: Summary,
    pub rx_packet_size: Summary,

    pub rx_ecn_ect0: Counter,
    pub rx_ecn_ect1: Counter,
    pub rx_ecn_ce: Counter,
    pub rx_ecn_not_ect: Counter,

    // Per-frame-type RX counters for inbound frames observed by the dispatch path.
    pub rx_frame_queue_data: Counter,
    pub rx_frame_queue_data_fin: Counter,
    pub rx_frame_queue_control: Counter,
    pub rx_frame_queue_max_data: Counter,
    pub rx_frame_queue_reset: Counter,
    pub rx_frame_queue_free: Counter,
    pub rx_queue_free_slots: Summary,
    pub rx_queue_free_ranges: Summary,
    pub rx_frame_ack: Counter,
    pub rx_frame_ping: Counter,
    pub rx_frame_queue_msg: Counter,
    pub rx_frame_queue_msg_fin: Counter,

    // QueueMsg reassembly metrics
    pub rx_msg_segment_completed: Counter,
    pub rx_msg_segment_size: Summary,
    pub rx_msg_chunks_per_segment: Summary,
}

impl Dispatch {
    pub fn new(counters: &Registry) -> Arc<Self> {
        Arc::new(Self {
            rx_data_pkt: counters.register("rx.data_pkt"),

            rx_init_no_acceptor: counters.register("!rx.init.no_acceptor"),
            rx_init_acceptor_closed: counters.register("!rx.init.acceptor_closed"),
            rx_init_acceptor_no_slots: counters.register("!rx.init.acceptor_no_slots"),

            rx_data_ok: counters.register("rx.data.ok"),
            rx_data_unallocated: counters.register_nominal("!rx.data", "unallocated"),
            rx_data_half_closed: counters.register_nominal("!rx.data", "half_closed"),
            rx_data_stale_binding: counters.register("rx.data.stale_binding"),
            rx_data_future_binding: counters.register_nominal("!rx.data", "future_binding"),

            rx_queue_control_ok: counters.register("rx.queue_control.ok"),
            rx_queue_control_unallocated: counters
                .register_nominal("!rx.queue_control", "unallocated"),
            rx_queue_control_half_closed: counters
                .register_nominal("!rx.queue_control", "half_closed"),

            rx_reset_both: counters.register("rx.reset.both"),
            rx_reset_stream: counters.register("rx.reset.stream"),
            rx_reset_control: counters.register("rx.reset.control"),

            queue_accepted: counters.register("queue.accepted"),

            rx_process_err_peer_lookup: counters.register("!rx.process.err.peer_lookup"),
            rx_process_err_decryption: counters.register("!rx.process.err.decrypt"),
            rx_process_err_duplicate: counters.register("!rx.process.err.duplicate"),
            rx_process_err_stale_key: counters.register("!rx.process.err.stale_key"),
            rx_process_err_missing_sender_id: counters
                .register("!rx.process.err.missing_sender_id"),
            rx_ack_state_impossible: counters.register("!rx.ack.state_impossible"),
            rx_ack_completion_impossible: counters.register("!rx.ack.completion_impossible"),
            rx_ack_ranges_culled: counters.register("rx.ack.ranges_culled"),

            rx_peer_cache_hit: counters.register("rx.peer_cache.hit"),
            rx_peer_cache_miss: counters.register("rx.peer_cache.miss"),
            rx_peer_lookup_time: counters.register_timer("rx.peer_lookup_time"),
            rx_decrypt_time: counters.register_timer("rx.decrypt_time"),
            rx_dispatch_time: counters.register_timer("rx.dispatch_time"),
            rx_frames_per_packet: counters.register_summary("rx.frames_per_packet", Unit::Count),
            rx_packet_size: counters.register_summary("rx.packet_size", Unit::Byte),

            rx_ecn_ect0: counters.register_nominal("rx.ecn", "ect0"),
            rx_ecn_ect1: counters.register_nominal("rx.ecn", "ect1"),
            rx_ecn_ce: counters.register_nominal("rx.ecn", "ce"),
            rx_ecn_not_ect: counters.register_nominal("rx.ecn", "not_ect"),

            rx_frame_queue_data: counters.register_nominal("rx.frame", "queue_data"),
            rx_frame_queue_data_fin: counters.register_nominal("rx.frame", "queue_data_fin"),
            rx_frame_queue_control: counters.register_nominal("rx.frame", "queue_control"),
            rx_frame_queue_max_data: counters.register_nominal("rx.frame", "queue_max_data"),
            rx_frame_queue_reset: counters.register_nominal("rx.frame", "queue_reset"),
            rx_frame_queue_free: counters.register_nominal("rx.frame", "queue_free"),
            rx_queue_free_slots: counters.register_summary("rx.queue_free.slots", Unit::Count),
            rx_queue_free_ranges: counters.register_summary("rx.queue_free.ranges", Unit::Count),
            rx_frame_ack: counters.register_nominal("rx.frame", "ack"),
            rx_frame_ping: counters.register_nominal("rx.frame", "ping"),
            rx_frame_queue_msg: counters.register_nominal("rx.frame", "queue_msg"),
            rx_frame_queue_msg_fin: counters.register_nominal("rx.frame", "queue_msg_fin"),

            rx_msg_segment_completed: counters.register("rx.msg.segment_completed"),
            rx_msg_segment_size: counters.register_summary("rx.msg.segment_size", Unit::Byte),
            rx_msg_chunks_per_segment: counters
                .register_summary("rx.msg.chunks_per_segment", Unit::Count),
        })
    }

    #[inline]
    pub fn on_ecn(&self, ecn: ExplicitCongestionNotification) {
        match ecn {
            ExplicitCongestionNotification::Ect0 => self.rx_ecn_ect0.add(1),
            ExplicitCongestionNotification::Ect1 => self.rx_ecn_ect1.add(1),
            ExplicitCongestionNotification::Ce => self.rx_ecn_ce.add(1),
            ExplicitCongestionNotification::NotEct => self.rx_ecn_not_ect.add(1),
        }
    }

    #[inline]
    pub fn on_received_frame(&self, header: &Header) {
        match header {
            Header::QueueData { is_fin: false, .. } => self.rx_frame_queue_data.add(1),
            Header::QueueData { is_fin: true, .. } => self.rx_frame_queue_data_fin.add(1),
            Header::QueueControl { .. } => self.rx_frame_queue_control.add(1),
            Header::QueueMaxData { .. } => self.rx_frame_queue_max_data.add(1),
            Header::QueueReset { .. } => self.rx_frame_queue_reset.add(1),
            Header::QueueFree { .. } => self.rx_frame_queue_free.add(1),
            Header::Ack { .. } => self.rx_frame_ack.add(1),
            Header::QueueMsg { is_fin: false, .. } => self.rx_frame_queue_msg.add(1),
            Header::QueueMsg { is_fin: true, .. } => self.rx_frame_queue_msg_fin.add(1),
            Header::Ping => self.rx_frame_ping.add(1),
        };
    }
}

/// Per-sender-socket counters for the send/ACK-processing path.
///
/// One instance per `SenderIdx`. All metrics are registered with a nominal variant
/// (`send.{idx}`) so you can query per-sender or aggregate across all senders.
pub(crate) struct Send {
    pub lost: Counter,
    pub ttl_exhausted: Counter,
    pub ack_received: Counter,
    pub ack_no_ctx: Counter,
    pub ack_packets: Summary,
    pub tx_rtt: Timer,
    pub send_cwnd: Summary,
    pub send_pacing_rate: Summary,
    pub send_cca_limited: Counter,
    pub send_app_limited: Counter,
    pub tx_ecn_ect0: Counter,
    pub tx_ecn_ect1: Counter,
    pub tx_ecn_ce: Counter,

    pub inflight_drain_ack: Counter,
    pub inflight_drain_loss: Counter,
    pub inflight_drain_invalidate: Counter,
    pub inflight_drain_expire: Counter,
    pub inflight_leaked_on_invalidate: Summary,
    pub probe_no_response: Counter,
    pub tx_probe_backoff: Summary,
    pub routing_asymmetry: Counter,
    pub context_count: Gauge,
    pub tx_packets: Counter,

    // Per-frame-type ACK counters (bumped when each inflight frame is acknowledged).
    pub tx_acked_frame_queue_data: Counter,
    pub tx_acked_frame_queue_data_fin: Counter,
    pub tx_acked_frame_queue_control: Counter,
    pub tx_acked_frame_queue_max_data: Counter,
    pub tx_acked_frame_queue_reset: Counter,
    pub tx_acked_frame_queue_free: Counter,
    pub tx_acked_frame_queue_msg: Counter,
}

impl Send {
    pub fn new(counters: &Registry, sender_idx: LocalSenderId) -> Rc<Self> {
        let v = format!("send.{sender_idx}");
        Rc::new(Self {
            lost: counters.register_nominal("!send.lost", &v),
            ttl_exhausted: counters.register_nominal("!send.ttl_exhausted", &v),
            ack_received: counters.register_nominal("tx.ack_received", &v),
            ack_no_ctx: counters.register_nominal("!tx.ack_no_ctx", &v),
            ack_packets: counters.register_nominal_summary("tx.ack_packets", &v, Unit::Count),
            tx_rtt: counters.register_nominal_timer("tx.rtt", &v),
            send_cwnd: counters.register_nominal_summary("send.cwnd", &v, Unit::Byte),
            send_pacing_rate: counters.register_nominal_summary("send.pacing_rate", &v, Unit::Byte),
            send_cca_limited: counters.register_nominal("send.cca_limited", &v),
            send_app_limited: counters.register_nominal("send.app_limited", &v),
            tx_ecn_ect0: counters.register_nominal("tx.ecn.ect0", &v),
            tx_ecn_ect1: counters.register_nominal("tx.ecn.ect1", &v),
            tx_ecn_ce: counters.register_nominal("tx.ecn.ce", &v),

            inflight_drain_ack: counters.register_nominal("send.inflight.drain.ack", &v),
            inflight_drain_loss: counters.register_nominal("send.inflight.drain.loss", &v),
            inflight_drain_invalidate: counters
                .register_nominal("send.inflight.drain.invalidate", &v),
            inflight_drain_expire: counters.register_nominal("send.inflight.drain.expire", &v),
            inflight_leaked_on_invalidate: counters.register_nominal_summary(
                "send.inflight.leaked_on_invalidate",
                &v,
                Unit::Byte,
            ),
            probe_no_response: counters.register_nominal("tx.probe.no_response", &v),
            tx_probe_backoff: counters.register_nominal_summary(
                "tx.probe.backoff",
                &v,
                Unit::Count,
            ),
            routing_asymmetry: counters.register_nominal("!send.routing_asymmetry", &v),
            context_count: counters.register_nominal_gauge("send.context.count", &v),
            tx_packets: counters.register_nominal("tx.data", &v),

            tx_acked_frame_queue_data: counters.register_nominal("tx.acked.frame.queue_data", &v),
            tx_acked_frame_queue_data_fin: counters
                .register_nominal("tx.acked.frame.queue_data_fin", &v),
            tx_acked_frame_queue_control: counters
                .register_nominal("tx.acked.frame.queue_control", &v),
            tx_acked_frame_queue_max_data: counters
                .register_nominal("tx.acked.frame.queue_max_data", &v),
            tx_acked_frame_queue_reset: counters.register_nominal("tx.acked.frame.queue_reset", &v),
            tx_acked_frame_queue_free: counters.register_nominal("tx.acked.frame.queue_free", &v),
            tx_acked_frame_queue_msg: counters.register_nominal("tx.acked.frame.queue_msg", &v),
        })
    }

    #[inline]
    pub fn on_lost(&self, count: u64) {
        self.lost.add(count);
    }

    #[inline]
    pub fn on_received_ack(&self) {
        self.ack_received.add(1);
    }

    #[inline]
    pub fn on_received_ack_no_ctx(&self) {
        self.ack_no_ctx.add(1);
    }

    #[inline]
    pub fn on_rtt(&self, rtt: core::time::Duration) {
        self.tx_rtt.record(rtt);
    }

    #[inline]
    pub fn on_cca_state(
        &self,
        cwnd_bytes: u32,
        pacing_rate_bytes_per_sec: u64,
        is_cca_limited: bool,
    ) {
        self.send_cwnd.record_value(cwnd_bytes as u64);
        self.send_pacing_rate
            .record_value(pacing_rate_bytes_per_sec);
        if is_cca_limited {
            self.send_cca_limited.add(1);
        } else {
            self.send_app_limited.add(1);
        }
    }

    #[inline]
    pub fn on_peer_ecn(&self, delta: &EcnCounts) {
        let ect0 = delta.ect_0_count.as_u64();
        let ect1 = delta.ect_1_count.as_u64();
        let ce = delta.ce_count.as_u64();
        if ect0 > 0 {
            self.tx_ecn_ect0.add(ect0);
        }
        if ect1 > 0 {
            self.tx_ecn_ect1.add(ect1);
        }
        if ce > 0 {
            self.tx_ecn_ce.add(ce);
        }
    }

    #[inline]
    pub fn on_inflight_drain_ack(&self, count: u64) {
        self.inflight_drain_ack.add(count);
    }

    #[inline]
    pub fn on_inflight_drain_loss(&self, count: u64) {
        self.inflight_drain_loss.add(count);
    }

    #[inline]
    pub fn on_inflight_drain_invalidate(&self, count: u64) {
        self.inflight_drain_invalidate.add(count);
    }

    #[inline]
    pub fn on_inflight_drain_expire(&self, count: u64) {
        self.inflight_drain_expire.add(count);
    }

    #[inline]
    pub fn on_inflight_leaked_on_invalidate(&self, bytes: u64) {
        if bytes > 0 {
            self.inflight_leaked_on_invalidate.record_value(bytes);
        }
    }

    #[inline]
    pub fn on_probe_no_response(&self) {
        self.probe_no_response.add(1);
    }

    #[inline]
    pub fn on_context_created(&self) {
        self.context_count.add(1);
    }

    #[inline]
    pub fn on_context_removed(&self) {
        self.context_count.sub(1);
    }

    #[inline]
    pub fn on_tx_packet(&self) {
        self.tx_packets.add(1);
    }

    #[inline]
    pub fn on_acked_frame(&self, header: &Header) {
        match header {
            Header::QueueData { is_fin: false, .. } => self.tx_acked_frame_queue_data.add(1),
            Header::QueueData { is_fin: true, .. } => self.tx_acked_frame_queue_data_fin.add(1),
            Header::QueueControl { .. } => self.tx_acked_frame_queue_control.add(1),
            Header::QueueMaxData { .. } => self.tx_acked_frame_queue_max_data.add(1),
            Header::QueueReset { .. } => self.tx_acked_frame_queue_reset.add(1),
            Header::QueueFree { .. } => self.tx_acked_frame_queue_free.add(1),
            Header::Ack { .. } => {
                debug_assert!(false, "ACK frames should never appear as inflight entries")
            }
            Header::Ping => {
                debug_assert!(false, "Ping frames should never appear as inflight entries")
            }
            Header::QueueMsg { .. } => self.tx_acked_frame_queue_msg.add(1),
        }
    }
}
