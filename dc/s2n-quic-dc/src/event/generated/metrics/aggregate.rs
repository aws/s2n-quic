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
static INFO: &[Info; 59usize] = &[
    info::Builder {
        id: 0usize,
        name: Str::new("application_write\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 1usize,
        name: Str::new("application_write.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 2usize,
        name: Str::new("application_write.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 3usize,
        name: Str::new("application_write.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 4usize,
        name: Str::new("application_read\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 5usize,
        name: Str::new("application_read.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 6usize,
        name: Str::new("application_read.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 7usize,
        name: Str::new("application_read.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 8usize,
        name: Str::new("endpoint_initialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 9usize,
        name: Str::new("endpoint_initialized.acceptor.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 10usize,
        name: Str::new("endpoint_initialized.handshake.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 11usize,
        name: Str::new("endpoint_initialized.tcp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 12usize,
        name: Str::new("endpoint_initialized.udp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 13usize,
        name: Str::new("path_secret_map_initialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 14usize,
        name: Str::new("path_secret_map_initialized.capacity\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 15usize,
        name: Str::new("path_secret_map_uninitialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 16usize,
        name: Str::new("path_secret_map_uninitialized.capacity\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 17usize,
        name: Str::new("path_secret_map_uninitialized.entries\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 18usize,
        name: Str::new("path_secret_map_background_handshake_requested\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 19usize,
        name: Str::new("path_secret_map_background_handshake_requested.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 20usize,
        name: Str::new("path_secret_map_entry_inserted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 21usize,
        name: Str::new("path_secret_map_entry_inserted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 22usize,
        name: Str::new("path_secret_map_entry_ready\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 23usize,
        name: Str::new("path_secret_map_entry_ready.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 24usize,
        name: Str::new("path_secret_map_entry_replaced\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 25usize,
        name: Str::new("path_secret_map_entry_replaced.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 26usize,
        name: Str::new("unknown_path_secret_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 27usize,
        name: Str::new("unknown_path_secret_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 28usize,
        name: Str::new("unknown_path_secret_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 29usize,
        name: Str::new("unknown_path_secret_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 30usize,
        name: Str::new("unknown_path_secret_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 31usize,
        name: Str::new("unknown_path_secret_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 32usize,
        name: Str::new("unknown_path_secret_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 33usize,
        name: Str::new("unknown_path_secret_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 34usize,
        name: Str::new("unknown_path_secret_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 35usize,
        name: Str::new("unknown_path_secret_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 36usize,
        name: Str::new("replay_definitely_detected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 37usize,
        name: Str::new("replay_potentially_detected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 38usize,
        name: Str::new("replay_potentially_detected.gap\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 39usize,
        name: Str::new("replay_detected_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 40usize,
        name: Str::new("replay_detected_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 41usize,
        name: Str::new("replay_detected_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 42usize,
        name: Str::new("replay_detected_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 43usize,
        name: Str::new("replay_detected_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 44usize,
        name: Str::new("replay_detected_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 45usize,
        name: Str::new("replay_detected_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 46usize,
        name: Str::new("replay_detected_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 47usize,
        name: Str::new("replay_detected_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 48usize,
        name: Str::new("replay_detected_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 49usize,
        name: Str::new("stale_key_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 50usize,
        name: Str::new("stale_key_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 51usize,
        name: Str::new("stale_key_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 52usize,
        name: Str::new("stale_key_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 53usize,
        name: Str::new("stale_key_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 54usize,
        name: Str::new("stale_key_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 55usize,
        name: Str::new("stale_key_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 56usize,
        name: Str::new("stale_key_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 57usize,
        name: Str::new("stale_key_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 58usize,
        name: Str::new("stale_key_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
];
pub struct Subscriber<R: Registry> {
    #[allow(dead_code)]
    counters: Box<[R::Counter; 28usize]>,
    #[allow(dead_code)]
    bool_counters: Box<[R::BoolCounter; 2usize]>,
    #[allow(dead_code)]
    nominal_counters: Box<[R::NominalCounter]>,
    #[allow(dead_code)]
    nominal_offsets: Box<[usize; 21usize]>,
    #[allow(dead_code)]
    measures: Box<[R::Measure; 8usize]>,
    #[allow(dead_code)]
    gauges: Box<[R::Gauge; 0usize]>,
    #[allow(dead_code)]
    timers: Box<[R::Timer; 0usize]>,
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
        let mut counters = Vec::with_capacity(28usize);
        let mut bool_counters = Vec::with_capacity(2usize);
        let mut nominal_offsets = Vec::with_capacity(21usize);
        let mut nominal_counters = Vec::with_capacity(21usize);
        let mut measures = Vec::with_capacity(8usize);
        let mut gauges = Vec::with_capacity(0usize);
        let mut timers = Vec::with_capacity(0usize);
        counters.push(registry.register_counter(&INFO[0usize]));
        counters.push(registry.register_counter(&INFO[2usize]));
        counters.push(registry.register_counter(&INFO[4usize]));
        counters.push(registry.register_counter(&INFO[6usize]));
        counters.push(registry.register_counter(&INFO[8usize]));
        counters.push(registry.register_counter(&INFO[13usize]));
        counters.push(registry.register_counter(&INFO[15usize]));
        counters.push(registry.register_counter(&INFO[18usize]));
        counters.push(registry.register_counter(&INFO[20usize]));
        counters.push(registry.register_counter(&INFO[22usize]));
        counters.push(registry.register_counter(&INFO[24usize]));
        counters.push(registry.register_counter(&INFO[26usize]));
        counters.push(registry.register_counter(&INFO[28usize]));
        counters.push(registry.register_counter(&INFO[30usize]));
        counters.push(registry.register_counter(&INFO[32usize]));
        counters.push(registry.register_counter(&INFO[34usize]));
        counters.push(registry.register_counter(&INFO[36usize]));
        counters.push(registry.register_counter(&INFO[37usize]));
        counters.push(registry.register_counter(&INFO[39usize]));
        counters.push(registry.register_counter(&INFO[41usize]));
        counters.push(registry.register_counter(&INFO[43usize]));
        counters.push(registry.register_counter(&INFO[45usize]));
        counters.push(registry.register_counter(&INFO[47usize]));
        counters.push(registry.register_counter(&INFO[49usize]));
        counters.push(registry.register_counter(&INFO[51usize]));
        counters.push(registry.register_counter(&INFO[53usize]));
        counters.push(registry.register_counter(&INFO[55usize]));
        counters.push(registry.register_counter(&INFO[57usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[11usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[12usize]));
        {
            #[allow(unused_imports)]
            use api::*;
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[9usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
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
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[19usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[21usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[23usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[25usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[27usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[29usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[31usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
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
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[35usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[40usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[42usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[44usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[46usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[48usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
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
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[52usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[54usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
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
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[58usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_offsets.push(offset);
            }
        }
        measures.push(registry.register_measure(&INFO[1usize]));
        measures.push(registry.register_measure(&INFO[3usize]));
        measures.push(registry.register_measure(&INFO[5usize]));
        measures.push(registry.register_measure(&INFO[7usize]));
        measures.push(registry.register_measure(&INFO[14usize]));
        measures.push(registry.register_measure(&INFO[16usize]));
        measures.push(registry.register_measure(&INFO[17usize]));
        measures.push(registry.register_measure(&INFO[38usize]));
        Self {
            counters: counters
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            bool_counters: bool_counters
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
                1usize => (&INFO[2usize], entry),
                2usize => (&INFO[4usize], entry),
                3usize => (&INFO[6usize], entry),
                4usize => (&INFO[8usize], entry),
                5usize => (&INFO[13usize], entry),
                6usize => (&INFO[15usize], entry),
                7usize => (&INFO[18usize], entry),
                8usize => (&INFO[20usize], entry),
                9usize => (&INFO[22usize], entry),
                10usize => (&INFO[24usize], entry),
                11usize => (&INFO[26usize], entry),
                12usize => (&INFO[28usize], entry),
                13usize => (&INFO[30usize], entry),
                14usize => (&INFO[32usize], entry),
                15usize => (&INFO[34usize], entry),
                16usize => (&INFO[36usize], entry),
                17usize => (&INFO[37usize], entry),
                18usize => (&INFO[39usize], entry),
                19usize => (&INFO[41usize], entry),
                20usize => (&INFO[43usize], entry),
                21usize => (&INFO[45usize], entry),
                22usize => (&INFO[47usize], entry),
                23usize => (&INFO[49usize], entry),
                24usize => (&INFO[51usize], entry),
                25usize => (&INFO[53usize], entry),
                26usize => (&INFO[55usize], entry),
                27usize => (&INFO[57usize], entry),
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
                0usize => (&INFO[11usize], entry),
                1usize => (&INFO[12usize], entry),
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
        self.nominal_offsets
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[9usize], entries, variants)
                }
                1usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[10usize], entries, variants)
                }
                2usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[19usize], entries, variants)
                }
                3usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[21usize], entries, variants)
                }
                4usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[23usize], entries, variants)
                }
                5usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[25usize], entries, variants)
                }
                6usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[27usize], entries, variants)
                }
                7usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[29usize], entries, variants)
                }
                8usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[31usize], entries, variants)
                }
                9usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[33usize], entries, variants)
                }
                10usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[35usize], entries, variants)
                }
                11usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[40usize], entries, variants)
                }
                12usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[42usize], entries, variants)
                }
                13usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[44usize], entries, variants)
                }
                14usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[46usize], entries, variants)
                }
                15usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[48usize], entries, variants)
                }
                16usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[50usize], entries, variants)
                }
                17usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[52usize], entries, variants)
                }
                18usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[54usize], entries, variants)
                }
                19usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[56usize], entries, variants)
                }
                20usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[58usize], entries, variants)
                }
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count_nominal<T: AsVariant>(&self, info: usize, id: usize, value: &T) {
        let info = &INFO[info];
        let idx = self.nominal_offsets[id] + value.variant_idx();
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
                0usize => (&INFO[1usize], entry),
                1usize => (&INFO[3usize], entry),
                2usize => (&INFO[5usize], entry),
                3usize => (&INFO[7usize], entry),
                4usize => (&INFO[14usize], entry),
                5usize => (&INFO[16usize], entry),
                6usize => (&INFO[17usize], entry),
                7usize => (&INFO[38usize], entry),
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
        core::iter::empty()
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
        &self,
        _meta: &api::ConnectionMeta,
        _info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }
    #[inline]
    fn on_application_write(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ApplicationWrite,
    ) {
        self.count(0usize, 0usize, 1usize);
        self.measure(1usize, 0usize, event.total_len);
        self.count(2usize, 1usize, event.write_len);
        self.measure(3usize, 1usize, event.write_len);
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
        self.count(4usize, 2usize, 1usize);
        self.measure(5usize, 2usize, event.capacity);
        self.count(6usize, 3usize, event.read_len);
        self.measure(7usize, 3usize, event.read_len);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_endpoint_initialized(&self, meta: &api::EndpointMeta, event: &api::EndpointInitialized) {
        self.count(8usize, 4usize, 1usize);
        self.count_nominal(9usize, 0usize, &event.acceptor_addr);
        self.count_nominal(10usize, 1usize, &event.handshake_addr);
        self.count_bool(11usize, 0usize, event.tcp);
        self.count_bool(12usize, 1usize, event.udp);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_initialized(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapInitialized,
    ) {
        self.count(13usize, 5usize, 1usize);
        self.measure(14usize, 4usize, event.capacity);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_uninitialized(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapUninitialized,
    ) {
        self.count(15usize, 6usize, 1usize);
        self.measure(16usize, 5usize, event.capacity);
        self.measure(17usize, 6usize, event.entries);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_background_handshake_requested(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapBackgroundHandshakeRequested,
    ) {
        self.count(18usize, 7usize, 1usize);
        self.count_nominal(19usize, 2usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_inserted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryInserted,
    ) {
        self.count(20usize, 8usize, 1usize);
        self.count_nominal(21usize, 3usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_ready(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryReady,
    ) {
        self.count(22usize, 9usize, 1usize);
        self.count_nominal(23usize, 4usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_replaced(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryReplaced,
    ) {
        self.count(24usize, 10usize, 1usize);
        self.count_nominal(25usize, 5usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_sent(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketSent,
    ) {
        self.count(26usize, 11usize, 1usize);
        self.count_nominal(27usize, 6usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketReceived,
    ) {
        self.count(28usize, 12usize, 1usize);
        self.count_nominal(29usize, 7usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketAccepted,
    ) {
        self.count(30usize, 13usize, 1usize);
        self.count_nominal(31usize, 8usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketRejected,
    ) {
        self.count(32usize, 14usize, 1usize);
        self.count_nominal(33usize, 9usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketDropped,
    ) {
        self.count(34usize, 15usize, 1usize);
        self.count_nominal(35usize, 10usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_definitely_detected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDefinitelyDetected,
    ) {
        self.count(36usize, 16usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_potentially_detected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayPotentiallyDetected,
    ) {
        self.count(37usize, 17usize, 1usize);
        self.measure(38usize, 7usize, event.gap);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_sent(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketSent,
    ) {
        self.count(39usize, 18usize, 1usize);
        self.count_nominal(40usize, 11usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketReceived,
    ) {
        self.count(41usize, 19usize, 1usize);
        self.count_nominal(42usize, 12usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketAccepted,
    ) {
        self.count(43usize, 20usize, 1usize);
        self.count_nominal(44usize, 13usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketRejected,
    ) {
        self.count(45usize, 21usize, 1usize);
        self.count_nominal(46usize, 14usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketDropped,
    ) {
        self.count(47usize, 22usize, 1usize);
        self.count_nominal(48usize, 15usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_sent(&self, meta: &api::EndpointMeta, event: &api::StaleKeyPacketSent) {
        self.count(49usize, 23usize, 1usize);
        self.count_nominal(50usize, 16usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketReceived,
    ) {
        self.count(51usize, 24usize, 1usize);
        self.count_nominal(52usize, 17usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketAccepted,
    ) {
        self.count(53usize, 25usize, 1usize);
        self.count_nominal(54usize, 18usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketRejected,
    ) {
        self.count(55usize, 26usize, 1usize);
        self.count_nominal(56usize, 19usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketDropped,
    ) {
        self.count(57usize, 27usize, 1usize);
        self.count_nominal(58usize, 20usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
}
