// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::event::metrics::aggregate::{self, info, Info, NominalRecorder, Recorder};
use s2n_quic_core::probe::define;
mod counter {
    #![allow(non_snake_case)]
    use super::Info;
    use crate::event::metrics::aggregate::AsMetric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            match info.id {
                0usize => Self(application_write),
                2usize => Self(application_write__committed__total),
                4usize => Self(application_read),
                6usize => Self(application_read__committed__total),
                8usize => Self(endpoint_initialized),
                11usize => Self(path_secret_map_initialized),
                13usize => Self(path_secret_map_uninitialized),
                16usize => Self(path_secret_map_background_handshake_requested),
                18usize => Self(path_secret_map_entry_inserted),
                20usize => Self(path_secret_map_entry_ready),
                22usize => Self(path_secret_map_entry_replaced),
                24usize => Self(unknown_path_secret_packet_sent),
                26usize => Self(unknown_path_secret_packet_received),
                28usize => Self(unknown_path_secret_packet_accepted),
                30usize => Self(unknown_path_secret_packet_rejected),
                32usize => Self(unknown_path_secret_packet_dropped),
                34usize => Self(replay_definitely_detected),
                35usize => Self(replay_potentially_detected),
                37usize => Self(replay_detected_packet_sent),
                39usize => Self(replay_detected_packet_received),
                41usize => Self(replay_detected_packet_accepted),
                43usize => Self(replay_detected_packet_rejected),
                45usize => Self(replay_detected_packet_dropped),
                47usize => Self(stale_key_packet_sent),
                49usize => Self(stale_key_packet_received),
                51usize => Self(stale_key_packet_accepted),
                53usize => Self(stale_key_packet_rejected),
                55usize => Self(stale_key_packet_dropped),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl super::Recorder<u64> for Recorder {
        fn record(&self, _info: &'static Info, value: u64) {
            (self.0)(value);
        }
    }
    impl super::Recorder<core::time::Duration> for Recorder {
        fn record(&self, info: &'static Info, value: core::time::Duration) {
            (self.0)(value.as_metric(info.units));
        }
    }
    super::define!(
        extern "probe" {
            # [link_name = s2n_quic_dc__event__counter__application_write]
            fn application_write(value: u64);
            # [link_name = s2n_quic_dc__event__counter__application_write__committed__total]
            fn application_write__committed__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__application_read]
            fn application_read(value: u64);
            # [link_name = s2n_quic_dc__event__counter__application_read__committed__total]
            fn application_read__committed__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_initialized]
            fn endpoint_initialized(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_initialized]
            fn path_secret_map_initialized(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_uninitialized]
            fn path_secret_map_uninitialized(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_background_handshake_requested]
            fn path_secret_map_background_handshake_requested(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_entry_inserted]
            fn path_secret_map_entry_inserted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_entry_ready]
            fn path_secret_map_entry_ready(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_entry_replaced]
            fn path_secret_map_entry_replaced(value: u64);
            # [link_name = s2n_quic_dc__event__counter__unknown_path_secret_packet_sent]
            fn unknown_path_secret_packet_sent(value: u64);
            # [link_name = s2n_quic_dc__event__counter__unknown_path_secret_packet_received]
            fn unknown_path_secret_packet_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__unknown_path_secret_packet_accepted]
            fn unknown_path_secret_packet_accepted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__unknown_path_secret_packet_rejected]
            fn unknown_path_secret_packet_rejected(value: u64);
            # [link_name = s2n_quic_dc__event__counter__unknown_path_secret_packet_dropped]
            fn unknown_path_secret_packet_dropped(value: u64);
            # [link_name = s2n_quic_dc__event__counter__replay_definitely_detected]
            fn replay_definitely_detected(value: u64);
            # [link_name = s2n_quic_dc__event__counter__replay_potentially_detected]
            fn replay_potentially_detected(value: u64);
            # [link_name = s2n_quic_dc__event__counter__replay_detected_packet_sent]
            fn replay_detected_packet_sent(value: u64);
            # [link_name = s2n_quic_dc__event__counter__replay_detected_packet_received]
            fn replay_detected_packet_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__replay_detected_packet_accepted]
            fn replay_detected_packet_accepted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__replay_detected_packet_rejected]
            fn replay_detected_packet_rejected(value: u64);
            # [link_name = s2n_quic_dc__event__counter__replay_detected_packet_dropped]
            fn replay_detected_packet_dropped(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stale_key_packet_sent]
            fn stale_key_packet_sent(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stale_key_packet_received]
            fn stale_key_packet_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stale_key_packet_accepted]
            fn stale_key_packet_accepted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stale_key_packet_rejected]
            fn stale_key_packet_rejected(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stale_key_packet_dropped]
            fn stale_key_packet_dropped(value: u64);
        }
    );
}
mod nominal_counter {
    #![allow(non_snake_case)]
    use super::{info, Info};
    use crate::event::metrics::aggregate::AsMetric;
    pub struct Recorder(fn(u64, u64, &info::Str));
    impl Recorder {
        pub(super) fn new(info: &'static Info, _variant: &'static info::Variant) -> Self {
            match info.id {
                9usize => Self(endpoint_initialized__tcp),
                10usize => Self(endpoint_initialized__udp),
                17usize => {
                    Self(path_secret_map_background_handshake_requested__peer_address__protocol)
                }
                19usize => Self(path_secret_map_entry_inserted__peer_address__protocol),
                21usize => Self(path_secret_map_entry_ready__peer_address__protocol),
                23usize => Self(path_secret_map_entry_replaced__peer_address__protocol),
                25usize => Self(unknown_path_secret_packet_sent__peer_address__protocol),
                27usize => Self(unknown_path_secret_packet_received__peer_address__protocol),
                29usize => Self(unknown_path_secret_packet_accepted__peer_address__protocol),
                31usize => Self(unknown_path_secret_packet_rejected__peer_address__protocol),
                33usize => Self(unknown_path_secret_packet_dropped__peer_address__protocol),
                38usize => Self(replay_detected_packet_sent__peer_address__protocol),
                40usize => Self(replay_detected_packet_received__peer_address__protocol),
                42usize => Self(replay_detected_packet_accepted__peer_address__protocol),
                44usize => Self(replay_detected_packet_rejected__peer_address__protocol),
                46usize => Self(replay_detected_packet_dropped__peer_address__protocol),
                48usize => Self(stale_key_packet_sent__peer_address__protocol),
                50usize => Self(stale_key_packet_received__peer_address__protocol),
                52usize => Self(stale_key_packet_accepted__peer_address__protocol),
                54usize => Self(stale_key_packet_rejected__peer_address__protocol),
                56usize => Self(stale_key_packet_dropped__peer_address__protocol),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl super::NominalRecorder<u64> for Recorder {
        fn record(&self, _info: &'static Info, variant: &'static info::Variant, value: u64) {
            (self.0)(value, variant.id as _, variant.name);
        }
    }
    impl super::NominalRecorder<core::time::Duration> for Recorder {
        fn record(
            &self,
            info: &'static Info,
            variant: &'static info::Variant,
            value: core::time::Duration,
        ) {
            (self.0)(value.as_metric(info.units), variant.id as _, variant.name);
        }
    }
    super::define!(
        extern "probe" {
            # [link_name = s2n_quic_dc__event__nominal_counter__endpoint_initialized__tcp]
            fn endpoint_initialized__tcp(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic_dc__event__nominal_counter__endpoint_initialized__udp]
            fn endpoint_initialized__udp(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic_dc__event__nominal_counter__path_secret_map_background_handshake_requested__peer_address__protocol]
            fn path_secret_map_background_handshake_requested__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__path_secret_map_entry_inserted__peer_address__protocol]
            fn path_secret_map_entry_inserted__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__path_secret_map_entry_ready__peer_address__protocol]
            fn path_secret_map_entry_ready__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__path_secret_map_entry_replaced__peer_address__protocol]
            fn path_secret_map_entry_replaced__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__unknown_path_secret_packet_sent__peer_address__protocol]
            fn unknown_path_secret_packet_sent__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__unknown_path_secret_packet_received__peer_address__protocol]
            fn unknown_path_secret_packet_received__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__unknown_path_secret_packet_accepted__peer_address__protocol]
            fn unknown_path_secret_packet_accepted__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__unknown_path_secret_packet_rejected__peer_address__protocol]
            fn unknown_path_secret_packet_rejected__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__unknown_path_secret_packet_dropped__peer_address__protocol]
            fn unknown_path_secret_packet_dropped__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__replay_detected_packet_sent__peer_address__protocol]
            fn replay_detected_packet_sent__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__replay_detected_packet_received__peer_address__protocol]
            fn replay_detected_packet_received__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__replay_detected_packet_accepted__peer_address__protocol]
            fn replay_detected_packet_accepted__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__replay_detected_packet_rejected__peer_address__protocol]
            fn replay_detected_packet_rejected__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__replay_detected_packet_dropped__peer_address__protocol]
            fn replay_detected_packet_dropped__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__stale_key_packet_sent__peer_address__protocol]
            fn stale_key_packet_sent__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__stale_key_packet_received__peer_address__protocol]
            fn stale_key_packet_received__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__stale_key_packet_accepted__peer_address__protocol]
            fn stale_key_packet_accepted__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__stale_key_packet_rejected__peer_address__protocol]
            fn stale_key_packet_rejected__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic_dc__event__nominal_counter__stale_key_packet_dropped__peer_address__protocol]
            fn stale_key_packet_dropped__peer_address__protocol(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
        }
    );
}
mod measure {
    #![allow(non_snake_case)]
    use super::Info;
    use crate::event::metrics::aggregate::AsMetric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            match info.id {
                1usize => Self(application_write__provided),
                3usize => Self(application_write__committed),
                5usize => Self(application_read__capacity),
                7usize => Self(application_read__committed),
                12usize => Self(path_secret_map_initialized__capacity),
                14usize => Self(path_secret_map_uninitialized__capacity),
                15usize => Self(path_secret_map_uninitialized__entries),
                36usize => Self(replay_potentially_detected__gap),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl super::Recorder<u64> for Recorder {
        fn record(&self, _info: &'static Info, value: u64) {
            (self.0)(value);
        }
    }
    impl super::Recorder<core::time::Duration> for Recorder {
        fn record(&self, info: &'static Info, value: core::time::Duration) {
            (self.0)(value.as_metric(info.units));
        }
    }
    super::define!(
        extern "probe" {
            # [link_name = s2n_quic_dc__event__measure__application_write__provided]
            fn application_write__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__application_write__committed]
            fn application_write__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__application_read__capacity]
            fn application_read__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__application_read__committed]
            fn application_read__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_initialized__capacity]
            fn path_secret_map_initialized__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_uninitialized__capacity]
            fn path_secret_map_uninitialized__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_uninitialized__entries]
            fn path_secret_map_uninitialized__entries(value: u64);
            # [link_name = s2n_quic_dc__event__measure__replay_potentially_detected__gap]
            fn replay_potentially_detected__gap(value: u64);
        }
    );
}
mod gauge {
    #![allow(non_snake_case)]
    use super::Info;
    use crate::event::metrics::aggregate::AsMetric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            unreachable!("invalid info: {info:?}")
        }
    }
    impl super::Recorder<u64> for Recorder {
        fn record(&self, _info: &'static Info, value: u64) {
            (self.0)(value);
        }
    }
    impl super::Recorder<core::time::Duration> for Recorder {
        fn record(&self, info: &'static Info, value: core::time::Duration) {
            (self.0)(value.as_metric(info.units));
        }
    }
}
mod timer {
    #![allow(non_snake_case)]
    use super::Info;
    use crate::event::metrics::aggregate::AsMetric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            unreachable!("invalid info: {info:?}")
        }
    }
    impl super::Recorder<u64> for Recorder {
        fn record(&self, _info: &'static Info, value: u64) {
            (self.0)(value);
        }
    }
    impl super::Recorder<core::time::Duration> for Recorder {
        fn record(&self, info: &'static Info, value: core::time::Duration) {
            (self.0)(value.as_metric(info.units));
        }
    }
}
#[derive(Default)]
pub struct Registry(());
impl aggregate::Registry for Registry {
    type Counter = counter::Recorder;
    type NominalCounter = nominal_counter::Recorder;
    type Measure = measure::Recorder;
    type Gauge = gauge::Recorder;
    type Timer = timer::Recorder;
    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        counter::Recorder::new(info)
    }
    #[inline]
    fn register_nominal_counter(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalCounter {
        nominal_counter::Recorder::new(info, variant)
    }
    #[inline]
    fn register_measure(&self, info: &'static Info) -> Self::Measure {
        measure::Recorder::new(info)
    }
    #[inline]
    fn register_gauge(&self, info: &'static Info) -> Self::Gauge {
        gauge::Recorder::new(info)
    }
    #[inline]
    fn register_timer(&self, info: &'static Info) -> Self::Timer {
        timer::Recorder::new(info)
    }
}
