// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::event::{
    self, api,
    metrics::aggregate::{
        info::{self, Str},
        AsMetric as _, AsVariant, Info, NominalRecorder, Recorder, Registry,
    },
};
use alloc::{boxed::Box, vec::Vec};
static INFO: &[Info; 132usize] = &[
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
        name: Str::new("frame_sent.frame\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 11usize,
        name: Str::new("frame_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 12usize,
        name: Str::new("frame_received.frame\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 13usize,
        name: Str::new("packet_lost\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 14usize,
        name: Str::new("packet_lost.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 15usize,
        name: Str::new("packet_lost.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 16usize,
        name: Str::new("packet_lost.is_mtu_probe\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 17usize,
        name: Str::new("recovery_metrics\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 18usize,
        name: Str::new("recovery_metrics.min_rtt\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 19usize,
        name: Str::new("recovery_metrics.smoothed_rtt\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 20usize,
        name: Str::new("recovery_metrics.latest_rtt\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 21usize,
        name: Str::new("recovery_metrics.rtt_variance\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 22usize,
        name: Str::new("recovery_metrics.max_ack_delay\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 23usize,
        name: Str::new("recovery_metrics.pto_count\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 24usize,
        name: Str::new("recovery_metrics.congestion_window\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 25usize,
        name: Str::new("recovery_metrics.bytes_in_flight\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 26usize,
        name: Str::new("recovery_metrics.congestion_limited\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 27usize,
        name: Str::new("congestion\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 28usize,
        name: Str::new("congestion.source\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 29usize,
        name: Str::new("rx_ack_range_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 30usize,
        name: Str::new("ack_range_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 31usize,
        name: Str::new("ack_range_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 32usize,
        name: Str::new("packet_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 33usize,
        name: Str::new("packet_dropped.reason\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 34usize,
        name: Str::new("key_update\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 35usize,
        name: Str::new("key_space_discarded\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 36usize,
        name: Str::new("key_space_discarded.space\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 37usize,
        name: Str::new("connection_started\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 38usize,
        name: Str::new("connection_closed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 39usize,
        name: Str::new("connection_closed.error\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 40usize,
        name: Str::new("duplicate_packet\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 41usize,
        name: Str::new("duplicate_packet.error\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 42usize,
        name: Str::new("transport_parameters_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 43usize,
        name: Str::new("datagram_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 44usize,
        name: Str::new("datagram_sent.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 45usize,
        name: Str::new("datagram_sent.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 46usize,
        name: Str::new("datagram_sent.gso_offset\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 47usize,
        name: Str::new("datagram_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 48usize,
        name: Str::new("datagram_received.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 49usize,
        name: Str::new("datagram_received.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 50usize,
        name: Str::new("datagram_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 51usize,
        name: Str::new("datagram_dropped.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 52usize,
        name: Str::new("datagram_dropped.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 53usize,
        name: Str::new("datagram_dropped.reason\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 54usize,
        name: Str::new("connection_id_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 55usize,
        name: Str::new("ecn_state_changed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 56usize,
        name: Str::new("ecn_state_changed.state\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 57usize,
        name: Str::new("connection_migration_denied\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 58usize,
        name: Str::new("connection_migration_denied.reason\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 59usize,
        name: Str::new("handshake_status_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 60usize,
        name: Str::new("handshake_status_updated.status\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 61usize,
        name: Str::new("tls_exporter_ready\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 62usize,
        name: Str::new("path_challenge_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 63usize,
        name: Str::new("tls_client_hello\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 64usize,
        name: Str::new("tls_server_hello\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 65usize,
        name: Str::new("rx_stream_progress\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 66usize,
        name: Str::new("rx_stream_progress.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 67usize,
        name: Str::new("rx_stream_progress.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 68usize,
        name: Str::new("tx_stream_progress\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 69usize,
        name: Str::new("tx_stream_progress.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 70usize,
        name: Str::new("tx_stream_progress.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 71usize,
        name: Str::new("keep_alive_timer_expired\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 72usize,
        name: Str::new("mtu_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 73usize,
        name: Str::new("mtu_updated.mtu\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 74usize,
        name: Str::new("mtu_updated.cause\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 75usize,
        name: Str::new("mtu_updated.search_complete\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 76usize,
        name: Str::new("slow_start_exited\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 77usize,
        name: Str::new("slow_start_exited.cause\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 78usize,
        name: Str::new("slow_start_exited.congestion_window\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 79usize,
        name: Str::new("delivery_rate_sampled\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 80usize,
        name: Str::new("pacing_rate_updated\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 81usize,
        name: Str::new("pacing_rate_updated.bytes_per_second\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 82usize,
        name: Str::new("pacing_rate_updated.burst_size\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 83usize,
        name: Str::new("bbr_state_changed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 84usize,
        name: Str::new("bbr_state_changed.state\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 85usize,
        name: Str::new("dc_state_changed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 86usize,
        name: Str::new("dc_state_changed.state\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 87usize,
        name: Str::new("version_information\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 88usize,
        name: Str::new("endpoint_packet_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 89usize,
        name: Str::new("endpoint_packet_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 90usize,
        name: Str::new("endpoint_datagram_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 91usize,
        name: Str::new("endpoint_datagram_sent.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 92usize,
        name: Str::new("endpoint_datagram_sent.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 93usize,
        name: Str::new("endpoint_datagram_sent.gso_offset\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 94usize,
        name: Str::new("endpoint_datagram_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 95usize,
        name: Str::new("endpoint_datagram_received.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 96usize,
        name: Str::new("endpoint_datagram_received.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 97usize,
        name: Str::new("endpoint_datagram_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 98usize,
        name: Str::new("endpoint_datagram_dropped.bytes\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 99usize,
        name: Str::new("endpoint_datagram_dropped.bytes.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 100usize,
        name: Str::new("endpoint_datagram_dropped.reason\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 101usize,
        name: Str::new("endpoint_connection_attempt_failed\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 102usize,
        name: Str::new("endpoint_connection_attempt_failed.error\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 103usize,
        name: Str::new("platform_tx\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 104usize,
        name: Str::new("platform_tx.packets.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 105usize,
        name: Str::new("platform_tx.packets\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 106usize,
        name: Str::new("platform_tx.syscalls.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 107usize,
        name: Str::new("platform_tx.syscalls\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 108usize,
        name: Str::new("platform_tx.syscalls.blocked.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 109usize,
        name: Str::new("platform_tx.syscalls.blocked\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 110usize,
        name: Str::new("platform_tx.errors.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 111usize,
        name: Str::new("platform_tx.errors\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 112usize,
        name: Str::new("platform_tx.errors.dropped.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 113usize,
        name: Str::new("platform_tx.errors.dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 114usize,
        name: Str::new("platform_tx_error\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 115usize,
        name: Str::new("platform_rx\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 116usize,
        name: Str::new("platform_rx.packets.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 117usize,
        name: Str::new("platform_rx.packets\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 118usize,
        name: Str::new("platform_rx.syscalls.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 119usize,
        name: Str::new("platform_rx.syscalls\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 120usize,
        name: Str::new("platform_rx.syscalls.blocked.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 121usize,
        name: Str::new("platform_rx.syscalls.blocked\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 122usize,
        name: Str::new("platform_rx.errors.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 123usize,
        name: Str::new("platform_rx.errors\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 124usize,
        name: Str::new("platform_rx.errors.dropped.total\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 125usize,
        name: Str::new("platform_rx.errors.dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 126usize,
        name: Str::new("platform_rx_error\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 127usize,
        name: Str::new("platform_feature_configured\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 128usize,
        name: Str::new("platform_event_loop_wakeup\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 129usize,
        name: Str::new("platform_event_loop_sleep\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 130usize,
        name: Str::new("platform_event_loop_sleep.processing_duration\0"),
        units: Str::new("us\0"),
    }
    .build(),
    info::Builder {
        id: 131usize,
        name: Str::new("platform_event_loop_started\0"),
        units: Str::new("\0"),
    }
    .build(),
];
pub struct Subscriber<R: Registry> {
    #[allow(dead_code)]
    counters: Box<[R::Counter; 74usize]>,
    #[allow(dead_code)]
    nominal_counters: Box<[R::NominalCounter]>,
    #[allow(dead_code)]
    nominal_offsets: Box<[usize; 20usize]>,
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
        let mut nominal_offsets = Vec::with_capacity(20usize);
        let mut nominal_counters = Vec::with_capacity(20usize);
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
        counters.push(registry.register_counter(&INFO[11usize]));
        counters.push(registry.register_counter(&INFO[13usize]));
        counters.push(registry.register_counter(&INFO[14usize]));
        counters.push(registry.register_counter(&INFO[17usize]));
        counters.push(registry.register_counter(&INFO[27usize]));
        counters.push(registry.register_counter(&INFO[29usize]));
        counters.push(registry.register_counter(&INFO[30usize]));
        counters.push(registry.register_counter(&INFO[31usize]));
        counters.push(registry.register_counter(&INFO[32usize]));
        counters.push(registry.register_counter(&INFO[34usize]));
        counters.push(registry.register_counter(&INFO[35usize]));
        counters.push(registry.register_counter(&INFO[37usize]));
        counters.push(registry.register_counter(&INFO[38usize]));
        counters.push(registry.register_counter(&INFO[40usize]));
        counters.push(registry.register_counter(&INFO[42usize]));
        counters.push(registry.register_counter(&INFO[43usize]));
        counters.push(registry.register_counter(&INFO[44usize]));
        counters.push(registry.register_counter(&INFO[47usize]));
        counters.push(registry.register_counter(&INFO[48usize]));
        counters.push(registry.register_counter(&INFO[50usize]));
        counters.push(registry.register_counter(&INFO[51usize]));
        counters.push(registry.register_counter(&INFO[54usize]));
        counters.push(registry.register_counter(&INFO[55usize]));
        counters.push(registry.register_counter(&INFO[57usize]));
        counters.push(registry.register_counter(&INFO[59usize]));
        counters.push(registry.register_counter(&INFO[61usize]));
        counters.push(registry.register_counter(&INFO[62usize]));
        counters.push(registry.register_counter(&INFO[63usize]));
        counters.push(registry.register_counter(&INFO[64usize]));
        counters.push(registry.register_counter(&INFO[65usize]));
        counters.push(registry.register_counter(&INFO[66usize]));
        counters.push(registry.register_counter(&INFO[68usize]));
        counters.push(registry.register_counter(&INFO[69usize]));
        counters.push(registry.register_counter(&INFO[71usize]));
        counters.push(registry.register_counter(&INFO[72usize]));
        counters.push(registry.register_counter(&INFO[76usize]));
        counters.push(registry.register_counter(&INFO[79usize]));
        counters.push(registry.register_counter(&INFO[80usize]));
        counters.push(registry.register_counter(&INFO[83usize]));
        counters.push(registry.register_counter(&INFO[85usize]));
        counters.push(registry.register_counter(&INFO[87usize]));
        counters.push(registry.register_counter(&INFO[88usize]));
        counters.push(registry.register_counter(&INFO[89usize]));
        counters.push(registry.register_counter(&INFO[90usize]));
        counters.push(registry.register_counter(&INFO[94usize]));
        counters.push(registry.register_counter(&INFO[97usize]));
        counters.push(registry.register_counter(&INFO[101usize]));
        counters.push(registry.register_counter(&INFO[103usize]));
        counters.push(registry.register_counter(&INFO[104usize]));
        counters.push(registry.register_counter(&INFO[106usize]));
        counters.push(registry.register_counter(&INFO[108usize]));
        counters.push(registry.register_counter(&INFO[110usize]));
        counters.push(registry.register_counter(&INFO[112usize]));
        counters.push(registry.register_counter(&INFO[114usize]));
        counters.push(registry.register_counter(&INFO[115usize]));
        counters.push(registry.register_counter(&INFO[116usize]));
        counters.push(registry.register_counter(&INFO[118usize]));
        counters.push(registry.register_counter(&INFO[120usize]));
        counters.push(registry.register_counter(&INFO[122usize]));
        counters.push(registry.register_counter(&INFO[124usize]));
        counters.push(registry.register_counter(&INFO[126usize]));
        counters.push(registry.register_counter(&INFO[127usize]));
        counters.push(registry.register_counter(&INFO[128usize]));
        counters.push(registry.register_counter(&INFO[129usize]));
        counters.push(registry.register_counter(&INFO[131usize]));
        {
            #[allow(unused_imports)]
            use api::*;
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <Frame as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[10usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <Frame as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[12usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <bool as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[16usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <bool as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[26usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <CongestionSource as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[28usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[33usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <KeySpace as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[36usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <crate::connection::Error as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[39usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <DuplicatePacketError as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[41usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <DatagramDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[53usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <EcnState as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[56usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <MigrationDenyReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[58usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <HandshakeStatus as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[60usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <MtuUpdatedCause as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[74usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <bool as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[75usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SlowStartExitCause as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[77usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <BbrState as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[84usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <DcState as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[86usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <DatagramDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[100usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <crate::connection::Error as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[102usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
        }
        measures.push(registry.register_measure(&INFO[5usize]));
        measures.push(registry.register_measure(&INFO[15usize]));
        measures.push(registry.register_measure(&INFO[18usize]));
        measures.push(registry.register_measure(&INFO[19usize]));
        measures.push(registry.register_measure(&INFO[20usize]));
        measures.push(registry.register_measure(&INFO[21usize]));
        measures.push(registry.register_measure(&INFO[22usize]));
        measures.push(registry.register_measure(&INFO[23usize]));
        measures.push(registry.register_measure(&INFO[24usize]));
        measures.push(registry.register_measure(&INFO[25usize]));
        measures.push(registry.register_measure(&INFO[45usize]));
        measures.push(registry.register_measure(&INFO[46usize]));
        measures.push(registry.register_measure(&INFO[49usize]));
        measures.push(registry.register_measure(&INFO[52usize]));
        measures.push(registry.register_measure(&INFO[67usize]));
        measures.push(registry.register_measure(&INFO[70usize]));
        measures.push(registry.register_measure(&INFO[73usize]));
        measures.push(registry.register_measure(&INFO[78usize]));
        measures.push(registry.register_measure(&INFO[81usize]));
        measures.push(registry.register_measure(&INFO[82usize]));
        measures.push(registry.register_measure(&INFO[91usize]));
        measures.push(registry.register_measure(&INFO[92usize]));
        measures.push(registry.register_measure(&INFO[93usize]));
        measures.push(registry.register_measure(&INFO[95usize]));
        measures.push(registry.register_measure(&INFO[96usize]));
        measures.push(registry.register_measure(&INFO[98usize]));
        measures.push(registry.register_measure(&INFO[99usize]));
        measures.push(registry.register_measure(&INFO[105usize]));
        measures.push(registry.register_measure(&INFO[107usize]));
        measures.push(registry.register_measure(&INFO[109usize]));
        measures.push(registry.register_measure(&INFO[111usize]));
        measures.push(registry.register_measure(&INFO[113usize]));
        measures.push(registry.register_measure(&INFO[117usize]));
        measures.push(registry.register_measure(&INFO[119usize]));
        measures.push(registry.register_measure(&INFO[121usize]));
        measures.push(registry.register_measure(&INFO[123usize]));
        measures.push(registry.register_measure(&INFO[125usize]));
        timers.push(registry.register_timer(&INFO[130usize]));
        Self {
            counters: counters
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            nominal_counters: nominal_counters.into(),
            nominal_offsets: nominal_offsets
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
                9usize => (&INFO[11usize], entry),
                10usize => (&INFO[13usize], entry),
                11usize => (&INFO[14usize], entry),
                12usize => (&INFO[17usize], entry),
                13usize => (&INFO[27usize], entry),
                14usize => (&INFO[29usize], entry),
                15usize => (&INFO[30usize], entry),
                16usize => (&INFO[31usize], entry),
                17usize => (&INFO[32usize], entry),
                18usize => (&INFO[34usize], entry),
                19usize => (&INFO[35usize], entry),
                20usize => (&INFO[37usize], entry),
                21usize => (&INFO[38usize], entry),
                22usize => (&INFO[40usize], entry),
                23usize => (&INFO[42usize], entry),
                24usize => (&INFO[43usize], entry),
                25usize => (&INFO[44usize], entry),
                26usize => (&INFO[47usize], entry),
                27usize => (&INFO[48usize], entry),
                28usize => (&INFO[50usize], entry),
                29usize => (&INFO[51usize], entry),
                30usize => (&INFO[54usize], entry),
                31usize => (&INFO[55usize], entry),
                32usize => (&INFO[57usize], entry),
                33usize => (&INFO[59usize], entry),
                34usize => (&INFO[61usize], entry),
                35usize => (&INFO[62usize], entry),
                36usize => (&INFO[63usize], entry),
                37usize => (&INFO[64usize], entry),
                38usize => (&INFO[65usize], entry),
                39usize => (&INFO[66usize], entry),
                40usize => (&INFO[68usize], entry),
                41usize => (&INFO[69usize], entry),
                42usize => (&INFO[71usize], entry),
                43usize => (&INFO[72usize], entry),
                44usize => (&INFO[76usize], entry),
                45usize => (&INFO[79usize], entry),
                46usize => (&INFO[80usize], entry),
                47usize => (&INFO[83usize], entry),
                48usize => (&INFO[85usize], entry),
                49usize => (&INFO[87usize], entry),
                50usize => (&INFO[88usize], entry),
                51usize => (&INFO[89usize], entry),
                52usize => (&INFO[90usize], entry),
                53usize => (&INFO[94usize], entry),
                54usize => (&INFO[97usize], entry),
                55usize => (&INFO[101usize], entry),
                56usize => (&INFO[103usize], entry),
                57usize => (&INFO[104usize], entry),
                58usize => (&INFO[106usize], entry),
                59usize => (&INFO[108usize], entry),
                60usize => (&INFO[110usize], entry),
                61usize => (&INFO[112usize], entry),
                62usize => (&INFO[114usize], entry),
                63usize => (&INFO[115usize], entry),
                64usize => (&INFO[116usize], entry),
                65usize => (&INFO[118usize], entry),
                66usize => (&INFO[120usize], entry),
                67usize => (&INFO[122usize], entry),
                68usize => (&INFO[124usize], entry),
                69usize => (&INFO[126usize], entry),
                70usize => (&INFO[127usize], entry),
                71usize => (&INFO[128usize], entry),
                72usize => (&INFO[129usize], entry),
                73usize => (&INFO[131usize], entry),
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
    #[doc = r" Returns all of the registered nominal counters"]
    #[inline]
    pub fn nominal_counters(
        &self,
    ) -> impl Iterator<Item = (&'static Info, &[R::NominalCounter], &[info::Variant])> + '_ {
        use api::*;
        self.nominal_offsets
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => {
                    let offset = *entry;
                    let variants = <Frame as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[10usize], entries, variants)
                }
                1usize => {
                    let offset = *entry;
                    let variants = <Frame as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[12usize], entries, variants)
                }
                2usize => {
                    let offset = *entry;
                    let variants = <bool as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[16usize], entries, variants)
                }
                3usize => {
                    let offset = *entry;
                    let variants = <bool as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[26usize], entries, variants)
                }
                4usize => {
                    let offset = *entry;
                    let variants = <CongestionSource as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[28usize], entries, variants)
                }
                5usize => {
                    let offset = *entry;
                    let variants = <PacketDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[33usize], entries, variants)
                }
                6usize => {
                    let offset = *entry;
                    let variants = <KeySpace as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[36usize], entries, variants)
                }
                7usize => {
                    let offset = *entry;
                    let variants = <crate::connection::Error as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[39usize], entries, variants)
                }
                8usize => {
                    let offset = *entry;
                    let variants = <DuplicatePacketError as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[41usize], entries, variants)
                }
                9usize => {
                    let offset = *entry;
                    let variants = <DatagramDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[53usize], entries, variants)
                }
                10usize => {
                    let offset = *entry;
                    let variants = <EcnState as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[56usize], entries, variants)
                }
                11usize => {
                    let offset = *entry;
                    let variants = <MigrationDenyReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[58usize], entries, variants)
                }
                12usize => {
                    let offset = *entry;
                    let variants = <HandshakeStatus as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[60usize], entries, variants)
                }
                13usize => {
                    let offset = *entry;
                    let variants = <MtuUpdatedCause as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[74usize], entries, variants)
                }
                14usize => {
                    let offset = *entry;
                    let variants = <bool as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[75usize], entries, variants)
                }
                15usize => {
                    let offset = *entry;
                    let variants = <SlowStartExitCause as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[77usize], entries, variants)
                }
                16usize => {
                    let offset = *entry;
                    let variants = <BbrState as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[84usize], entries, variants)
                }
                17usize => {
                    let offset = *entry;
                    let variants = <DcState as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[86usize], entries, variants)
                }
                18usize => {
                    let offset = *entry;
                    let variants = <DatagramDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[100usize], entries, variants)
                }
                19usize => {
                    let offset = *entry;
                    let variants = <crate::connection::Error as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[102usize], entries, variants)
                }
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn nominal_count<T: AsVariant>(&self, info: usize, id: usize, value: &T) {
        let info = &INFO[info];
        let idx = self.nominal_offsets[id] + value.variant_idx();
        let counter = &self.nominal_counters[idx];
        counter.record(info, value.as_variant(), 1);
    }
    #[doc = r" Returns all of the registered measures"]
    #[inline]
    pub fn measures(&self) -> impl Iterator<Item = (&'static Info, &R::Measure)> + '_ {
        self.measures
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[5usize], entry),
                1usize => (&INFO[15usize], entry),
                2usize => (&INFO[18usize], entry),
                3usize => (&INFO[19usize], entry),
                4usize => (&INFO[20usize], entry),
                5usize => (&INFO[21usize], entry),
                6usize => (&INFO[22usize], entry),
                7usize => (&INFO[23usize], entry),
                8usize => (&INFO[24usize], entry),
                9usize => (&INFO[25usize], entry),
                10usize => (&INFO[45usize], entry),
                11usize => (&INFO[46usize], entry),
                12usize => (&INFO[49usize], entry),
                13usize => (&INFO[52usize], entry),
                14usize => (&INFO[67usize], entry),
                15usize => (&INFO[70usize], entry),
                16usize => (&INFO[73usize], entry),
                17usize => (&INFO[78usize], entry),
                18usize => (&INFO[81usize], entry),
                19usize => (&INFO[82usize], entry),
                20usize => (&INFO[91usize], entry),
                21usize => (&INFO[92usize], entry),
                22usize => (&INFO[93usize], entry),
                23usize => (&INFO[95usize], entry),
                24usize => (&INFO[96usize], entry),
                25usize => (&INFO[98usize], entry),
                26usize => (&INFO[99usize], entry),
                27usize => (&INFO[105usize], entry),
                28usize => (&INFO[107usize], entry),
                29usize => (&INFO[109usize], entry),
                30usize => (&INFO[111usize], entry),
                31usize => (&INFO[113usize], entry),
                32usize => (&INFO[117usize], entry),
                33usize => (&INFO[119usize], entry),
                34usize => (&INFO[121usize], entry),
                35usize => (&INFO[123usize], entry),
                36usize => (&INFO[125usize], entry),
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
                0usize => (&INFO[130usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn time(&self, info: usize, id: usize, value: core::time::Duration) {
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
        self.nominal_count(10usize, 0usize, &event.frame);
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
        self.count(11usize, 9usize, 1);
        self.nominal_count(12usize, 1usize, &event.frame);
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
        self.count(13usize, 10usize, 1);
        self.count(14usize, 11usize, event.bytes_lost.as_metric("b"));
        self.measure(15usize, 1usize, event.bytes_lost.as_metric("b"));
        self.nominal_count(16usize, 2usize, &event.is_mtu_probe);
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
        self.count(17usize, 12usize, 1);
        self.measure(18usize, 2usize, event.min_rtt.as_metric("us"));
        self.measure(19usize, 3usize, event.smoothed_rtt.as_metric("us"));
        self.measure(20usize, 4usize, event.latest_rtt.as_metric("us"));
        self.measure(21usize, 5usize, event.rtt_variance.as_metric("us"));
        self.measure(22usize, 6usize, event.max_ack_delay.as_metric("us"));
        self.measure(23usize, 7usize, event.pto_count.as_metric(""));
        self.measure(24usize, 8usize, event.congestion_window.as_metric("b"));
        self.measure(25usize, 9usize, event.bytes_in_flight.as_metric("b"));
        self.nominal_count(26usize, 3usize, &event.congestion_limited);
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
        self.count(27usize, 13usize, 1);
        self.nominal_count(28usize, 4usize, &event.source);
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
        self.count(29usize, 14usize, 1);
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
        self.count(30usize, 15usize, 1);
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
        self.count(31usize, 16usize, 1);
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
        self.count(32usize, 17usize, 1);
        self.nominal_count(33usize, 5usize, &event.reason);
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
        self.count(34usize, 18usize, 1);
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
        self.count(35usize, 19usize, 1);
        self.nominal_count(36usize, 6usize, &event.space);
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
        self.count(37usize, 20usize, 1);
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
        self.count(38usize, 21usize, 1);
        self.nominal_count(39usize, 7usize, &event.error);
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
        self.count(40usize, 22usize, 1);
        self.nominal_count(41usize, 8usize, &event.error);
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
        self.count(42usize, 23usize, 1);
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
        self.count(43usize, 24usize, 1);
        self.count(44usize, 25usize, event.len.as_metric("b"));
        self.measure(45usize, 10usize, event.len.as_metric("b"));
        self.measure(46usize, 11usize, event.gso_offset.as_metric(""));
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
        self.count(47usize, 26usize, 1);
        self.count(48usize, 27usize, event.len.as_metric("b"));
        self.measure(49usize, 12usize, event.len.as_metric("b"));
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
        self.count(50usize, 28usize, 1);
        self.count(51usize, 29usize, event.len.as_metric("b"));
        self.measure(52usize, 13usize, event.len.as_metric("b"));
        self.nominal_count(53usize, 9usize, &event.reason);
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
        self.count(54usize, 30usize, 1);
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
        self.count(55usize, 31usize, 1);
        self.nominal_count(56usize, 10usize, &event.state);
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
        self.count(57usize, 32usize, 1);
        self.nominal_count(58usize, 11usize, &event.reason);
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
        self.count(59usize, 33usize, 1);
        self.nominal_count(60usize, 12usize, &event.status);
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
        self.count(61usize, 34usize, 1);
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
        self.count(62usize, 35usize, 1);
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
        self.count(63usize, 36usize, 1);
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
        self.count(64usize, 37usize, 1);
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
        self.count(65usize, 38usize, 1);
        self.count(66usize, 39usize, event.bytes.as_metric("b"));
        self.measure(67usize, 14usize, event.bytes.as_metric("b"));
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
        self.count(68usize, 40usize, 1);
        self.count(69usize, 41usize, event.bytes.as_metric("b"));
        self.measure(70usize, 15usize, event.bytes.as_metric("b"));
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
        self.count(71usize, 42usize, 1);
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
        self.count(72usize, 43usize, 1);
        self.measure(73usize, 16usize, event.mtu.as_metric("b"));
        self.nominal_count(74usize, 13usize, &event.cause);
        self.nominal_count(75usize, 14usize, &event.search_complete);
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
        self.count(76usize, 44usize, 1);
        self.nominal_count(77usize, 15usize, &event.cause);
        self.measure(78usize, 17usize, event.congestion_window.as_metric("b"));
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
        self.count(79usize, 45usize, 1);
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
        self.count(80usize, 46usize, 1);
        self.measure(81usize, 18usize, event.bytes_per_second.as_metric("b"));
        self.measure(82usize, 19usize, event.burst_size.as_metric("b"));
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
        self.count(83usize, 47usize, 1);
        self.nominal_count(84usize, 16usize, &event.state);
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
        self.count(85usize, 48usize, 1);
        self.nominal_count(86usize, 17usize, &event.state);
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
        self.count(87usize, 49usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_packet_sent(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointPacketSent,
    ) {
        self.count(88usize, 50usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_packet_received(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointPacketReceived,
    ) {
        self.count(89usize, 51usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_datagram_sent(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointDatagramSent,
    ) {
        self.count(90usize, 52usize, 1);
        self.measure(91usize, 20usize, event.len.as_metric("b"));
        self.measure(92usize, 21usize, event.len.as_metric("b"));
        self.measure(93usize, 22usize, event.gso_offset.as_metric(""));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_datagram_received(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointDatagramReceived,
    ) {
        self.count(94usize, 53usize, 1);
        self.measure(95usize, 23usize, event.len.as_metric("b"));
        self.measure(96usize, 24usize, event.len.as_metric("b"));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_datagram_dropped(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointDatagramDropped,
    ) {
        self.count(97usize, 54usize, 1);
        self.measure(98usize, 25usize, event.len.as_metric("b"));
        self.measure(99usize, 26usize, event.len.as_metric("b"));
        self.nominal_count(100usize, 18usize, &event.reason);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_connection_attempt_failed(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointConnectionAttemptFailed,
    ) {
        self.count(101usize, 55usize, 1);
        self.nominal_count(102usize, 19usize, &event.error);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_tx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTx) {
        self.count(103usize, 56usize, 1);
        self.count(104usize, 57usize, event.count.as_metric(""));
        self.measure(105usize, 27usize, event.count.as_metric(""));
        self.count(106usize, 58usize, event.syscalls.as_metric(""));
        self.measure(107usize, 28usize, event.syscalls.as_metric(""));
        self.count(108usize, 59usize, event.blocked_syscalls.as_metric(""));
        self.measure(109usize, 29usize, event.blocked_syscalls.as_metric(""));
        self.count(110usize, 60usize, event.total_errors.as_metric(""));
        self.measure(111usize, 30usize, event.total_errors.as_metric(""));
        self.count(112usize, 61usize, event.dropped_errors.as_metric(""));
        self.measure(113usize, 31usize, event.dropped_errors.as_metric(""));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_tx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTxError) {
        self.count(114usize, 62usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_rx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRx) {
        self.count(115usize, 63usize, 1);
        self.count(116usize, 64usize, event.count.as_metric(""));
        self.measure(117usize, 32usize, event.count.as_metric(""));
        self.count(118usize, 65usize, event.syscalls.as_metric(""));
        self.measure(119usize, 33usize, event.syscalls.as_metric(""));
        self.count(120usize, 66usize, event.blocked_syscalls.as_metric(""));
        self.measure(121usize, 34usize, event.blocked_syscalls.as_metric(""));
        self.count(122usize, 67usize, event.total_errors.as_metric(""));
        self.measure(123usize, 35usize, event.total_errors.as_metric(""));
        self.count(124usize, 68usize, event.dropped_errors.as_metric(""));
        self.measure(125usize, 36usize, event.dropped_errors.as_metric(""));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_rx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRxError) {
        self.count(126usize, 69usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_feature_configured(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformFeatureConfigured,
    ) {
        self.count(127usize, 70usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_event_loop_wakeup(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformEventLoopWakeup,
    ) {
        self.count(128usize, 71usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_event_loop_sleep(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformEventLoopSleep,
    ) {
        self.count(129usize, 72usize, 1);
        self.time(130usize, 0usize, event.processing_duration.as_metric("us"));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_event_loop_started(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformEventLoopStarted,
    ) {
        self.count(131usize, 73usize, 1);
        let _ = event;
        let _ = meta;
    }
}
