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
static INFO: &[Info; 36usize] = &[
    info::Builder {
        id: 0usize,
        name: Str::new("application_write\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 1usize,
        name: Str::new("application_write.provided\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 2usize,
        name: Str::new("application_write.committed.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 3usize,
        name: Str::new("application_write.committed\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 4usize,
        name: Str::new("application_read\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 5usize,
        name: Str::new("application_read.capacity\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 6usize,
        name: Str::new("application_read.committed.total\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 7usize,
        name: Str::new("application_read.committed\0"),
        units: Str::new("b\0"),
    }
    .build(),
    info::Builder {
        id: 8usize,
        name: Str::new("endpoint_initialized\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 9usize,
        name: Str::new("path_secret_map_initialized\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 10usize,
        name: Str::new("path_secret_map_initialized.capacity\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 11usize,
        name: Str::new("path_secret_map_uninitialized\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 12usize,
        name: Str::new("path_secret_map_uninitialized.capacity\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 13usize,
        name: Str::new("path_secret_map_uninitialized.entries\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 14usize,
        name: Str::new("path_secret_map_background_handshake_requested\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 15usize,
        name: Str::new("path_secret_map_entry_inserted\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 16usize,
        name: Str::new("path_secret_map_entry_ready\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 17usize,
        name: Str::new("path_secret_map_entry_replaced\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 18usize,
        name: Str::new("unknown_path_secret_packet_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 19usize,
        name: Str::new("unknown_path_secret_packet_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 20usize,
        name: Str::new("unknown_path_secret_packet_accepted\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 21usize,
        name: Str::new("unknown_path_secret_packet_rejected\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 22usize,
        name: Str::new("unknown_path_secret_packet_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 23usize,
        name: Str::new("replay_definitely_detected\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 24usize,
        name: Str::new("replay_potentially_detected\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 25usize,
        name: Str::new("replay_potentially_detected.gap\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 26usize,
        name: Str::new("replay_detected_packet_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 27usize,
        name: Str::new("replay_detected_packet_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 28usize,
        name: Str::new("replay_detected_packet_accepted\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 29usize,
        name: Str::new("replay_detected_packet_rejected\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 30usize,
        name: Str::new("replay_detected_packet_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 31usize,
        name: Str::new("stale_key_packet_sent\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 32usize,
        name: Str::new("stale_key_packet_received\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 33usize,
        name: Str::new("stale_key_packet_accepted\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 34usize,
        name: Str::new("stale_key_packet_rejected\0"),
        units: Str::new("\0"),
    }
    .build(),
    info::Builder {
        id: 35usize,
        name: Str::new("stale_key_packet_dropped\0"),
        units: Str::new("\0"),
    }
    .build(),
];
pub struct Subscriber<R: Registry> {
    #[allow(dead_code)]
    counters: Box<[R::Counter; 28usize]>,
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
        let mut measures = Vec::with_capacity(8usize);
        let mut gauges = Vec::with_capacity(0usize);
        let mut timers = Vec::with_capacity(0usize);
        counters.push(registry.register_counter(&INFO[0usize]));
        counters.push(registry.register_counter(&INFO[2usize]));
        counters.push(registry.register_counter(&INFO[4usize]));
        counters.push(registry.register_counter(&INFO[6usize]));
        counters.push(registry.register_counter(&INFO[8usize]));
        counters.push(registry.register_counter(&INFO[9usize]));
        counters.push(registry.register_counter(&INFO[11usize]));
        counters.push(registry.register_counter(&INFO[14usize]));
        counters.push(registry.register_counter(&INFO[15usize]));
        counters.push(registry.register_counter(&INFO[16usize]));
        counters.push(registry.register_counter(&INFO[17usize]));
        counters.push(registry.register_counter(&INFO[18usize]));
        counters.push(registry.register_counter(&INFO[19usize]));
        counters.push(registry.register_counter(&INFO[20usize]));
        counters.push(registry.register_counter(&INFO[21usize]));
        counters.push(registry.register_counter(&INFO[22usize]));
        counters.push(registry.register_counter(&INFO[23usize]));
        counters.push(registry.register_counter(&INFO[24usize]));
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
        measures.push(registry.register_measure(&INFO[1usize]));
        measures.push(registry.register_measure(&INFO[3usize]));
        measures.push(registry.register_measure(&INFO[5usize]));
        measures.push(registry.register_measure(&INFO[7usize]));
        measures.push(registry.register_measure(&INFO[10usize]));
        measures.push(registry.register_measure(&INFO[12usize]));
        measures.push(registry.register_measure(&INFO[13usize]));
        measures.push(registry.register_measure(&INFO[25usize]));
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
                1usize => (&INFO[2usize], entry),
                2usize => (&INFO[4usize], entry),
                3usize => (&INFO[6usize], entry),
                4usize => (&INFO[8usize], entry),
                5usize => (&INFO[9usize], entry),
                6usize => (&INFO[11usize], entry),
                7usize => (&INFO[14usize], entry),
                8usize => (&INFO[15usize], entry),
                9usize => (&INFO[16usize], entry),
                10usize => (&INFO[17usize], entry),
                11usize => (&INFO[18usize], entry),
                12usize => (&INFO[19usize], entry),
                13usize => (&INFO[20usize], entry),
                14usize => (&INFO[21usize], entry),
                15usize => (&INFO[22usize], entry),
                16usize => (&INFO[23usize], entry),
                17usize => (&INFO[24usize], entry),
                18usize => (&INFO[26usize], entry),
                19usize => (&INFO[27usize], entry),
                20usize => (&INFO[28usize], entry),
                21usize => (&INFO[29usize], entry),
                22usize => (&INFO[30usize], entry),
                23usize => (&INFO[31usize], entry),
                24usize => (&INFO[32usize], entry),
                25usize => (&INFO[33usize], entry),
                26usize => (&INFO[34usize], entry),
                27usize => (&INFO[35usize], entry),
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
                0usize => (&INFO[1usize], entry),
                1usize => (&INFO[3usize], entry),
                2usize => (&INFO[5usize], entry),
                3usize => (&INFO[7usize], entry),
                4usize => (&INFO[10usize], entry),
                5usize => (&INFO[12usize], entry),
                6usize => (&INFO[13usize], entry),
                7usize => (&INFO[25usize], entry),
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
        core::iter::empty()
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
        self.count(0usize, 0usize, 1);
        self.measure(1usize, 0usize, event.total_len.as_metric("b"));
        self.count(2usize, 1usize, event.write_len.as_metric("b"));
        self.measure(3usize, 1usize, event.write_len.as_metric("b"));
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
        self.count(4usize, 2usize, 1);
        self.measure(5usize, 2usize, event.capacity.as_metric("b"));
        self.count(6usize, 3usize, event.read_len.as_metric("b"));
        self.measure(7usize, 3usize, event.read_len.as_metric("b"));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_endpoint_initialized(&self, meta: &api::EndpointMeta, event: &api::EndpointInitialized) {
        self.count(8usize, 4usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_initialized(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapInitialized,
    ) {
        self.count(9usize, 5usize, 1);
        self.measure(10usize, 4usize, event.capacity.as_metric(""));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_uninitialized(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapUninitialized,
    ) {
        self.count(11usize, 6usize, 1);
        self.measure(12usize, 5usize, event.capacity.as_metric(""));
        self.measure(13usize, 6usize, event.entries.as_metric(""));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_background_handshake_requested(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapBackgroundHandshakeRequested,
    ) {
        self.count(14usize, 7usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_inserted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryInserted,
    ) {
        self.count(15usize, 8usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_ready(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryReady,
    ) {
        self.count(16usize, 9usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_replaced(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryReplaced,
    ) {
        self.count(17usize, 10usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_sent(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketSent,
    ) {
        self.count(18usize, 11usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketReceived,
    ) {
        self.count(19usize, 12usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketAccepted,
    ) {
        self.count(20usize, 13usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketRejected,
    ) {
        self.count(21usize, 14usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketDropped,
    ) {
        self.count(22usize, 15usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_definitely_detected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDefinitelyDetected,
    ) {
        self.count(23usize, 16usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_potentially_detected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayPotentiallyDetected,
    ) {
        self.count(24usize, 17usize, 1);
        self.measure(25usize, 7usize, event.gap.as_metric(""));
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_sent(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketSent,
    ) {
        self.count(26usize, 18usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketReceived,
    ) {
        self.count(27usize, 19usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketAccepted,
    ) {
        self.count(28usize, 20usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketRejected,
    ) {
        self.count(29usize, 21usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketDropped,
    ) {
        self.count(30usize, 22usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_sent(&self, meta: &api::EndpointMeta, event: &api::StaleKeyPacketSent) {
        self.count(31usize, 23usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketReceived,
    ) {
        self.count(32usize, 24usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketAccepted,
    ) {
        self.count(33usize, 25usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketRejected,
    ) {
        self.count(34usize, 26usize, 1);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketDropped,
    ) {
        self.count(35usize, 27usize, 1);
        let _ = event;
        let _ = meta;
    }
}
