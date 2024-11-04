// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::event::{
    self, api,
    metrics::aggregate::{
        info::{self, Str},
        AsMetric as _, Info, Recorder, Registry,
    },
};
use alloc::{boxed::Box, vec::Vec};
static INFO: &[Info; 112usize] = &[
    info::Builder {
        id: 0usize,
        name: Str::new("application_protocol_information\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 1usize,
        name: Str::new("server_name_information\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 2usize,
        name: Str::new("packet_skipped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 3usize,
        name: Str::new("packet_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 4usize,
        name: Str::new("packet_sent.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 5usize,
        name: Str::new("packet_sent.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 6usize,
        name: Str::new("packet_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 7usize,
        name: Str::new("active_path_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 8usize,
        name: Str::new("path_created\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 9usize,
        name: Str::new("frame_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 10usize,
        name: Str::new("frame_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 11usize,
        name: Str::new("packet_lost\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 12usize,
        name: Str::new("packet_lost.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 13usize,
        name: Str::new("packet_lost.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 14usize,
        name: Str::new("recovery_metrics\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 15usize,
        name: Str::new("recovery_metrics.min_rtt\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 16usize,
        name: Str::new("recovery_metrics.smoothed_rtt\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 17usize,
        name: Str::new("recovery_metrics.latest_rtt\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 18usize,
        name: Str::new("recovery_metrics.rtt_variance\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 19usize,
        name: Str::new("recovery_metrics.max_ack_delay\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 20usize,
        name: Str::new("recovery_metrics.pto_count\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 21usize,
        name: Str::new("recovery_metrics.congestion_window\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 22usize,
        name: Str::new("recovery_metrics.bytes_in_flight\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 23usize,
        name: Str::new("congestion\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 24usize,
        name: Str::new("rx_ack_range_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 25usize,
        name: Str::new("ack_range_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 26usize,
        name: Str::new("ack_range_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 27usize,
        name: Str::new("packet_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 28usize,
        name: Str::new("key_update\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 29usize,
        name: Str::new("key_space_discarded\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 30usize,
        name: Str::new("connection_started\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 31usize,
        name: Str::new("connection_closed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 32usize,
        name: Str::new("duplicate_packet\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 33usize,
        name: Str::new("transport_parameters_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 34usize,
        name: Str::new("datagram_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 35usize,
        name: Str::new("datagram_sent.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 36usize,
        name: Str::new("datagram_sent.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 37usize,
        name: Str::new("datagram_sent.gso_offset\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 38usize,
        name: Str::new("datagram_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 39usize,
        name: Str::new("datagram_received.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 40usize,
        name: Str::new("datagram_received.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 41usize,
        name: Str::new("datagram_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 42usize,
        name: Str::new("datagram_dropped.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 43usize,
        name: Str::new("datagram_dropped.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 44usize,
        name: Str::new("connection_id_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 45usize,
        name: Str::new("ecn_state_changed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 46usize,
        name: Str::new("connection_migration_denied\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 47usize,
        name: Str::new("handshake_status_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 48usize,
        name: Str::new("tls_exporter_ready\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 49usize,
        name: Str::new("path_challenge_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 50usize,
        name: Str::new("tls_client_hello\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 51usize,
        name: Str::new("tls_server_hello\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 52usize,
        name: Str::new("rx_stream_progress\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 53usize,
        name: Str::new("rx_stream_progress.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 54usize,
        name: Str::new("rx_stream_progress.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 55usize,
        name: Str::new("tx_stream_progress\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 56usize,
        name: Str::new("tx_stream_progress.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 57usize,
        name: Str::new("tx_stream_progress.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 58usize,
        name: Str::new("keep_alive_timer_expired\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 59usize,
        name: Str::new("mtu_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 60usize,
        name: Str::new("mtu_updated.mtu\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 61usize,
        name: Str::new("slow_start_exited\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 62usize,
        name: Str::new("slow_start_exited.congestion_window\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 63usize,
        name: Str::new("delivery_rate_sampled\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 64usize,
        name: Str::new("pacing_rate_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 65usize,
        name: Str::new("pacing_rate_updated.bytes_per_second\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 66usize,
        name: Str::new("pacing_rate_updated.burst_size\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 67usize,
        name: Str::new("bbr_state_changed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 68usize,
        name: Str::new("dc_state_changed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 69usize,
        name: Str::new("version_information\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 70usize,
        name: Str::new("endpoint_packet_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 71usize,
        name: Str::new("endpoint_packet_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 72usize,
        name: Str::new("endpoint_datagram_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 73usize,
        name: Str::new("endpoint_datagram_sent.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 74usize,
        name: Str::new("endpoint_datagram_sent.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 75usize,
        name: Str::new("endpoint_datagram_sent.gso_offset\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 76usize,
        name: Str::new("endpoint_datagram_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 77usize,
        name: Str::new("endpoint_datagram_received.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 78usize,
        name: Str::new("endpoint_datagram_received.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 79usize,
        name: Str::new("endpoint_datagram_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 80usize,
        name: Str::new("endpoint_datagram_dropped.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 81usize,
        name: Str::new("endpoint_datagram_dropped.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 82usize,
        name: Str::new("endpoint_connection_attempt_failed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 83usize,
        name: Str::new("platform_tx\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 84usize,
        name: Str::new("platform_tx.packets.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 85usize,
        name: Str::new("platform_tx.packets\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 86usize,
        name: Str::new("platform_tx.syscalls.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 87usize,
        name: Str::new("platform_tx.syscalls\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 88usize,
        name: Str::new("platform_tx.syscalls.blocked.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 89usize,
        name: Str::new("platform_tx.syscalls.blocked\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 90usize,
        name: Str::new("platform_tx.errors.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 91usize,
        name: Str::new("platform_tx.errors\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 92usize,
        name: Str::new("platform_tx.errors.dropped.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 93usize,
        name: Str::new("platform_tx.errors.dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 94usize,
        name: Str::new("platform_tx_error\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 95usize,
        name: Str::new("platform_rx\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 96usize,
        name: Str::new("platform_rx.packets.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 97usize,
        name: Str::new("platform_rx.packets\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 98usize,
        name: Str::new("platform_rx.syscalls.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 99usize,
        name: Str::new("platform_rx.syscalls\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 100usize,
        name: Str::new("platform_rx.syscalls.blocked.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 101usize,
        name: Str::new("platform_rx.syscalls.blocked\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 102usize,
        name: Str::new("platform_rx.errors.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 103usize,
        name: Str::new("platform_rx.errors\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 104usize,
        name: Str::new("platform_rx.errors.dropped.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 105usize,
        name: Str::new("platform_rx.errors.dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 106usize,
        name: Str::new("platform_rx_error\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 107usize,
        name: Str::new("platform_feature_configured\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 108usize,
        name: Str::new("platform_event_loop_wakeup\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 109usize,
        name: Str::new("platform_event_loop_sleep\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 110usize,
        name: Str::new("platform_event_loop_sleep.processing_duration\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 111usize,
        name: Str::new("platform_event_loop_started\0"),
        units: Str::new("\0"),
    }
    .build(),
];
pub struct Subscriber<R: Registry> {
    #[allow(dead_code)]
    counters: Box<[R::Counter; 74usize]>,
    #[allow(dead_code)]
    measures: Box<[R::Measure; 37usize]>,
    #[allow(dead_code)]
    gauges: Box<[R::Gauge; 0usize]>,
    #[allow(dead_code)]
    timers: Box<[R::Timer; 1usize]>,
    #[allow(dead_code)]
    registry: R,
}
impl<R: Registry + Default> Default for Subscriber<R> {
    fn default() -> Self {
        Self::new(R::default())
    }
}
impl<R: Registry> Subscriber<R> {
    #[doc = r" Creates a new subscriber with the given registry"]
    #[doc = r""]
    #[doc = r" # Note"]
    #[doc = r""]
    #[doc = r" All of the recorders are registered on initialization and cached for the lifetime"]
    #[doc = r" of the subscriber."]
    #[allow(unused_mut)]
    #[inline]
    pub fn new(registry: R) -> Self {
        let mut counters = Vec::with_capacity(74usize);
        let mut measures = Vec::with_capacity(37usize);
        let mut gauges = Vec::with_capacity(0usize);
        let mut timers = Vec::with_capacity(1usize);
        counters.push(registry.register_counter(&INFO[0usize]));
        counters.push(registry.register_counter(&INFO[1usize]));
        counters.push(registry.register_counter(&INFO[2usize]));
        counters.push(registry.register_counter(&INFO[3usize]));
        counters.push(registry.register_counter(&INFO[4usize]));
        counters.push(registry.register_counter(&INFO[6usize]));
        counters.push(registry.register_counter(&INFO[7usize]));
        counters.push(registry.register_counter(&INFO[8usize]));
        counters.push(registry.register_counter(&INFO[9usize]));
        counters.push(registry.register_counter(&INFO[10usize]));
        counters.push(registry.register_counter(&INFO[11usize]));
        counters.push(registry.register_counter(&INFO[12usize]));
        counters.push(registry.register_counter(&INFO[14usize]));
        counters.push(registry.register_counter(&INFO[23usize]));
        counters.push(registry.register_counter(&INFO[24usize]));
        counters.push(registry.register_counter(&INFO[25usize]));
        counters.push(registry.register_counter(&INFO[26usize]));
        counters.push(registry.register_counter(&INFO[27usize]));
        counters.push(registry.register_counter(&INFO[28usize]));
        counters.push(registry.register_counter(&INFO[29usize]));
        counters.push(registry.register_counter(&INFO[30usize]));
        counters.push(registry.register_counter(&INFO[31usize]));
        counters.push(registry.register_counter(&INFO[32usize]));
        counters.push(registry.register_counter(&INFO[33usize]));
        counters.push(registry.register_counter(&INFO[34usize]));
        counters.push(registry.register_counter(&INFO[35usize]));
        counters.push(registry.register_counter(&INFO[38usize]));
        counters.push(registry.register_counter(&INFO[39usize]));
        counters.push(registry.register_counter(&INFO[41usize]));
        counters.push(registry.register_counter(&INFO[42usize]));
        counters.push(registry.register_counter(&INFO[44usize]));
        counters.push(registry.register_counter(&INFO[45usize]));
        counters.push(registry.register_counter(&INFO[46usize]));
        counters.push(registry.register_counter(&INFO[47usize]));
        counters.push(registry.register_counter(&INFO[48usize]));
        counters.push(registry.register_counter(&INFO[49usize]));
        counters.push(registry.register_counter(&INFO[50usize]));
        counters.push(registry.register_counter(&INFO[51usize]));
        counters.push(registry.register_counter(&INFO[52usize]));
        counters.push(registry.register_counter(&INFO[53usize]));
        counters.push(registry.register_counter(&INFO[55usize]));
        counters.push(registry.register_counter(&INFO[56usize]));
        counters.push(registry.register_counter(&INFO[58usize]));
        counters.push(registry.register_counter(&INFO[59usize]));
        counters.push(registry.register_counter(&INFO[61usize]));
        counters.push(registry.register_counter(&INFO[63usize]));
        counters.push(registry.register_counter(&INFO[64usize]));
        counters.push(registry.register_counter(&INFO[67usize]));
        counters.push(registry.register_counter(&INFO[68usize]));
        counters.push(registry.register_counter(&INFO[69usize]));
        counters.push(registry.register_counter(&INFO[70usize]));
        counters.push(registry.register_counter(&INFO[71usize]));
        counters.push(registry.register_counter(&INFO[72usize]));
        counters.push(registry.register_counter(&INFO[76usize]));
        counters.push(registry.register_counter(&INFO[79usize]));
        counters.push(registry.register_counter(&INFO[82usize]));
        counters.push(registry.register_counter(&INFO[83usize]));
        counters.push(registry.register_counter(&INFO[84usize]));
        counters.push(registry.register_counter(&INFO[86usize]));
        counters.push(registry.register_counter(&INFO[88usize]));
        counters.push(registry.register_counter(&INFO[90usize]));
        counters.push(registry.register_counter(&INFO[92usize]));
        counters.push(registry.register_counter(&INFO[94usize]));
        counters.push(registry.register_counter(&INFO[95usize]));
        counters.push(registry.register_counter(&INFO[96usize]));
        counters.push(registry.register_counter(&INFO[98usize]));
        counters.push(registry.register_counter(&INFO[100usize]));
        counters.push(registry.register_counter(&INFO[102usize]));
        counters.push(registry.register_counter(&INFO[104usize]));
        counters.push(registry.register_counter(&INFO[106usize]));
        counters.push(registry.register_counter(&INFO[107usize]));
        counters.push(registry.register_counter(&INFO[108usize]));
        counters.push(registry.register_counter(&INFO[109usize]));
        counters.push(registry.register_counter(&INFO[111usize]));
        measures.push(registry.register_measure(&INFO[5usize]));
        measures.push(registry.register_measure(&INFO[13usize]));
        measures.push(registry.register_measure(&INFO[15usize]));
        measures.push(registry.register_measure(&INFO[16usize]));
        measures.push(registry.register_measure(&INFO[17usize]));
        measures.push(registry.register_measure(&INFO[18usize]));
        measures.push(registry.register_measure(&INFO[19usize]));
        measures.push(registry.register_measure(&INFO[20usize]));
        measures.push(registry.register_measure(&INFO[21usize]));
        measures.push(registry.register_measure(&INFO[22usize]));
        measures.push(registry.register_measure(&INFO[36usize]));
        measures.push(registry.register_measure(&INFO[37usize]));
        measures.push(registry.register_measure(&INFO[40usize]));
        measures.push(registry.register_measure(&INFO[43usize]));
        measures.push(registry.register_measure(&INFO[54usize]));
        measures.push(registry.register_measure(&INFO[57usize]));
        measures.push(registry.register_measure(&INFO[60usize]));
        measures.push(registry.register_measure(&INFO[62usize]));
        measures.push(registry.register_measure(&INFO[65usize]));
        measures.push(registry.register_measure(&INFO[66usize]));
        measures.push(registry.register_measure(&INFO[73usize]));
        measures.push(registry.register_measure(&INFO[74usize]));
        measures.push(registry.register_measure(&INFO[75usize]));
        measures.push(registry.register_measure(&INFO[77usize]));
        measures.push(registry.register_measure(&INFO[78usize]));
        measures.push(registry.register_measure(&INFO[80usize]));
        measures.push(registry.register_measure(&INFO[81usize]));
        measures.push(registry.register_measure(&INFO[85usize]));
        measures.push(registry.register_measure(&INFO[87usize]));
        measures.push(registry.register_measure(&INFO[89usize]));
        measures.push(registry.register_measure(&INFO[91usize]));
        measures.push(registry.register_measure(&INFO[93usize]));
        measures.push(registry.register_measure(&INFO[97usize]));
        measures.push(registry.register_measure(&INFO[99usize]));
        measures.push(registry.register_measure(&INFO[101usize]));
        measures.push(registry.register_measure(&INFO[103usize]));
        measures.push(registry.register_measure(&INFO[105usize]));
        timers.push(registry.register_timer(&INFO[110usize]));
        Self {
            counters: counters
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            measures: measures
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            gauges: gauges.try_into().unwrap_or_else(|_| panic!("invalid len")),
            timers: timers.try_into().unwrap_or_else(|_| panic!("invalid len")),
            registry,
        }
    }
    #[doc = r" Returns all of the registered counters"]
    #[inline]
    pub fn counters(&self) -> impl Iterator<Item = (&'static Info, &R::Counter)> + '_ {
        self.counters
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[0usize], entry),
                1usize => (&INFO[1usize], entry),
                2usize => (&INFO[2usize], entry),
                3usize => (&INFO[3usize], entry),
                4usize => (&INFO[4usize], entry),
                5usize => (&INFO[6usize], entry),
                6usize => (&INFO[7usize], entry),
                7usize => (&INFO[8usize], entry),
                8usize => (&INFO[9usize], entry),
                9usize => (&INFO[10usize], entry),
                10usize => (&INFO[11usize], entry),
                11usize => (&INFO[12usize], entry),
                12usize => (&INFO[14usize], entry),
                13usize => (&INFO[23usize], entry),
                14usize => (&INFO[24usize], entry),
                15usize => (&INFO[25usize], entry),
                16usize => (&INFO[26usize], entry),
                17usize => (&INFO[27usize], entry),
                18usize => (&INFO[28usize], entry),
                19usize => (&INFO[29usize], entry),
                20usize => (&INFO[30usize], entry),
                21usize => (&INFO[31usize], entry),
                22usize => (&INFO[32usize], entry),
                23usize => (&INFO[33usize], entry),
                24usize => (&INFO[34usize], entry),
                25usize => (&INFO[35usize], entry),
                26usize => (&INFO[38usize], entry),
                27usize => (&INFO[39usize], entry),
                28usize => (&INFO[41usize], entry),
                29usize => (&INFO[42usize], entry),
                30usize => (&INFO[44usize], entry),
                31usize => (&INFO[45usize], entry),
                32usize => (&INFO[46usize], entry),
                33usize => (&INFO[47usize], entry),
                34usize => (&INFO[48usize], entry),
                35usize => (&INFO[49usize], entry),
                36usize => (&INFO[50usize], entry),
                37usize => (&INFO[51usize], entry),
                38usize => (&INFO[52usize], entry),
                39usize => (&INFO[53usize], entry),
                40usize => (&INFO[55usize], entry),
                41usize => (&INFO[56usize], entry),
                42usize => (&INFO[58usize], entry),
                43usize => (&INFO[59usize], entry),
                44usize => (&INFO[61usize], entry),
                45usize => (&INFO[63usize], entry),
                46usize => (&INFO[64usize], entry),
                47usize => (&INFO[67usize], entry),
                48usize => (&INFO[68usize], entry),
                49usize => (&INFO[69usize], entry),
                50usize => (&INFO[70usize], entry),
                51usize => (&INFO[71usize], entry),
                52usize => (&INFO[72usize], entry),
                53usize => (&INFO[76usize], entry),
                54usize => (&INFO[79usize], entry),
                55usize => (&INFO[82usize], entry),
                56usize => (&INFO[83usize], entry),
                57usize => (&INFO[84usize], entry),
                58usize => (&INFO[86usize], entry),
                59usize => (&INFO[88usize], entry),
                60usize => (&INFO[90usize], entry),
                61usize => (&INFO[92usize], entry),
                62usize => (&INFO[94usize], entry),
                63usize => (&INFO[95usize], entry),
                64usize => (&INFO[96usize], entry),
                65usize => (&INFO[98usize], entry),
                66usize => (&INFO[100usize], entry),
                67usize => (&INFO[102usize], entry),
                68usize => (&INFO[104usize], entry),
                69usize => (&INFO[106usize], entry),
                70usize => (&INFO[107usize], entry),
                71usize => (&INFO[108usize], entry),
                72usize => (&INFO[109usize], entry),
                73usize => (&INFO[111usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count(&self, info: usize, id: usize, value: u64) {
        let info = &INFO[info];
        let counter = &self.counters[id];
        counter.record(info, value);
    }
    #[doc = r" Returns all of the registered measures"]
    #[inline]
    pub fn measures(&self) -> impl Iterator<Item = (&'static Info, &R::Measure)> + '_ {
        self.measures
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[5usize], entry),
                1usize => (&INFO[13usize], entry),
                2usize => (&INFO[15usize], entry),
                3usize => (&INFO[16usize], entry),
                4usize => (&INFO[17usize], entry),
                5usize => (&INFO[18usize], entry),
                6usize => (&INFO[19usize], entry),
                7usize => (&INFO[20usize], entry),
                8usize => (&INFO[21usize], entry),
                9usize => (&INFO[22usize], entry),
                10usize => (&INFO[36usize], entry),
                11usize => (&INFO[37usize], entry),
                12usize => (&INFO[40usize], entry),
                13usize => (&INFO[43usize], entry),
                14usize => (&INFO[54usize], entry),
                15usize => (&INFO[57usize], entry),
                16usize => (&INFO[60usize], entry),
                17usize => (&INFO[62usize], entry),
                18usize => (&INFO[65usize], entry),
                19usize => (&INFO[66usize], entry),
                20usize => (&INFO[73usize], entry),
                21usize => (&INFO[74usize], entry),
                22usize => (&INFO[75usize], entry),
                23usize => (&INFO[77usize], entry),
                24usize => (&INFO[78usize], entry),
                25usize => (&INFO[80usize], entry),
                26usize => (&INFO[81usize], entry),
                27usize => (&INFO[85usize], entry),
                28usize => (&INFO[87usize], entry),
                29usize => (&INFO[89usize], entry),
                30usize => (&INFO[91usize], entry),
                31usize => (&INFO[93usize], entry),
                32usize => (&INFO[97usize], entry),
                33usize => (&INFO[99usize], entry),
                34usize => (&INFO[101usize], entry),
                35usize => (&INFO[103usize], entry),
                36usize => (&INFO[105usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn measure(&self, info: usize, id: usize, value: u64) {
        let info = &INFO[info];
        let measure = &self.measures[id];
        measure.record(info, value);
    }
    #[doc = r" Returns all of the registered gauges"]
    #[inline]
    pub fn gauges(&self) -> impl Iterator<Item = (&'static Info, &R::Gauge)> + '_ {
        core::iter::empty()
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn gauge(&self, info: usize, id: usize, value: u64) {
        let info = &INFO[info];
        let gauge = &self.gauges[id];
        gauge.record(info, value);
    }
    #[doc = r" Returns all of the registered timers"]
    #[inline]
    pub fn timers(&self) -> impl Iterator<Item = (&'static Info, &R::Timer)> + '_ {
        self.timers
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[110usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn time(&self, info: usize, id: usize, value: u64) {
        let info = &INFO[info];
        let timer = &self.timers[id];
        timer.record(info, value);
    }
}
impl<R: Registry> event::Subscriber for Subscriber<R> {
    type ConnectionContext = ();
    fn create_connection_context(
        &mut self,
        _meta: &api::ConnectionMeta,
        _info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }
    #[inline]
    fn on_application_protocol_information(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ApplicationProtocolInformation,
    ) {
        self.count(0usize, 0usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_server_name_information(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ServerNameInformation,
    ) {
        self.count(1usize, 1usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_skipped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketSkipped,
    ) {
        self.count(2usize, 2usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketSent,
    ) {
        self.count(3usize, 3usize, 1);
        self.count(4usize, 4usize, event.packet_len.as_metric("b"));
        self.measure(5usize, 0usize, event.packet_len.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketReceived,
    ) {
        self.count(6usize, 5usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_active_path_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ActivePathUpdated,
    ) {
        self.count(7usize, 6usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_path_created(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PathCreated,
    ) {
        self.count(8usize, 7usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_frame_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::FrameSent,
    ) {
        self.count(9usize, 8usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_frame_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::FrameReceived,
    ) {
        self.count(10usize, 9usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_lost(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketLost,
    ) {
        self.count(11usize, 10usize, 1);
        self.count(12usize, 11usize, event.bytes_lost.as_metric("b"));
        self.measure(13usize, 1usize, event.bytes_lost.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_recovery_metrics(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::RecoveryMetrics,
    ) {
        self.count(14usize, 12usize, 1);
        self.measure(15usize, 2usize, event.min_rtt.as_metric("us"));
        self.measure(16usize, 3usize, event.smoothed_rtt.as_metric("us"));
        self.measure(17usize, 4usize, event.latest_rtt.as_metric("us"));
        self.measure(18usize, 5usize, event.rtt_variance.as_metric("us"));
        self.measure(19usize, 6usize, event.max_ack_delay.as_metric("us"));
        self.measure(20usize, 7usize, event.pto_count.as_metric(""));
        self.measure(21usize, 8usize, event.congestion_window.as_metric("b"));
        self.measure(22usize, 9usize, event.bytes_in_flight.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_congestion(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::Congestion,
    ) {
        self.count(23usize, 13usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_rx_ack_range_dropped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::RxAckRangeDropped,
    ) {
        self.count(24usize, 14usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_ack_range_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::AckRangeReceived,
    ) {
        self.count(25usize, 15usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_ack_range_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::AckRangeSent,
    ) {
        self.count(26usize, 16usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_dropped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketDropped,
    ) {
        self.count(27usize, 17usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_key_update(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeyUpdate,
    ) {
        self.count(28usize, 18usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_key_space_discarded(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeySpaceDiscarded,
    ) {
        self.count(29usize, 19usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_connection_started(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionStarted,
    ) {
        self.count(30usize, 20usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_connection_closed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionClosed,
    ) {
        self.count(31usize, 21usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_duplicate_packet(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DuplicatePacket,
    ) {
        self.count(32usize, 22usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_transport_parameters_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TransportParametersReceived,
    ) {
        self.count(33usize, 23usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_datagram_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DatagramSent,
    ) {
        self.count(34usize, 24usize, 1);
        self.count(35usize, 25usize, event.len.as_metric("b"));
        self.measure(36usize, 10usize, event.len.as_metric("b"));
        self.measure(37usize, 11usize, event.gso_offset.as_metric(""));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_datagram_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DatagramReceived,
    ) {
        self.count(38usize, 26usize, 1);
        self.count(39usize, 27usize, event.len.as_metric("b"));
        self.measure(40usize, 12usize, event.len.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_datagram_dropped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DatagramDropped,
    ) {
        self.count(41usize, 28usize, 1);
        self.count(42usize, 29usize, event.len.as_metric("b"));
        self.measure(43usize, 13usize, event.len.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_connection_id_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionIdUpdated,
    ) {
        self.count(44usize, 30usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_ecn_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::EcnStateChanged,
    ) {
        self.count(45usize, 31usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_connection_migration_denied(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionMigrationDenied,
    ) {
        self.count(46usize, 32usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_handshake_status_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::HandshakeStatusUpdated,
    ) {
        self.count(47usize, 33usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_tls_exporter_ready(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsExporterReady,
    ) {
        self.count(48usize, 34usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_path_challenge_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PathChallengeUpdated,
    ) {
        self.count(49usize, 35usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_tls_client_hello(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsClientHello,
    ) {
        self.count(50usize, 36usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_tls_server_hello(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsServerHello,
    ) {
        self.count(51usize, 37usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_rx_stream_progress(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::RxStreamProgress,
    ) {
        self.count(52usize, 38usize, 1);
        self.count(53usize, 39usize, event.bytes.as_metric("b"));
        self.measure(54usize, 14usize, event.bytes.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_tx_stream_progress(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TxStreamProgress,
    ) {
        self.count(55usize, 40usize, 1);
        self.count(56usize, 41usize, event.bytes.as_metric("b"));
        self.measure(57usize, 15usize, event.bytes.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_keep_alive_timer_expired(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeepAliveTimerExpired,
    ) {
        self.count(58usize, 42usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_mtu_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::MtuUpdated,
    ) {
        self.count(59usize, 43usize, 1);
        self.measure(60usize, 16usize, event.mtu.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_slow_start_exited(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::SlowStartExited,
    ) {
        self.count(61usize, 44usize, 1);
        self.measure(62usize, 17usize, event.congestion_window.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_delivery_rate_sampled(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DeliveryRateSampled,
    ) {
        self.count(63usize, 45usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_pacing_rate_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacingRateUpdated,
    ) {
        self.count(64usize, 46usize, 1);
        self.measure(65usize, 18usize, event.bytes_per_second.as_metric("b"));
        self.measure(66usize, 19usize, event.burst_size.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_bbr_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::BbrStateChanged,
    ) {
        self.count(67usize, 47usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_dc_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DcStateChanged,
    ) {
        self.count(68usize, 48usize, 1);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_version_information(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::VersionInformation,
    ) {
        self.count(69usize, 49usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_packet_sent(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointPacketSent,
    ) {
        self.count(70usize, 50usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_packet_received(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointPacketReceived,
    ) {
        self.count(71usize, 51usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_datagram_sent(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointDatagramSent,
    ) {
        self.count(72usize, 52usize, 1);
        self.measure(73usize, 20usize, event.len.as_metric("b"));
        self.measure(74usize, 21usize, event.len.as_metric("b"));
        self.measure(75usize, 22usize, event.gso_offset.as_metric(""));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_datagram_received(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointDatagramReceived,
    ) {
        self.count(76usize, 53usize, 1);
        self.measure(77usize, 23usize, event.len.as_metric("b"));
        self.measure(78usize, 24usize, event.len.as_metric("b"));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_datagram_dropped(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointDatagramDropped,
    ) {
        self.count(79usize, 54usize, 1);
        self.measure(80usize, 25usize, event.len.as_metric("b"));
        self.measure(81usize, 26usize, event.len.as_metric("b"));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_connection_attempt_failed(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointConnectionAttemptFailed,
    ) {
        self.count(82usize, 55usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_tx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTx) {
        self.count(83usize, 56usize, 1);
        self.count(84usize, 57usize, event.count.as_metric(""));
        self.measure(85usize, 27usize, event.count.as_metric(""));
        self.count(86usize, 58usize, event.syscalls.as_metric(""));
        self.measure(87usize, 28usize, event.syscalls.as_metric(""));
        self.count(88usize, 59usize, event.blocked_syscalls.as_metric(""));
        self.measure(89usize, 29usize, event.blocked_syscalls.as_metric(""));
        self.count(90usize, 60usize, event.total_errors.as_metric(""));
        self.measure(91usize, 30usize, event.total_errors.as_metric(""));
        self.count(92usize, 61usize, event.dropped_errors.as_metric(""));
        self.measure(93usize, 31usize, event.dropped_errors.as_metric(""));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_tx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTxError) {
        self.count(94usize, 62usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_rx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRx) {
        self.count(95usize, 63usize, 1);
        self.count(96usize, 64usize, event.count.as_metric(""));
        self.measure(97usize, 32usize, event.count.as_metric(""));
        self.count(98usize, 65usize, event.syscalls.as_metric(""));
        self.measure(99usize, 33usize, event.syscalls.as_metric(""));
        self.count(100usize, 66usize, event.blocked_syscalls.as_metric(""));
        self.measure(101usize, 34usize, event.blocked_syscalls.as_metric(""));
        self.count(102usize, 67usize, event.total_errors.as_metric(""));
        self.measure(103usize, 35usize, event.total_errors.as_metric(""));
        self.count(104usize, 68usize, event.dropped_errors.as_metric(""));
        self.measure(105usize, 36usize, event.dropped_errors.as_metric(""));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_rx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRxError) {
        self.count(106usize, 69usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_feature_configured(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformFeatureConfigured,
    ) {
        self.count(107usize, 70usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_event_loop_wakeup(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformEventLoopWakeup,
    ) {
        self.count(108usize, 71usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_event_loop_sleep(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformEventLoopSleep,
    ) {
        self.count(109usize, 72usize, 1);
        self.time(110usize, 0usize, event.processing_duration.as_metric("us"));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_event_loop_started(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformEventLoopStarted,
    ) {
        self.count(111usize, 73usize, 1);
        let _ = event;
        let _ = meta;
    }
}
