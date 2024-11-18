// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::event::{
    self, api,
    metrics::aggregate::{
        info::{self, Str},
        AsVariant, BoolRecorder, Info, Metric, NominalRecorder, Recorder, Registry, Units,
    },
};
static INFO: &[Info; 101usize] = &[
    info::Builder {
        id: 0usize,
        name: Str::new("acceptor_tcp_started\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 1usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 2usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.pending_streams\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 3usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 4usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.max_sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 5usize,
        name: Str::new("acceptor_tcp_fresh_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 6usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 7usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed.enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 8usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed.dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 9usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed.errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 10usize,
        name: Str::new("acceptor_tcp_stream_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 11usize,
        name: Str::new("acceptor_tcp_stream_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 12usize,
        name: Str::new("acceptor_tcp_stream_replaced\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 13usize,
        name: Str::new("acceptor_tcp_stream_replaced.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 14usize,
        name: Str::new("acceptor_tcp_stream_replaced.buffer_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 15usize,
        name: Str::new("acceptor_tcp_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 16usize,
        name: Str::new("acceptor_tcp_packet_received.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 17usize,
        name: Str::new("acceptor_tcp_packet_received.is_fin\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 18usize,
        name: Str::new("acceptor_tcp_packet_received.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 19usize,
        name: Str::new("acceptor_tcp_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 20usize,
        name: Str::new("acceptor_tcp_packet_dropped.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 21usize,
        name: Str::new("acceptor_tcp_stream_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 22usize,
        name: Str::new("acceptor_tcp_stream_enqueued.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 23usize,
        name: Str::new("acceptor_tcp_stream_enqueued.blocked_count\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 24usize,
        name: Str::new("acceptor_tcp_io_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 25usize,
        name: Str::new("acceptor_udp_started\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 26usize,
        name: Str::new("acceptor_udp_datagram_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 27usize,
        name: Str::new("acceptor_udp_datagram_received.len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 28usize,
        name: Str::new("acceptor_udp_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 29usize,
        name: Str::new("acceptor_udp_packet_received.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 30usize,
        name: Str::new("acceptor_udp_packet_received.is_zero_offset\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 31usize,
        name: Str::new("acceptor_udp_packet_received.is_retransmisson\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 32usize,
        name: Str::new("acceptor_udp_packet_received.is_fin\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 33usize,
        name: Str::new("acceptor_udp_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 34usize,
        name: Str::new("acceptor_udp_stream_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 35usize,
        name: Str::new("acceptor_udp_io_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 36usize,
        name: Str::new("acceptor_stream_pruned\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 37usize,
        name: Str::new("acceptor_stream_pruned.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 38usize,
        name: Str::new("acceptor_stream_pruned.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 39usize,
        name: Str::new("acceptor_stream_dequeued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 40usize,
        name: Str::new("acceptor_stream_dequeued.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 41usize,
        name: Str::new("application_write\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 42usize,
        name: Str::new("application_write.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 43usize,
        name: Str::new("application_write.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 44usize,
        name: Str::new("application_write.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 45usize,
        name: Str::new("application_read\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 46usize,
        name: Str::new("application_read.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 47usize,
        name: Str::new("application_read.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 48usize,
        name: Str::new("application_read.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 49usize,
        name: Str::new("endpoint_initialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 50usize,
        name: Str::new("endpoint_initialized.acceptor.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 51usize,
        name: Str::new("endpoint_initialized.handshake.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 52usize,
        name: Str::new("endpoint_initialized.tcp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 53usize,
        name: Str::new("endpoint_initialized.udp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 54usize,
        name: Str::new("path_secret_map_initialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 55usize,
        name: Str::new("path_secret_map_initialized.capacity\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 56usize,
        name: Str::new("path_secret_map_uninitialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 57usize,
        name: Str::new("path_secret_map_uninitialized.capacity\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 58usize,
        name: Str::new("path_secret_map_uninitialized.entries\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 59usize,
        name: Str::new("path_secret_map_uninitialized.lifetime\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 60usize,
        name: Str::new("path_secret_map_background_handshake_requested\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 61usize,
        name: Str::new("path_secret_map_background_handshake_requested.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 62usize,
        name: Str::new("path_secret_map_entry_inserted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 63usize,
        name: Str::new("path_secret_map_entry_inserted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 64usize,
        name: Str::new("path_secret_map_entry_ready\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 65usize,
        name: Str::new("path_secret_map_entry_ready.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 66usize,
        name: Str::new("path_secret_map_entry_replaced\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 67usize,
        name: Str::new("path_secret_map_entry_replaced.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 68usize,
        name: Str::new("unknown_path_secret_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 69usize,
        name: Str::new("unknown_path_secret_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 70usize,
        name: Str::new("unknown_path_secret_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 71usize,
        name: Str::new("unknown_path_secret_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 72usize,
        name: Str::new("unknown_path_secret_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 73usize,
        name: Str::new("unknown_path_secret_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 74usize,
        name: Str::new("unknown_path_secret_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 75usize,
        name: Str::new("unknown_path_secret_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 76usize,
        name: Str::new("unknown_path_secret_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 77usize,
        name: Str::new("unknown_path_secret_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 78usize,
        name: Str::new("replay_definitely_detected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 79usize,
        name: Str::new("replay_potentially_detected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 80usize,
        name: Str::new("replay_potentially_detected.gap\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 81usize,
        name: Str::new("replay_detected_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 82usize,
        name: Str::new("replay_detected_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 83usize,
        name: Str::new("replay_detected_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 84usize,
        name: Str::new("replay_detected_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 85usize,
        name: Str::new("replay_detected_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 86usize,
        name: Str::new("replay_detected_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 87usize,
        name: Str::new("replay_detected_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 88usize,
        name: Str::new("replay_detected_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 89usize,
        name: Str::new("replay_detected_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 90usize,
        name: Str::new("replay_detected_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 91usize,
        name: Str::new("stale_key_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 92usize,
        name: Str::new("stale_key_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 93usize,
        name: Str::new("stale_key_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 94usize,
        name: Str::new("stale_key_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 95usize,
        name: Str::new("stale_key_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 96usize,
        name: Str::new("stale_key_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 97usize,
        name: Str::new("stale_key_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 98usize,
        name: Str::new("stale_key_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 99usize,
        name: Str::new("stale_key_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 100usize,
        name: Str::new("stale_key_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
];
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct ConnectionContext {
    start_time: crate::event::Timestamp,
}
pub struct Subscriber<R: Registry> {
    #[allow(dead_code)]
    counters: Box<[R::Counter; 46usize]>,
    #[allow(dead_code)]
    bool_counters: Box<[R::BoolCounter; 6usize]>,
    #[allow(dead_code)]
    nominal_counters: Box<[R::NominalCounter]>,
    #[allow(dead_code)]
    nominal_counter_offsets: Box<[usize; 23usize]>,
    #[allow(dead_code)]
    measures: Box<[R::Measure; 19usize]>,
    #[allow(dead_code)]
    gauges: Box<[R::Gauge; 0usize]>,
    #[allow(dead_code)]
    timers: Box<[R::Timer; 7usize]>,
    #[allow(dead_code)]
    nominal_timers: Box<[R::NominalTimer]>,
    #[allow(dead_code)]
    nominal_timer_offsets: Box<[usize; 0usize]>,
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
        let mut counters = Vec::with_capacity(46usize);
        let mut bool_counters = Vec::with_capacity(6usize);
        let mut nominal_counters = Vec::with_capacity(23usize);
        let mut nominal_counter_offsets = Vec::with_capacity(23usize);
        let mut measures = Vec::with_capacity(19usize);
        let mut gauges = Vec::with_capacity(0usize);
        let mut timers = Vec::with_capacity(7usize);
        let mut nominal_timers = Vec::with_capacity(0usize);
        let mut nominal_timer_offsets = Vec::with_capacity(0usize);
        counters.push(registry.register_counter(&INFO[0usize]));
        counters.push(registry.register_counter(&INFO[1usize]));
        counters.push(registry.register_counter(&INFO[5usize]));
        counters.push(registry.register_counter(&INFO[6usize]));
        counters.push(registry.register_counter(&INFO[10usize]));
        counters.push(registry.register_counter(&INFO[12usize]));
        counters.push(registry.register_counter(&INFO[15usize]));
        counters.push(registry.register_counter(&INFO[19usize]));
        counters.push(registry.register_counter(&INFO[21usize]));
        counters.push(registry.register_counter(&INFO[24usize]));
        counters.push(registry.register_counter(&INFO[25usize]));
        counters.push(registry.register_counter(&INFO[26usize]));
        counters.push(registry.register_counter(&INFO[28usize]));
        counters.push(registry.register_counter(&INFO[33usize]));
        counters.push(registry.register_counter(&INFO[34usize]));
        counters.push(registry.register_counter(&INFO[35usize]));
        counters.push(registry.register_counter(&INFO[36usize]));
        counters.push(registry.register_counter(&INFO[39usize]));
        counters.push(registry.register_counter(&INFO[41usize]));
        counters.push(registry.register_counter(&INFO[43usize]));
        counters.push(registry.register_counter(&INFO[45usize]));
        counters.push(registry.register_counter(&INFO[47usize]));
        counters.push(registry.register_counter(&INFO[49usize]));
        counters.push(registry.register_counter(&INFO[54usize]));
        counters.push(registry.register_counter(&INFO[56usize]));
        counters.push(registry.register_counter(&INFO[60usize]));
        counters.push(registry.register_counter(&INFO[62usize]));
        counters.push(registry.register_counter(&INFO[64usize]));
        counters.push(registry.register_counter(&INFO[66usize]));
        counters.push(registry.register_counter(&INFO[68usize]));
        counters.push(registry.register_counter(&INFO[70usize]));
        counters.push(registry.register_counter(&INFO[72usize]));
        counters.push(registry.register_counter(&INFO[74usize]));
        counters.push(registry.register_counter(&INFO[76usize]));
        counters.push(registry.register_counter(&INFO[78usize]));
        counters.push(registry.register_counter(&INFO[79usize]));
        counters.push(registry.register_counter(&INFO[81usize]));
        counters.push(registry.register_counter(&INFO[83usize]));
        counters.push(registry.register_counter(&INFO[85usize]));
        counters.push(registry.register_counter(&INFO[87usize]));
        counters.push(registry.register_counter(&INFO[89usize]));
        counters.push(registry.register_counter(&INFO[91usize]));
        counters.push(registry.register_counter(&INFO[93usize]));
        counters.push(registry.register_counter(&INFO[95usize]));
        counters.push(registry.register_counter(&INFO[97usize]));
        counters.push(registry.register_counter(&INFO[99usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[17usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[30usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[31usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[32usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[52usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[53usize]));
        {
            #[allow(unused_imports)]
            use api::*;
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <AcceptorTcpStreamDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[11usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <AcceptorStreamPruneReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[38usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[50usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[51usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[61usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[63usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[65usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[67usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[69usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[71usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[73usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[75usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[77usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[82usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[84usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[86usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[88usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[90usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[92usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[94usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[96usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[98usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[100usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
        }
        measures.push(registry.register_measure(&INFO[2usize]));
        measures.push(registry.register_measure(&INFO[4usize]));
        measures.push(registry.register_measure(&INFO[7usize]));
        measures.push(registry.register_measure(&INFO[8usize]));
        measures.push(registry.register_measure(&INFO[9usize]));
        measures.push(registry.register_measure(&INFO[14usize]));
        measures.push(registry.register_measure(&INFO[16usize]));
        measures.push(registry.register_measure(&INFO[23usize]));
        measures.push(registry.register_measure(&INFO[27usize]));
        measures.push(registry.register_measure(&INFO[29usize]));
        measures.push(registry.register_measure(&INFO[42usize]));
        measures.push(registry.register_measure(&INFO[44usize]));
        measures.push(registry.register_measure(&INFO[46usize]));
        measures.push(registry.register_measure(&INFO[48usize]));
        measures.push(registry.register_measure(&INFO[55usize]));
        measures.push(registry.register_measure(&INFO[57usize]));
        measures.push(registry.register_measure(&INFO[58usize]));
        measures.push(registry.register_measure(&INFO[59usize]));
        measures.push(registry.register_measure(&INFO[80usize]));
        timers.push(registry.register_timer(&INFO[3usize]));
        timers.push(registry.register_timer(&INFO[13usize]));
        timers.push(registry.register_timer(&INFO[18usize]));
        timers.push(registry.register_timer(&INFO[20usize]));
        timers.push(registry.register_timer(&INFO[22usize]));
        timers.push(registry.register_timer(&INFO[37usize]));
        timers.push(registry.register_timer(&INFO[40usize]));
        {
            #[allow(unused_imports)]
            use api::*;
        }
        Self {
            counters: counters
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            bool_counters: bool_counters
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            nominal_counters: nominal_counters.into(),
            nominal_counter_offsets: nominal_counter_offsets
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            measures: measures
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            gauges: gauges.try_into().unwrap_or_else(|_| panic!("invalid len")),
            timers: timers.try_into().unwrap_or_else(|_| panic!("invalid len")),
            nominal_timers: nominal_timers.into(),
            nominal_timer_offsets: nominal_timer_offsets
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
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
                2usize => (&INFO[5usize], entry),
                3usize => (&INFO[6usize], entry),
                4usize => (&INFO[10usize], entry),
                5usize => (&INFO[12usize], entry),
                6usize => (&INFO[15usize], entry),
                7usize => (&INFO[19usize], entry),
                8usize => (&INFO[21usize], entry),
                9usize => (&INFO[24usize], entry),
                10usize => (&INFO[25usize], entry),
                11usize => (&INFO[26usize], entry),
                12usize => (&INFO[28usize], entry),
                13usize => (&INFO[33usize], entry),
                14usize => (&INFO[34usize], entry),
                15usize => (&INFO[35usize], entry),
                16usize => (&INFO[36usize], entry),
                17usize => (&INFO[39usize], entry),
                18usize => (&INFO[41usize], entry),
                19usize => (&INFO[43usize], entry),
                20usize => (&INFO[45usize], entry),
                21usize => (&INFO[47usize], entry),
                22usize => (&INFO[49usize], entry),
                23usize => (&INFO[54usize], entry),
                24usize => (&INFO[56usize], entry),
                25usize => (&INFO[60usize], entry),
                26usize => (&INFO[62usize], entry),
                27usize => (&INFO[64usize], entry),
                28usize => (&INFO[66usize], entry),
                29usize => (&INFO[68usize], entry),
                30usize => (&INFO[70usize], entry),
                31usize => (&INFO[72usize], entry),
                32usize => (&INFO[74usize], entry),
                33usize => (&INFO[76usize], entry),
                34usize => (&INFO[78usize], entry),
                35usize => (&INFO[79usize], entry),
                36usize => (&INFO[81usize], entry),
                37usize => (&INFO[83usize], entry),
                38usize => (&INFO[85usize], entry),
                39usize => (&INFO[87usize], entry),
                40usize => (&INFO[89usize], entry),
                41usize => (&INFO[91usize], entry),
                42usize => (&INFO[93usize], entry),
                43usize => (&INFO[95usize], entry),
                44usize => (&INFO[97usize], entry),
                45usize => (&INFO[99usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count<T: Metric>(&self, info: usize, id: usize, value: T) {
        let info = &INFO[info];
        let counter = &self.counters[id];
        counter.record(info, value);
    }
    #[doc = r" Returns all of the registered bool counters"]
    #[inline]
    pub fn bool_counters(&self) -> impl Iterator<Item = (&'static Info, &R::BoolCounter)> + '_ {
        self.bool_counters
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[17usize], entry),
                1usize => (&INFO[30usize], entry),
                2usize => (&INFO[31usize], entry),
                3usize => (&INFO[32usize], entry),
                4usize => (&INFO[52usize], entry),
                5usize => (&INFO[53usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count_bool(&self, info: usize, id: usize, value: bool) {
        let info = &INFO[info];
        let counter = &self.bool_counters[id];
        counter.record(info, value);
    }
    #[doc = r" Returns all of the registered nominal counters"]
    #[inline]
    pub fn nominal_counters(
        &self,
    ) -> impl Iterator<Item = (&'static Info, &[R::NominalCounter], &[info::Variant])> + '_ {
        use api::*;
        self.nominal_counter_offsets
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => {
                    let offset = *entry;
                    let variants = <AcceptorTcpStreamDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[11usize], entries, variants)
                }
                1usize => {
                    let offset = *entry;
                    let variants = <AcceptorStreamPruneReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[38usize], entries, variants)
                }
                2usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[50usize], entries, variants)
                }
                3usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[51usize], entries, variants)
                }
                4usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[61usize], entries, variants)
                }
                5usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[63usize], entries, variants)
                }
                6usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[65usize], entries, variants)
                }
                7usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[67usize], entries, variants)
                }
                8usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[69usize], entries, variants)
                }
                9usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[71usize], entries, variants)
                }
                10usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[73usize], entries, variants)
                }
                11usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[75usize], entries, variants)
                }
                12usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[77usize], entries, variants)
                }
                13usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[82usize], entries, variants)
                }
                14usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[84usize], entries, variants)
                }
                15usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[86usize], entries, variants)
                }
                16usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[88usize], entries, variants)
                }
                17usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[90usize], entries, variants)
                }
                18usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[92usize], entries, variants)
                }
                19usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[94usize], entries, variants)
                }
                20usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[96usize], entries, variants)
                }
                21usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[98usize], entries, variants)
                }
                22usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[100usize], entries, variants)
                }
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count_nominal<T: AsVariant>(&self, info: usize, id: usize, value: &T) {
        let info = &INFO[info];
        let idx = self.nominal_counter_offsets[id] + value.variant_idx();
        let counter = &self.nominal_counters[idx];
        counter.record(info, value.as_variant(), 1usize);
    }
    #[doc = r" Returns all of the registered measures"]
    #[inline]
    pub fn measures(&self) -> impl Iterator<Item = (&'static Info, &R::Measure)> + '_ {
        self.measures
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[2usize], entry),
                1usize => (&INFO[4usize], entry),
                2usize => (&INFO[7usize], entry),
                3usize => (&INFO[8usize], entry),
                4usize => (&INFO[9usize], entry),
                5usize => (&INFO[14usize], entry),
                6usize => (&INFO[16usize], entry),
                7usize => (&INFO[23usize], entry),
                8usize => (&INFO[27usize], entry),
                9usize => (&INFO[29usize], entry),
                10usize => (&INFO[42usize], entry),
                11usize => (&INFO[44usize], entry),
                12usize => (&INFO[46usize], entry),
                13usize => (&INFO[48usize], entry),
                14usize => (&INFO[55usize], entry),
                15usize => (&INFO[57usize], entry),
                16usize => (&INFO[58usize], entry),
                17usize => (&INFO[59usize], entry),
                18usize => (&INFO[80usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn measure<T: Metric>(&self, info: usize, id: usize, value: T) {
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
    fn gauge<T: Metric>(&self, info: usize, id: usize, value: T) {
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
                0usize => (&INFO[3usize], entry),
                1usize => (&INFO[13usize], entry),
                2usize => (&INFO[18usize], entry),
                3usize => (&INFO[20usize], entry),
                4usize => (&INFO[22usize], entry),
                5usize => (&INFO[37usize], entry),
                6usize => (&INFO[40usize], entry),
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
    #[allow(dead_code)]
    #[inline(always)]
    fn time_nominal<T: AsVariant>(
        &self,
        info: usize,
        id: usize,
        value: &T,
        duration: core::time::Duration,
    ) {
        let info = &INFO[info];
        let idx = self.nominal_timer_offsets[id] + value.variant_idx();
        let counter = &self.nominal_timers[idx];
        counter.record(info, value.as_variant(), duration);
    }
}
impl<R: Registry> event::Subscriber for Subscriber<R> {
    type ConnectionContext = ConnectionContext;
    fn create_connection_context(
        &self,
        meta: &api::ConnectionMeta,
        _info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
        Self::ConnectionContext {
            start_time: meta.timestamp,
        }
    }
    #[inline]
    fn on_acceptor_tcp_started(&self, meta: &api::EndpointMeta, event: &api::AcceptorTcpStarted) {
        #[allow(unused_imports)]
        use api::*;
        self.count(0usize, 0usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_loop_iteration_completed(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpLoopIterationCompleted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(1usize, 1usize, 1usize);
        self.measure(2usize, 0usize, event.pending_streams);
        self.time(3usize, 0usize, event.processing_duration);
        self.measure(4usize, 1usize, event.max_sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_fresh_enqueued(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpFreshEnqueued,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(5usize, 2usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_fresh_batch_completed(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpFreshBatchCompleted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(6usize, 3usize, 1usize);
        self.measure(7usize, 2usize, event.enqueued);
        self.measure(8usize, 3usize, event.dropped);
        self.measure(9usize, 4usize, event.errored);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_stream_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpStreamDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(10usize, 4usize, 1usize);
        self.count_nominal(11usize, 0usize, &event.reason);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_stream_replaced(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpStreamReplaced,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(12usize, 5usize, 1usize);
        self.time(13usize, 1usize, event.sojourn_time);
        self.measure(14usize, 5usize, event.buffer_len);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(15usize, 6usize, 1usize);
        self.measure(16usize, 6usize, event.payload_len);
        self.count_bool(17usize, 0usize, event.is_fin);
        self.time(18usize, 2usize, event.sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(19usize, 7usize, 1usize);
        self.time(20usize, 3usize, event.sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_stream_enqueued(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpStreamEnqueued,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(21usize, 8usize, 1usize);
        self.time(22usize, 4usize, event.sojourn_time);
        self.measure(23usize, 7usize, event.blocked_count);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_io_error(&self, meta: &api::EndpointMeta, event: &api::AcceptorTcpIoError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(24usize, 9usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_started(&self, meta: &api::EndpointMeta, event: &api::AcceptorUdpStarted) {
        #[allow(unused_imports)]
        use api::*;
        self.count(25usize, 10usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_datagram_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorUdpDatagramReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(26usize, 11usize, 1usize);
        self.measure(27usize, 8usize, event.len);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorUdpPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(28usize, 12usize, 1usize);
        self.measure(29usize, 9usize, event.payload_len);
        self.count_bool(30usize, 1usize, event.is_zero_offset);
        self.count_bool(31usize, 2usize, event.is_retransmission);
        self.count_bool(32usize, 3usize, event.is_fin);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorUdpPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(33usize, 13usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_stream_enqueued(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorUdpStreamEnqueued,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(34usize, 14usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_io_error(&self, meta: &api::EndpointMeta, event: &api::AcceptorUdpIoError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(35usize, 15usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_stream_pruned(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorStreamPruned,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(36usize, 16usize, 1usize);
        self.time(37usize, 5usize, event.sojourn_time);
        self.count_nominal(38usize, 1usize, &event.reason);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_stream_dequeued(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorStreamDequeued,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(39usize, 17usize, 1usize);
        self.time(40usize, 6usize, event.sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_application_write(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ApplicationWrite,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(41usize, 18usize, 1usize);
        self.measure(42usize, 10usize, event.total_len);
        self.count(43usize, 19usize, event.write_len);
        self.measure(44usize, 11usize, event.write_len);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_application_read(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ApplicationRead,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(45usize, 20usize, 1usize);
        self.measure(46usize, 12usize, event.capacity);
        self.count(47usize, 21usize, event.read_len);
        self.measure(48usize, 13usize, event.read_len);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_endpoint_initialized(&self, meta: &api::EndpointMeta, event: &api::EndpointInitialized) {
        #[allow(unused_imports)]
        use api::*;
        self.count(49usize, 22usize, 1usize);
        self.count_nominal(50usize, 2usize, &event.acceptor_addr);
        self.count_nominal(51usize, 3usize, &event.handshake_addr);
        self.count_bool(52usize, 4usize, event.tcp);
        self.count_bool(53usize, 5usize, event.udp);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_initialized(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapInitialized,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(54usize, 23usize, 1usize);
        self.measure(55usize, 14usize, event.capacity);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_uninitialized(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapUninitialized,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(56usize, 24usize, 1usize);
        self.measure(57usize, 15usize, event.capacity);
        self.measure(58usize, 16usize, event.entries);
        self.measure(59usize, 17usize, event.lifetime);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_background_handshake_requested(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapBackgroundHandshakeRequested,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(60usize, 25usize, 1usize);
        self.count_nominal(61usize, 4usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_inserted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryInserted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(62usize, 26usize, 1usize);
        self.count_nominal(63usize, 5usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_ready(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryReady,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(64usize, 27usize, 1usize);
        self.count_nominal(65usize, 6usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_replaced(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryReplaced,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(66usize, 28usize, 1usize);
        self.count_nominal(67usize, 7usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_sent(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(68usize, 29usize, 1usize);
        self.count_nominal(69usize, 8usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(70usize, 30usize, 1usize);
        self.count_nominal(71usize, 9usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketAccepted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(72usize, 31usize, 1usize);
        self.count_nominal(73usize, 10usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketRejected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(74usize, 32usize, 1usize);
        self.count_nominal(75usize, 11usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(76usize, 33usize, 1usize);
        self.count_nominal(77usize, 12usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_definitely_detected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDefinitelyDetected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(78usize, 34usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_potentially_detected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayPotentiallyDetected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(79usize, 35usize, 1usize);
        self.measure(80usize, 18usize, event.gap);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_sent(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(81usize, 36usize, 1usize);
        self.count_nominal(82usize, 13usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(83usize, 37usize, 1usize);
        self.count_nominal(84usize, 14usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketAccepted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(85usize, 38usize, 1usize);
        self.count_nominal(86usize, 15usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketRejected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(87usize, 39usize, 1usize);
        self.count_nominal(88usize, 16usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(89usize, 40usize, 1usize);
        self.count_nominal(90usize, 17usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_sent(&self, meta: &api::EndpointMeta, event: &api::StaleKeyPacketSent) {
        #[allow(unused_imports)]
        use api::*;
        self.count(91usize, 41usize, 1usize);
        self.count_nominal(92usize, 18usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(93usize, 42usize, 1usize);
        self.count_nominal(94usize, 19usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketAccepted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(95usize, 43usize, 1usize);
        self.count_nominal(96usize, 20usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketRejected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(97usize, 44usize, 1usize);
        self.count_nominal(98usize, 21usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(99usize, 45usize, 1usize);
        self.count_nominal(100usize, 22usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
}
