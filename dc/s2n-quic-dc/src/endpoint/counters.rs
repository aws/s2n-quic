// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Registry, Summary, Timer, Unit},
    endpoint::frame::Header,
    flow::queue::ValidationError,
    packet::datagram::ResetTarget,
};
use s2n_quic_core::{frame::ack::EcnCounts, inet::ExplicitCongestionNotification};
use std::sync::Arc;

/// Counters for the datagram receive/dispatch pipeline.
pub(crate) struct Dispatch {
    pub rx_data_pkt: Counter,
    pub rx_init: Counter,
    pub rx_validate: Counter,
    pub rx_init_validate: Counter,
    pub rx_data: Counter,
    pub rx_flow_control: Counter,
    pub rx_pkt_control: Counter,
    pub rx_reset: Counter,

    pub rx_init_dup: Counter,
    pub rx_init_too_old: Counter,
    pub rx_init_retx: Counter,
    pub rx_init_accepted: Counter,
    pub rx_init_accepted_retry: Counter,
    pub rx_init_reject: Counter,
    pub rx_init_no_acceptor: Counter,
    pub rx_init_acceptor_reset: Counter,

    pub rx_validate_ok: Counter,
    pub rx_validate_failed: Counter,
    pub rx_init_validate_ok: Counter,
    pub rx_init_validate_validation_failed: Counter,
    pub rx_init_validate_dispatch_failed: Counter,

    pub rx_data_ok: Counter,
    pub rx_data_unallocated: Counter,
    pub rx_data_half_closed: Counter,
    pub rx_data_credential_mismatch: Counter,
    pub rx_data_stream_id_mismatch: Counter,
    pub rx_data_tombstone: Counter,
    pub rx_data_perm_closed: Counter,

    pub rx_flow_control_ok: Counter,
    pub rx_flow_control_unallocated: Counter,
    pub rx_flow_control_half_closed: Counter,
    pub rx_flow_control_credential_mismatch: Counter,
    pub rx_flow_control_stream_id_mismatch: Counter,
    pub rx_flow_control_tombstone: Counter,
    pub rx_flow_control_perm_closed: Counter,

    pub rx_reset_both: Counter,
    pub rx_reset_stream: Counter,
    pub rx_reset_control: Counter,

    pub rx_res_validate: Counter,
    pub rx_res_init_validate: Counter,
    pub rx_res_reset: Counter,
    pub rx_res_reset_both: Counter,
    pub rx_res_reset_stream: Counter,
    pub rx_res_reset_control: Counter,

    pub flow_accepted: Counter,
    pub flow_pending: Counter,

    pub rx_process_err_peer_lookup: Counter,
    pub rx_process_err_decryption: Counter,
    pub rx_process_err_duplicate: Counter,
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
}

impl Dispatch {
    pub fn new(counters: &Registry) -> Arc<Self> {
        Arc::new(Self {
            rx_data_pkt: counters.register("rx.data_pkt"),
            rx_init: counters.register("rx.init"),
            rx_validate: counters.register("rx.validate"),
            rx_init_validate: counters.register("rx.init_validate"),
            rx_data: counters.register("rx.data"),
            rx_flow_control: counters.register("rx.flow_control"),
            rx_pkt_control: counters.register("rx.pkt_control"),
            rx_reset: counters.register("rx.reset"),

            rx_init_dup: counters.register("!rx.init.dup"),
            rx_init_too_old: counters.register("!rx.init.too_old"),
            rx_init_retx: counters.register("rx.init.retx"),
            rx_init_accepted: counters.register("rx.init.accepted"),
            rx_init_accepted_retry: counters.register("rx.init.accepted_retry"),
            rx_init_reject: counters.register("!rx.init.reject"),
            rx_init_no_acceptor: counters.register("!rx.init.no_acceptor"),
            rx_init_acceptor_reset: counters.register("!rx.init.acceptor_reset"),

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
            rx_data_stream_id_mismatch: counters.register_nominal("!rx.data", "stream_id_mismatch"),
            rx_data_tombstone: counters.register_nominal("rx.data", "tombstone"),
            rx_data_perm_closed: counters.register("rx.data.perm_closed"),

            rx_flow_control_ok: counters.register("rx.flow_control.ok"),
            rx_flow_control_unallocated: counters
                .register_nominal("!rx.flow_control", "unallocated"),
            rx_flow_control_half_closed: counters
                .register_nominal("!rx.flow_control", "half_closed"),
            rx_flow_control_credential_mismatch: counters
                .register_nominal("!rx.flow_control", "credential_mismatch"),
            rx_flow_control_stream_id_mismatch: counters
                .register_nominal("!rx.flow_control", "stream_id_mismatch"),
            rx_flow_control_tombstone: counters.register_nominal("rx.flow_control", "tombstone"),
            rx_flow_control_perm_closed: counters.register("rx.flow_control.perm_closed"),

            rx_reset_both: counters.register("rx.reset.both"),
            rx_reset_stream: counters.register("rx.reset.stream"),
            rx_reset_control: counters.register("rx.reset.control"),

            rx_res_validate: counters.register("rx.res.validate"),
            rx_res_init_validate: counters.register("rx.res.init_validate"),
            rx_res_reset: counters.register("rx.res.reset"),
            rx_res_reset_both: counters.register("rx.res.reset.both"),
            rx_res_reset_stream: counters.register("rx.res.reset.stream"),
            rx_res_reset_control: counters.register("rx.res.reset.control"),

            flow_accepted: counters.register("flow.accepted"),
            flow_pending: counters.register("flow.pending"),

            rx_process_err_peer_lookup: counters.register("!rx.process.err.peer_lookup"),
            rx_process_err_decryption: counters.register("!rx.process.err.decrypt"),
            rx_process_err_duplicate: counters.register("!rx.process.err.duplicate"),
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
        })
    }

    #[inline]
    pub fn on_data_validation_failed(&self, reason: ValidationError) {
        match reason {
            ValidationError::CredentialMismatch => self.rx_data_credential_mismatch.add(1),
            ValidationError::StreamIdMismatch => self.rx_data_stream_id_mismatch.add(1),
            ValidationError::Tombstone => self.rx_data_tombstone.add(1),
        }
    }

    #[inline]
    pub fn on_flow_control_validation_failed(&self, reason: ValidationError) {
        match reason {
            ValidationError::CredentialMismatch => self.rx_flow_control_credential_mismatch.add(1),
            ValidationError::StreamIdMismatch => self.rx_flow_control_stream_id_mismatch.add(1),
            ValidationError::Tombstone => self.rx_flow_control_tombstone.add(1),
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
            Header::FlowInit { .. } => self.rx_init.add(1),
            Header::FlowValidateRequest { .. } => self.rx_validate.add(1),
            Header::FlowInitValidate { .. } => self.rx_init_validate.add(1),
            Header::FlowData { .. } => self.rx_data.add(1),
            Header::FlowControl { .. } => self.rx_flow_control.add(1),
            Header::FlowMaxData { .. } => self.rx_flow_control.add(1),
            Header::FlowReset { .. } => self.rx_reset.add(1),
            Header::Ack { .. } => self.rx_pkt_control.add(1),
        };
    }

    #[inline]
    pub fn on_response_frame(&self, header: &Header) {
        match header {
            Header::FlowValidateRequest { .. } => self.rx_res_validate.add(1),
            Header::FlowInitValidate { .. } => self.rx_res_init_validate.add(1),
            Header::FlowReset { reset_target, .. } => {
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

/// Counters for the send/ACK-processing path.
pub(crate) struct Send {
    pub lost: Counter,
    pub invalid_sender_idx: Counter,
    pub tx_ack_received: Counter,
    pub tx_ack_no_ctx: Counter,
    pub tx_rtt: Timer,
    pub tx_ecn_ect0: Counter,
    pub tx_ecn_ect1: Counter,
    pub tx_ecn_ce: Counter,
}

impl Send {
    pub fn new(counters: &Registry) -> Arc<Self> {
        Arc::new(Self {
            lost: counters.register("!send.lost"),
            invalid_sender_idx: counters.register("!send.invalid_sender_idx"),
            tx_ack_received: counters.register("tx.ack_received"),
            tx_ack_no_ctx: counters.register("!tx.ack_no_ctx"),
            tx_rtt: counters.register_timer("tx.rtt"),
            tx_ecn_ect0: counters.register_nominal("tx.ecn", "ect0"),
            tx_ecn_ect1: counters.register_nominal("tx.ecn", "ect1"),
            tx_ecn_ce: counters.register_nominal("tx.ecn", "ce"),
        })
    }

    #[inline]
    pub fn on_lost(&self, count: u64) {
        self.lost.add(count);
    }

    #[inline]
    pub fn on_invalid_sender_idx(&self) {
        self.invalid_sender_idx.add(1);
    }

    #[inline]
    pub fn on_received_ack(&self) {
        self.tx_ack_received.add(1);
    }

    #[inline]
    pub fn on_received_ack_no_ctx(&self) {
        self.tx_ack_no_ctx.add(1);
    }

    #[inline]
    pub fn on_rtt(&self, rtt: core::time::Duration) {
        self.tx_rtt.record(rtt);
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
}
