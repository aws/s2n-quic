// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Gauge, Registry, Summary, Timer, Unit},
    endpoint::{frame::Header, id::LocalSenderId},
    flow::queue::ValidationError,
    packet::datagram::ResetTarget,
};
use s2n_quic_core::{frame::ack::EcnCounts, inet::ExplicitCongestionNotification};
use std::{rc::Rc, sync::Arc};

pub mod os;

/// Counters for the datagram receive/dispatch pipeline.
pub(crate) struct Dispatch {
    pub rx_data_pkt: Counter,

    pub rx_init_dup: Counter,
    pub rx_init_too_old: Counter,
    pub rx_init_retx: Counter,
    pub rx_init_accepted_retry: Counter,
    pub rx_init_no_acceptor: Counter,
    pub rx_init_acceptor_closed: Counter,
    pub rx_init_acceptor_no_slots: Counter,

    pub rx_validate_ok: Counter,
    pub rx_validate_failed: Counter,
    pub rx_init_validate_ok: Counter,
    pub rx_init_validate_validation_failed: Counter,
    pub rx_init_validate_dispatch_failed: Counter,

    pub rx_data_ok: Counter,
    pub rx_data_unallocated: Counter,
    pub rx_data_half_closed: Counter,
    pub rx_data_credential_mismatch: Counter,
    pub rx_data_binding_id_mismatch: Counter,
    pub rx_data_perm_closed: Counter,

    pub rx_queue_control_ok: Counter,
    pub rx_queue_control_unallocated: Counter,
    pub rx_queue_control_half_closed: Counter,
    pub rx_queue_control_credential_mismatch: Counter,
    pub rx_queue_control_binding_id_mismatch: Counter,
    pub rx_queue_control_perm_closed: Counter,

    pub rx_reset_both: Counter,
    pub rx_reset_stream: Counter,
    pub rx_reset_control: Counter,
    pub rx_init_reset_unknown: Counter,
    pub rx_init_fin_unknown: Counter,

    pub rx_res_validate: Counter,
    pub rx_res_init_validate: Counter,
    pub rx_res_reset: Counter,
    pub rx_res_reset_both: Counter,
    pub rx_res_reset_stream: Counter,
    pub rx_res_reset_control: Counter,

    pub queue_accepted: Counter,
    pub queue_pending: Counter,

    pub rx_process_err_peer_lookup: Counter,
    pub rx_process_err_decryption: Counter,
    pub rx_process_err_duplicate: Counter,
    pub rx_process_err_stale_key: Counter,
    pub rx_process_err_missing_sender_id: Counter,
    pub rx_ack_state_impossible: Counter,
    pub rx_ack_completion_impossible: Counter,

    pub rx_peer_cache_hit: Counter,
    pub rx_peer_cache_miss: Counter,
    pub rx_peer_lookup_time: Timer,
    pub rx_decrypt_time: Timer,
    pub rx_dispatch_time: Timer,
    pub rx_init_register_time: Timer,
    pub rx_init_create_stream_time: Timer,
    pub rx_init_dispatch_time: Timer,
    pub rx_frames_per_packet: Summary,
    pub rx_packet_size: Summary,

    pub rx_ecn_ect0: Counter,
    pub rx_ecn_ect1: Counter,
    pub rx_ecn_ce: Counter,
    pub rx_ecn_not_ect: Counter,

    // Per-frame-type RX counters for inbound frames observed by the dispatch path.
    pub rx_frame_queue_init: Counter,
    pub rx_frame_queue_data: Counter,
    pub rx_frame_queue_data_fin: Counter,
    pub rx_frame_queue_control: Counter,
    pub rx_frame_queue_max_data: Counter,
    pub rx_frame_queue_reset: Counter,
    pub rx_frame_queue_init_reset: Counter,
    pub rx_frame_queue_init_fin: Counter,
    pub rx_frame_queue_init_validate: Counter,
    pub rx_frame_queue_validate_request: Counter,
    pub rx_frame_ack: Counter,
}

impl Dispatch {
    pub fn new(counters: &Registry) -> Arc<Self> {
        Arc::new(Self {
            rx_data_pkt: counters.register("rx.data_pkt"),

            rx_init_dup: counters.register("!rx.init.dup"),
            rx_init_too_old: counters.register("!rx.init.too_old"),
            rx_init_retx: counters.register("rx.init.retx"),
            rx_init_accepted_retry: counters.register("rx.init.accepted_retry"),
            rx_init_no_acceptor: counters.register("!rx.init.no_acceptor"),
            rx_init_acceptor_closed: counters.register("!rx.init.acceptor_closed"),
            rx_init_acceptor_no_slots: counters.register("!rx.init.acceptor_no_slots"),

            rx_validate_ok: counters.register("rx.validate.ok"),
            rx_validate_failed: counters.register("!rx.validate.failed"),
            rx_init_validate_ok: counters.register("rx.init_validate.ok"),
            rx_init_validate_validation_failed: counters
                .register("!rx.init_validate.validation_failed"),
            rx_init_validate_dispatch_failed: counters
                .register("!rx.init_validate.dispatch_failed"),

            rx_data_ok: counters.register("rx.data.ok"),
            rx_data_unallocated: counters.register_nominal("!rx.data", "unallocated"),
            rx_data_half_closed: counters.register_nominal("!rx.data", "half_closed"),
            rx_data_credential_mismatch: counters
                .register_nominal("!rx.data", "credential_mismatch"),
            rx_data_binding_id_mismatch: counters.register_nominal("!rx.data", "binding_id_mismatch"),
            rx_data_perm_closed: counters.register("rx.data.perm_closed"),

            rx_queue_control_ok: counters.register("rx.queue_control.ok"),
            rx_queue_control_unallocated: counters
                .register_nominal("!rx.queue_control", "unallocated"),
            rx_queue_control_half_closed: counters
                .register_nominal("!rx.queue_control", "half_closed"),
            rx_queue_control_credential_mismatch: counters
                .register_nominal("!rx.queue_control", "credential_mismatch"),
            rx_queue_control_binding_id_mismatch: counters
                .register_nominal("!rx.queue_control", "binding_id_mismatch"),
            rx_queue_control_perm_closed: counters.register("rx.queue_control.perm_closed"),

            rx_reset_both: counters.register("rx.reset.both"),
            rx_reset_stream: counters.register("rx.reset.stream"),
            rx_reset_control: counters.register("rx.reset.control"),
            rx_init_reset_unknown: counters.register("!rx.init_reset.unknown"),
            rx_init_fin_unknown: counters.register("!rx.init_fin.unknown"),

            rx_res_validate: counters.register("rx.res.validate"),
            rx_res_init_validate: counters.register("rx.res.init_validate"),
            rx_res_reset: counters.register("rx.res.reset"),
            rx_res_reset_both: counters.register("rx.res.reset.both"),
            rx_res_reset_stream: counters.register("rx.res.reset.stream"),
            rx_res_reset_control: counters.register("rx.res.reset.control"),

            queue_accepted: counters.register("queue.accepted"),
            queue_pending: counters.register("queue.pending"),

            rx_process_err_peer_lookup: counters.register("!rx.process.err.peer_lookup"),
            rx_process_err_decryption: counters.register("!rx.process.err.decrypt"),
            rx_process_err_duplicate: counters.register("!rx.process.err.duplicate"),
            rx_process_err_stale_key: counters.register("!rx.process.err.stale_key"),
            rx_process_err_missing_sender_id: counters
                .register("!rx.process.err.missing_sender_id"),
            rx_ack_state_impossible: counters.register("!rx.ack.state_impossible"),
            rx_ack_completion_impossible: counters.register("!rx.ack.completion_impossible"),

            rx_peer_cache_hit: counters.register("rx.peer_cache.hit"),
            rx_peer_cache_miss: counters.register("rx.peer_cache.miss"),
            rx_peer_lookup_time: counters.register_timer("rx.peer_lookup_time"),
            rx_decrypt_time: counters.register_timer("rx.decrypt_time"),
            rx_dispatch_time: counters.register_timer("rx.dispatch_time"),
            rx_init_register_time: counters.register_timer("rx.init.register_time"),
            rx_init_create_stream_time: counters.register_timer("rx.init.create_stream_time"),
            rx_init_dispatch_time: counters.register_timer("rx.init.dispatch_time"),
            rx_frames_per_packet: counters.register_summary("rx.frames_per_packet", Unit::Count),
            rx_packet_size: counters.register_summary("rx.packet_size", Unit::Byte),

            rx_ecn_ect0: counters.register_nominal("rx.ecn", "ect0"),
            rx_ecn_ect1: counters.register_nominal("rx.ecn", "ect1"),
            rx_ecn_ce: counters.register_nominal("rx.ecn", "ce"),
            rx_ecn_not_ect: counters.register_nominal("rx.ecn", "not_ect"),

            rx_frame_queue_init: counters.register_nominal("rx.frame", "queue_init"),
            rx_frame_queue_data: counters.register_nominal("rx.frame", "queue_data"),
            rx_frame_queue_data_fin: counters.register_nominal("rx.frame", "queue_data_fin"),
            rx_frame_queue_control: counters.register_nominal("rx.frame", "queue_control"),
            rx_frame_queue_max_data: counters.register_nominal("rx.frame", "queue_max_data"),
            rx_frame_queue_reset: counters.register_nominal("rx.frame", "queue_reset"),
            rx_frame_queue_init_reset: counters.register_nominal("rx.frame", "queue_init_reset"),
            rx_frame_queue_init_fin: counters.register_nominal("rx.frame", "queue_init_fin"),
            rx_frame_queue_init_validate: counters
                .register_nominal("rx.frame", "queue_init_validate"),
            rx_frame_queue_validate_request: counters
                .register_nominal("rx.frame", "queue_validate_request"),
            rx_frame_ack: counters.register_nominal("rx.frame", "ack"),
        })
    }

    #[inline]
    pub fn on_data_validation_failed(&self, reason: ValidationError) {
        match reason {
            ValidationError::CredentialMismatch => self.rx_data_credential_mismatch.add(1),
            ValidationError::BindingIdMismatch => self.rx_data_binding_id_mismatch.add(1),
        }
    }

    #[inline]
    pub fn on_queue_control_validation_failed(&self, reason: ValidationError) {
        match reason {
            ValidationError::CredentialMismatch => self.rx_queue_control_credential_mismatch.add(1),
            ValidationError::BindingIdMismatch => self.rx_queue_control_binding_id_mismatch.add(1),
        }
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
            Header::QueueInit { .. } => self.rx_frame_queue_init.add(1),
            Header::QueueValidateRequest { .. } => self.rx_frame_queue_validate_request.add(1),
            Header::QueueInitValidate { .. } => self.rx_frame_queue_init_validate.add(1),
            Header::QueueData { is_fin: false, .. } => self.rx_frame_queue_data.add(1),
            Header::QueueData { is_fin: true, .. } => self.rx_frame_queue_data_fin.add(1),
            Header::QueueControl { .. } => self.rx_frame_queue_control.add(1),
            Header::QueueMaxData { .. } => self.rx_frame_queue_max_data.add(1),
            Header::QueueReset { .. } => self.rx_frame_queue_reset.add(1),
            Header::QueueInitReset { .. } => self.rx_frame_queue_init_reset.add(1),
            Header::QueueInitFin { .. } => self.rx_frame_queue_init_fin.add(1),
            Header::Ack { .. } => self.rx_frame_ack.add(1),
        };
    }

    #[inline]
    pub fn on_response_frame(&self, header: &Header) {
        match header {
            Header::QueueValidateRequest { .. } => self.rx_res_validate.add(1),
            Header::QueueInitValidate { .. } => self.rx_res_init_validate.add(1),
            Header::QueueReset { reset_target, .. } => {
                self.rx_res_reset.add(1);
                match reset_target {
                    ResetTarget::Both => self.rx_res_reset_both.add(1),
                    ResetTarget::Stream => self.rx_res_reset_stream.add(1),
                    ResetTarget::Control => self.rx_res_reset_control.add(1),
                };
            }
            _ => {}
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
    pub tx_acked_frame_queue_init: Counter,
    pub tx_acked_frame_queue_data: Counter,
    pub tx_acked_frame_queue_data_fin: Counter,
    pub tx_acked_frame_queue_control: Counter,
    pub tx_acked_frame_queue_max_data: Counter,
    pub tx_acked_frame_queue_reset: Counter,
    pub tx_acked_frame_queue_init_reset: Counter,
    pub tx_acked_frame_queue_init_fin: Counter,
    pub tx_acked_frame_queue_init_validate: Counter,
    pub tx_acked_frame_queue_validate_request: Counter,
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

            tx_acked_frame_queue_init: counters.register_nominal("tx.acked.frame.queue_init", &v),
            tx_acked_frame_queue_data: counters.register_nominal("tx.acked.frame.queue_data", &v),
            tx_acked_frame_queue_data_fin: counters
                .register_nominal("tx.acked.frame.queue_data_fin", &v),
            tx_acked_frame_queue_control: counters
                .register_nominal("tx.acked.frame.queue_control", &v),
            tx_acked_frame_queue_max_data: counters
                .register_nominal("tx.acked.frame.queue_max_data", &v),
            tx_acked_frame_queue_reset: counters.register_nominal("tx.acked.frame.queue_reset", &v),
            tx_acked_frame_queue_init_reset: counters
                .register_nominal("tx.acked.frame.queue_init_reset", &v),
            tx_acked_frame_queue_init_fin: counters
                .register_nominal("tx.acked.frame.queue_init_fin", &v),
            tx_acked_frame_queue_init_validate: counters
                .register_nominal("tx.acked.frame.queue_init_validate", &v),
            tx_acked_frame_queue_validate_request: counters
                .register_nominal("tx.acked.frame.queue_validate_request", &v),
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
            Header::QueueInit { .. } => self.tx_acked_frame_queue_init.add(1),
            Header::QueueData { is_fin: false, .. } => self.tx_acked_frame_queue_data.add(1),
            Header::QueueData { is_fin: true, .. } => self.tx_acked_frame_queue_data_fin.add(1),
            Header::QueueControl { .. } => self.tx_acked_frame_queue_control.add(1),
            Header::QueueMaxData { .. } => self.tx_acked_frame_queue_max_data.add(1),
            Header::QueueReset { .. } => self.tx_acked_frame_queue_reset.add(1),
            Header::QueueInitReset { .. } => self.tx_acked_frame_queue_init_reset.add(1),
            Header::QueueInitFin { .. } => self.tx_acked_frame_queue_init_fin.add(1),
            Header::QueueInitValidate { .. } => self.tx_acked_frame_queue_init_validate.add(1),
            Header::QueueValidateRequest { .. } => self.tx_acked_frame_queue_validate_request.add(1),
            // ACK frames are stripped before inflight insertion and are never ACKed as
            // inflight entries; this branch should be unreachable in practice.
            Header::Ack { .. } => {
                debug_assert!(false, "ACK frames should never appear as inflight entries")
            }
        }
    }
}
