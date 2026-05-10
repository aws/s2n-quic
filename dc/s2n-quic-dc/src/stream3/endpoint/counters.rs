// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Registry},
    packet::datagram::ResetTarget,
    stream3::frame::Header,
};

/// Counters for the datagram processing pipeline.
#[derive(Clone)]
pub(crate) struct Dispatch {
    pub rx_data_pkt: Counter,
    pub rx_none: Counter,
    pub rx_init: Counter,
    pub rx_validate: Counter,
    pub rx_init_validate: Counter,
    pub rx_data: Counter,
    pub rx_control: Counter,
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
    pub rx_data_fully_closed: Counter,
    pub rx_data_perm_closed: Counter,

    pub rx_control_ok: Counter,
    pub rx_control_unallocated: Counter,
    pub rx_control_half_closed: Counter,
    pub rx_control_fully_closed: Counter,
    pub rx_control_perm_closed: Counter,

    pub rx_reset_both: Counter,
    pub rx_reset_stream: Counter,
    pub rx_reset_control: Counter,

    pub tx_validate: Counter,
    pub tx_init_validate: Counter,
    pub tx_reset: Counter,
    pub tx_reset_both: Counter,
    pub tx_reset_stream: Counter,
    pub tx_reset_control: Counter,

    pub flow_accepted: Counter,
    pub flow_pending: Counter,

    pub rx_process_err_peer_lookup: Counter,
    pub rx_process_err_decryption: Counter,
    pub rx_process_err_duplicate: Counter,
    pub rx_process_err_missing_sender_id: Counter,
    pub rx_process_err_unsupported_routing: Counter,
}

impl Dispatch {
    pub fn new(counters: &Registry) -> Self {
        Self {
            rx_data_pkt: counters.register("rx.data_pkt"),
            rx_none: counters.register("!rx.none"),
            rx_init: counters.register("rx.init"),
            rx_validate: counters.register("rx.validate"),
            rx_init_validate: counters.register("rx.init_validate"),
            rx_data: counters.register("rx.data"),
            rx_control: counters.register("rx.control"),
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
            rx_data_unallocated: counters.register("!rx.data.unallocated"),
            rx_data_half_closed: counters.register("!rx.data.half_closed"),
            rx_data_fully_closed: counters.register("!rx.data.fully_closed"),
            rx_data_perm_closed: counters.register("rx.data.perm_closed"),

            rx_control_ok: counters.register("rx.control.ok"),
            rx_control_unallocated: counters.register("!rx.control.unallocated"),
            rx_control_half_closed: counters.register("!rx.control.half_closed"),
            rx_control_fully_closed: counters.register("!rx.control.fully_closed"),
            rx_control_perm_closed: counters.register("rx.control.perm_closed"),

            rx_reset_both: counters.register("rx.reset.both"),
            rx_reset_stream: counters.register("rx.reset.stream"),
            rx_reset_control: counters.register("rx.reset.control"),

            tx_validate: counters.register("tx.validate"),
            tx_init_validate: counters.register("tx.init_validate"),
            tx_reset: counters.register("tx.reset"),
            tx_reset_both: counters.register("tx.reset.both"),
            tx_reset_stream: counters.register("tx.reset.stream"),
            tx_reset_control: counters.register("tx.reset.control"),

            flow_accepted: counters.register("flow.accepted"),
            flow_pending: counters.register("flow.pending"),

            rx_process_err_peer_lookup: counters.register("!rx.process.err.peer_lookup"),
            rx_process_err_decryption: counters.register("!rx.process.err.decrypt"),
            rx_process_err_duplicate: counters.register("!rx.process.err.duplicate"),
            rx_process_err_missing_sender_id: counters
                .register("!rx.process.err.missing_sender_id"),
            rx_process_err_unsupported_routing: counters
                .register("!rx.process.err.unsupported_routing"),
        }
    }

    #[inline]
    pub fn on_received_frame(&self, header: &Header) {
        match header {
            Header::FlowInit { .. } => self.rx_init.add(1),
            Header::FlowValidateRequest { .. } => self.rx_validate.add(1),
            Header::FlowInitValidate { .. } => self.rx_init_validate.add(1),
            Header::FlowData { .. } => self.rx_data.add(1),
            Header::FlowControl { .. } => self.rx_control.add(1),
            Header::FlowReset { .. } => self.rx_reset.add(1),
            Header::Control { .. } => self.rx_control.add(1),
        };
    }

    #[inline]
    pub fn on_sent_frame(&self, header: &Header) {
        match header {
            Header::FlowValidateRequest { .. } => self.tx_validate.add(1),
            Header::FlowInitValidate { .. } => self.tx_init_validate.add(1),
            Header::FlowReset { reset_target, .. } => {
                self.tx_reset.add(1);
                match reset_target {
                    ResetTarget::Both => self.tx_reset_both.add(1),
                    ResetTarget::Stream => self.tx_reset_stream.add(1),
                    ResetTarget::Control => self.tx_reset_control.add(1),
                };
            }
            _ => {}
        };
    }
}
