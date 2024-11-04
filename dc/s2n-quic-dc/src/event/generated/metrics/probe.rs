// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::event::metrics::aggregate::{self, Info, Recorder};
use s2n_quic_core::probe::define;
mod counter {
    #![allow(non_snake_case)]
    use super::Info;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            match info.id {
                0usize => Self(application_write),
                2usize => Self(application_write__committed__total),
                4usize => Self(application_read),
                6usize => Self(application_read__committed__total),
                8usize => Self(endpoint_initialized),
                9usize => Self(path_secret_map_initialized),
                11usize => Self(path_secret_map_uninitialized),
                14usize => Self(path_secret_map_background_handshake_requested),
                15usize => Self(path_secret_map_entry_inserted),
                16usize => Self(path_secret_map_entry_ready),
                17usize => Self(path_secret_map_entry_replaced),
                18usize => Self(unknown_path_secret_packet_sent),
                19usize => Self(unknown_path_secret_packet_received),
                20usize => Self(unknown_path_secret_packet_accepted),
                21usize => Self(unknown_path_secret_packet_rejected),
                22usize => Self(unknown_path_secret_packet_dropped),
                23usize => Self(replay_definitely_detected),
                24usize => Self(replay_potentially_detected),
                26usize => Self(replay_detected_packet_sent),
                27usize => Self(replay_detected_packet_received),
                28usize => Self(replay_detected_packet_accepted),
                29usize => Self(replay_detected_packet_rejected),
                30usize => Self(replay_detected_packet_dropped),
                31usize => Self(stale_key_packet_sent),
                32usize => Self(stale_key_packet_received),
                33usize => Self(stale_key_packet_accepted),
                34usize => Self(stale_key_packet_rejected),
                35usize => Self(stale_key_packet_dropped),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl super::Recorder for Recorder {
        fn record(&self, _info: &'static Info, value: u64) {
            (self.0)(value);
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
mod measure {
    #![allow(non_snake_case)]
    use super::Info;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            match info.id {
                1usize => Self(application_write__provided),
                3usize => Self(application_write__committed),
                5usize => Self(application_read__capacity),
                7usize => Self(application_read__committed),
                10usize => Self(path_secret_map_initialized__capacity),
                12usize => Self(path_secret_map_uninitialized__capacity),
                13usize => Self(path_secret_map_uninitialized__entries),
                25usize => Self(replay_potentially_detected__gap),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl super::Recorder for Recorder {
        fn record(&self, _info: &'static Info, value: u64) {
            (self.0)(value);
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
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            unreachable!("invalid info: {info:?}")
        }
    }
    impl super::Recorder for Recorder {
        fn record(&self, _info: &'static Info, value: u64) {
            (self.0)(value);
        }
    }
}
mod timer {
    #![allow(non_snake_case)]
    use super::Info;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            unreachable!("invalid info: {info:?}")
        }
    }
    impl super::Recorder for Recorder {
        fn record(&self, _info: &'static Info, value: u64) {
            (self.0)(value);
        }
    }
}
#[derive(Default)]
pub struct Registry(());
impl aggregate::Registry for Registry {
    type Counter = counter::Recorder;
    type Measure = measure::Recorder;
    type Gauge = gauge::Recorder;
    type Timer = timer::Recorder;
    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        counter::Recorder::new(info)
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
