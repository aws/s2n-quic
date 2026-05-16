// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-events` crate and any required
// changes should be made there.

use crate::event::metrics::aggregate::{
    self, info, BoolRecorder, Info, NominalRecorder, Recorder as MetricRecorder,
};
use s2n_quic_core::probe::define;
mod counter {
    #![allow(non_snake_case)]
    use super::*;
    use crate::event::metrics::aggregate::Metric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(crate) fn new(info: &'static Info) -> Self {
            match info.id {
                0usize => Self(stream_write_key_updated),
                1usize => Self(stream_read_key_updated),
                2usize => Self(endpoint_initialized),
                7usize => Self(dc_connection_timeout),
                9usize => Self(endpoint_udp_immediate_transmission_scheduled),
                14usize => Self(endpoint_udp_transmission_scheduled),
                20usize => Self(endpoint_udp_transmission_rejected),
                27usize => Self(endpoint_udp_packet_transmitted),
                32usize => Self(endpoint_udp_transmit_errored),
                37usize => Self(endpoint_udp_packet_received),
                42usize => Self(endpoint_udp_receive_errored),
                43usize => Self(path_secret_map_initialized),
                45usize => Self(path_secret_map_uninitialized),
                49usize => Self(path_secret_map_background_handshake_requested),
                51usize => Self(path_secret_map_entry_inserted),
                53usize => Self(path_secret_map_entry_ready),
                55usize => Self(path_secret_map_entry_replaced),
                57usize => Self(path_secret_map_id_entry_evicted),
                60usize => Self(path_secret_map_address_entry_evicted),
                63usize => Self(unknown_path_secret_packet_sent),
                65usize => Self(unknown_path_secret_packet_received),
                67usize => Self(unknown_path_secret_packet_accepted),
                69usize => Self(unknown_path_secret_packet_rejected),
                71usize => Self(unknown_path_secret_packet_dropped),
                73usize => Self(key_accepted),
                76usize => Self(replay_definitely_detected),
                77usize => Self(replay_potentially_detected),
                79usize => Self(replay_detected_packet_sent),
                81usize => Self(replay_detected_packet_received),
                83usize => Self(replay_detected_packet_accepted),
                85usize => Self(replay_detected_packet_rejected),
                87usize => Self(replay_detected_packet_dropped),
                89usize => Self(stale_key_packet_sent),
                91usize => Self(stale_key_packet_received),
                93usize => Self(stale_key_packet_accepted),
                95usize => Self(stale_key_packet_rejected),
                97usize => Self(stale_key_packet_dropped),
                99usize => Self(path_secret_map_address_cache_accessed),
                102usize => Self(path_secret_map_address_cache_accessed_hit),
                105usize => Self(path_secret_map_id_cache_accessed),
                107usize => Self(path_secret_map_id_cache_accessed_hit),
                109usize => Self(path_secret_map_cleaner_cycled),
                126usize => Self(path_secret_map_id_write_lock),
                129usize => Self(path_secret_map_address_write_lock),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl MetricRecorder for Recorder {
        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
            (self.0)(value.as_u64());
        }
    }
    define!(
        extern "probe" {
            # [link_name = s2n_quic_dc__event__counter__stream_write_key_updated]
            fn stream_write_key_updated(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_key_updated]
            fn stream_read_key_updated(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_initialized]
            fn endpoint_initialized(value: u64);
            # [link_name = s2n_quic_dc__event__counter__dc_connection_timeout]
            fn dc_connection_timeout(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_udp_immediate_transmission_scheduled]
            fn endpoint_udp_immediate_transmission_scheduled(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_udp_transmission_scheduled]
            fn endpoint_udp_transmission_scheduled(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_udp_transmission_rejected]
            fn endpoint_udp_transmission_rejected(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_udp_packet_transmitted]
            fn endpoint_udp_packet_transmitted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_udp_transmit_errored]
            fn endpoint_udp_transmit_errored(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_udp_packet_received]
            fn endpoint_udp_packet_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_udp_receive_errored]
            fn endpoint_udp_receive_errored(value: u64);
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
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_id_entry_evicted]
            fn path_secret_map_id_entry_evicted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_address_entry_evicted]
            fn path_secret_map_address_entry_evicted(value: u64);
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
            # [link_name = s2n_quic_dc__event__counter__key_accepted]
            fn key_accepted(value: u64);
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
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_address_cache_accessed]
            fn path_secret_map_address_cache_accessed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_address_cache_accessed_hit]
            fn path_secret_map_address_cache_accessed_hit(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_id_cache_accessed]
            fn path_secret_map_id_cache_accessed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_id_cache_accessed_hit]
            fn path_secret_map_id_cache_accessed_hit(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_cleaner_cycled]
            fn path_secret_map_cleaner_cycled(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_id_write_lock]
            fn path_secret_map_id_write_lock(value: u64);
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_address_write_lock]
            fn path_secret_map_address_write_lock(value: u64);
        }
    );
    pub mod bool {
        #![allow(non_snake_case)]
        use super::*;
        pub struct Recorder(fn(bool));
        impl Recorder {
            pub(crate) fn new(info: &'static Info) -> Self {
                match info.id {
                    5usize => Self(endpoint_initialized__tcp),
                    6usize => Self(endpoint_initialized__udp),
                    101usize => Self(path_secret_map_address_cache_accessed__hit),
                    106usize => Self(path_secret_map_id_cache_accessed__hit),
                    _ => unreachable!("invalid info: {info:?}"),
                }
            }
        }
        impl BoolRecorder for Recorder {
            fn record(&self, _info: &'static Info, value: bool) {
                (self.0)(value);
            }
        }
        define!(
            extern "probe" {
                # [link_name = s2n_quic_dc__event__counter__bool__endpoint_initialized__tcp]
                fn endpoint_initialized__tcp(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__endpoint_initialized__udp]
                fn endpoint_initialized__udp(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__path_secret_map_address_cache_accessed__hit]
                fn path_secret_map_address_cache_accessed__hit(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__path_secret_map_id_cache_accessed__hit]
                fn path_secret_map_id_cache_accessed__hit(value: bool);
            }
        );
    }
    pub mod nominal {
        #![allow(non_snake_case)]
        use super::*;
        use crate::event::metrics::aggregate::Metric;
        pub struct Recorder(fn(u64, u64, &info::Str));
        impl Recorder {
            pub(crate) fn new(info: &'static Info, _variant: &'static info::Variant) -> Self {
                match info.id {
                    3usize => Self(endpoint_initialized__acceptor__protocol),
                    4usize => Self(endpoint_initialized__handshake__protocol),
                    8usize => Self(dc_connection_timeout__peer_address__protocol),
                    10usize => Self(endpoint_udp_immediate_transmission_scheduled__peer__protocol),
                    15usize => Self(endpoint_udp_transmission_scheduled__peer__protocol),
                    21usize => Self(endpoint_udp_transmission_rejected__peer__protocol),
                    28usize => Self(endpoint_udp_packet_transmitted__peer__protocol),
                    33usize => Self(endpoint_udp_transmit_errored__peer__protocol),
                    38usize => Self(endpoint_udp_packet_received__peer__protocol),
                    50usize => {
                        Self(path_secret_map_background_handshake_requested__peer_address__protocol)
                    }
                    52usize => Self(path_secret_map_entry_inserted__peer_address__protocol),
                    54usize => Self(path_secret_map_entry_ready__peer_address__protocol),
                    56usize => Self(path_secret_map_entry_replaced__peer_address__protocol),
                    58usize => Self(path_secret_map_id_entry_evicted__peer_address__protocol),
                    61usize => Self(path_secret_map_address_entry_evicted__peer_address__protocol),
                    64usize => Self(unknown_path_secret_packet_sent__peer_address__protocol),
                    66usize => Self(unknown_path_secret_packet_received__peer_address__protocol),
                    68usize => Self(unknown_path_secret_packet_accepted__peer_address__protocol),
                    70usize => Self(unknown_path_secret_packet_rejected__peer_address__protocol),
                    72usize => Self(unknown_path_secret_packet_dropped__peer_address__protocol),
                    80usize => Self(replay_detected_packet_sent__peer_address__protocol),
                    82usize => Self(replay_detected_packet_received__peer_address__protocol),
                    84usize => Self(replay_detected_packet_accepted__peer_address__protocol),
                    86usize => Self(replay_detected_packet_rejected__peer_address__protocol),
                    88usize => Self(replay_detected_packet_dropped__peer_address__protocol),
                    90usize => Self(stale_key_packet_sent__peer_address__protocol),
                    92usize => Self(stale_key_packet_received__peer_address__protocol),
                    94usize => Self(stale_key_packet_accepted__peer_address__protocol),
                    96usize => Self(stale_key_packet_rejected__peer_address__protocol),
                    98usize => Self(stale_key_packet_dropped__peer_address__protocol),
                    100usize => {
                        Self(path_secret_map_address_cache_accessed__peer_address__protocol)
                    }
                    103usize => {
                        Self(path_secret_map_address_cache_accessed_hit__peer_address__protocol)
                    }
                    _ => unreachable!("invalid info: {info:?}"),
                }
            }
        }
        impl NominalRecorder for Recorder {
            fn record<T: Metric>(
                &self,
                _info: &'static Info,
                variant: &'static info::Variant,
                value: T,
            ) {
                (self.0)(value.as_u64(), variant.id as _, variant.name);
            }
        }
        define!(
            extern "probe" {
                # [link_name = s2n_quic_dc__event__counter__nominal__endpoint_initialized__acceptor__protocol]
                fn endpoint_initialized__acceptor__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__endpoint_initialized__handshake__protocol]
                fn endpoint_initialized__handshake__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__dc_connection_timeout__peer_address__protocol]
                fn dc_connection_timeout__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__endpoint_udp_immediate_transmission_scheduled__peer__protocol]
                fn endpoint_udp_immediate_transmission_scheduled__peer__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__endpoint_udp_transmission_scheduled__peer__protocol]
                fn endpoint_udp_transmission_scheduled__peer__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__endpoint_udp_transmission_rejected__peer__protocol]
                fn endpoint_udp_transmission_rejected__peer__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__endpoint_udp_packet_transmitted__peer__protocol]
                fn endpoint_udp_packet_transmitted__peer__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__endpoint_udp_transmit_errored__peer__protocol]
                fn endpoint_udp_transmit_errored__peer__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__endpoint_udp_packet_received__peer__protocol]
                fn endpoint_udp_packet_received__peer__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__path_secret_map_background_handshake_requested__peer_address__protocol]
                fn path_secret_map_background_handshake_requested__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__path_secret_map_entry_inserted__peer_address__protocol]
                fn path_secret_map_entry_inserted__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__path_secret_map_entry_ready__peer_address__protocol]
                fn path_secret_map_entry_ready__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__path_secret_map_entry_replaced__peer_address__protocol]
                fn path_secret_map_entry_replaced__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__path_secret_map_id_entry_evicted__peer_address__protocol]
                fn path_secret_map_id_entry_evicted__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__path_secret_map_address_entry_evicted__peer_address__protocol]
                fn path_secret_map_address_entry_evicted__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__unknown_path_secret_packet_sent__peer_address__protocol]
                fn unknown_path_secret_packet_sent__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__unknown_path_secret_packet_received__peer_address__protocol]
                fn unknown_path_secret_packet_received__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__unknown_path_secret_packet_accepted__peer_address__protocol]
                fn unknown_path_secret_packet_accepted__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__unknown_path_secret_packet_rejected__peer_address__protocol]
                fn unknown_path_secret_packet_rejected__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__unknown_path_secret_packet_dropped__peer_address__protocol]
                fn unknown_path_secret_packet_dropped__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__replay_detected_packet_sent__peer_address__protocol]
                fn replay_detected_packet_sent__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__replay_detected_packet_received__peer_address__protocol]
                fn replay_detected_packet_received__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__replay_detected_packet_accepted__peer_address__protocol]
                fn replay_detected_packet_accepted__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__replay_detected_packet_rejected__peer_address__protocol]
                fn replay_detected_packet_rejected__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__replay_detected_packet_dropped__peer_address__protocol]
                fn replay_detected_packet_dropped__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__stale_key_packet_sent__peer_address__protocol]
                fn stale_key_packet_sent__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__stale_key_packet_received__peer_address__protocol]
                fn stale_key_packet_received__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__stale_key_packet_accepted__peer_address__protocol]
                fn stale_key_packet_accepted__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__stale_key_packet_rejected__peer_address__protocol]
                fn stale_key_packet_rejected__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__stale_key_packet_dropped__peer_address__protocol]
                fn stale_key_packet_dropped__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__path_secret_map_address_cache_accessed__peer_address__protocol]
                fn path_secret_map_address_cache_accessed__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__path_secret_map_address_cache_accessed_hit__peer_address__protocol]
                fn path_secret_map_address_cache_accessed_hit__peer_address__protocol(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
            }
        );
    }
}
mod measure {
    #![allow(non_snake_case)]
    use super::*;
    use crate::event::metrics::aggregate::Metric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(crate) fn new(info: &'static Info) -> Self {
            match info.id {
                11usize => Self(endpoint_udp_immediate_transmission_scheduled__buffer_size),
                12usize => Self(endpoint_udp_immediate_transmission_scheduled__segment_size),
                13usize => Self(endpoint_udp_immediate_transmission_scheduled__segment_count),
                16usize => Self(endpoint_udp_transmission_scheduled__buffer_size),
                17usize => Self(endpoint_udp_transmission_scheduled__segment_size),
                18usize => Self(endpoint_udp_transmission_scheduled__segment_count),
                19usize => Self(endpoint_udp_transmission_scheduled__delay),
                22usize => Self(endpoint_udp_transmission_rejected__buffer_size),
                23usize => Self(endpoint_udp_transmission_rejected__segment_size),
                24usize => Self(endpoint_udp_transmission_rejected__segment_count),
                25usize => Self(endpoint_udp_transmission_rejected__delay),
                26usize => Self(endpoint_udp_transmission_rejected__backoff),
                29usize => Self(endpoint_udp_packet_transmitted__buffer_size),
                30usize => Self(endpoint_udp_packet_transmitted__segment_size),
                31usize => Self(endpoint_udp_packet_transmitted__segment_count),
                34usize => Self(endpoint_udp_transmit_errored__buffer_size),
                35usize => Self(endpoint_udp_transmit_errored__segment_size),
                36usize => Self(endpoint_udp_transmit_errored__segment_count),
                39usize => Self(endpoint_udp_packet_received__buffer_size),
                40usize => Self(endpoint_udp_packet_received__segment_size),
                41usize => Self(endpoint_udp_packet_received__segment_count),
                44usize => Self(path_secret_map_initialized__capacity),
                46usize => Self(path_secret_map_uninitialized__capacity),
                47usize => Self(path_secret_map_uninitialized__entries),
                48usize => Self(path_secret_map_uninitialized__lifetime),
                59usize => Self(path_secret_map_id_entry_evicted__age),
                62usize => Self(path_secret_map_address_entry_evicted__age),
                74usize => Self(key_accepted__gap),
                75usize => Self(key_accepted__forward_shift),
                78usize => Self(replay_potentially_detected__gap),
                104usize => Self(path_secret_map_address_cache_accessed_hit__age),
                108usize => Self(path_secret_map_id_cache_accessed_hit__age),
                110usize => Self(path_secret_map_cleaner_cycled__entries__id),
                111usize => Self(path_secret_map_cleaner_cycled__entries__id__retired),
                112usize => Self(path_secret_map_cleaner_cycled__entries__id__active),
                113usize => Self(path_secret_map_cleaner_cycled__entries__id__active__utilization),
                114usize => Self(path_secret_map_cleaner_cycled__entries__id__utilization),
                115usize => Self(path_secret_map_cleaner_cycled__entries__id__utilization__initial),
                116usize => Self(path_secret_map_cleaner_cycled__entries__address),
                117usize => Self(path_secret_map_cleaner_cycled__entries__address__active),
                118usize => {
                    Self(path_secret_map_cleaner_cycled__entries__address__active__utilization)
                }
                119usize => Self(path_secret_map_cleaner_cycled__entries__address__retired),
                120usize => Self(path_secret_map_cleaner_cycled__entries__address__utilization),
                121usize => {
                    Self(path_secret_map_cleaner_cycled__entries__address__utilization__initial)
                }
                122usize => Self(path_secret_map_cleaner_cycled__handshake_requests),
                123usize => Self(path_secret_map_cleaner_cycled__handshake_requests__skipped),
                124usize => Self(path_secret_map_cleaner_cycled__handshake_lock_duration),
                125usize => Self(path_secret_map_cleaner_cycled__total_duration),
                127usize => Self(path_secret_map_id_write_lock__acquire),
                128usize => Self(path_secret_map_id_write_lock__duration),
                130usize => Self(path_secret_map_address_write_lock__acquire),
                131usize => Self(path_secret_map_address_write_lock__duration),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl MetricRecorder for Recorder {
        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
            (self.0)(value.as_u64());
        }
    }
    define!(
        extern "probe" {
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_immediate_transmission_scheduled__buffer_size]
            fn endpoint_udp_immediate_transmission_scheduled__buffer_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_immediate_transmission_scheduled__segment_size]
            fn endpoint_udp_immediate_transmission_scheduled__segment_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_immediate_transmission_scheduled__segment_count]
            fn endpoint_udp_immediate_transmission_scheduled__segment_count(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmission_scheduled__buffer_size]
            fn endpoint_udp_transmission_scheduled__buffer_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmission_scheduled__segment_size]
            fn endpoint_udp_transmission_scheduled__segment_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmission_scheduled__segment_count]
            fn endpoint_udp_transmission_scheduled__segment_count(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmission_scheduled__delay]
            fn endpoint_udp_transmission_scheduled__delay(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmission_rejected__buffer_size]
            fn endpoint_udp_transmission_rejected__buffer_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmission_rejected__segment_size]
            fn endpoint_udp_transmission_rejected__segment_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmission_rejected__segment_count]
            fn endpoint_udp_transmission_rejected__segment_count(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmission_rejected__delay]
            fn endpoint_udp_transmission_rejected__delay(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmission_rejected__backoff]
            fn endpoint_udp_transmission_rejected__backoff(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_packet_transmitted__buffer_size]
            fn endpoint_udp_packet_transmitted__buffer_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_packet_transmitted__segment_size]
            fn endpoint_udp_packet_transmitted__segment_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_packet_transmitted__segment_count]
            fn endpoint_udp_packet_transmitted__segment_count(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmit_errored__buffer_size]
            fn endpoint_udp_transmit_errored__buffer_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmit_errored__segment_size]
            fn endpoint_udp_transmit_errored__segment_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_transmit_errored__segment_count]
            fn endpoint_udp_transmit_errored__segment_count(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_packet_received__buffer_size]
            fn endpoint_udp_packet_received__buffer_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_packet_received__segment_size]
            fn endpoint_udp_packet_received__segment_size(value: u64);
            # [link_name = s2n_quic_dc__event__measure__endpoint_udp_packet_received__segment_count]
            fn endpoint_udp_packet_received__segment_count(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_initialized__capacity]
            fn path_secret_map_initialized__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_uninitialized__capacity]
            fn path_secret_map_uninitialized__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_uninitialized__entries]
            fn path_secret_map_uninitialized__entries(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_uninitialized__lifetime]
            fn path_secret_map_uninitialized__lifetime(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_id_entry_evicted__age]
            fn path_secret_map_id_entry_evicted__age(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_address_entry_evicted__age]
            fn path_secret_map_address_entry_evicted__age(value: u64);
            # [link_name = s2n_quic_dc__event__measure__key_accepted__gap]
            fn key_accepted__gap(value: u64);
            # [link_name = s2n_quic_dc__event__measure__key_accepted__forward_shift]
            fn key_accepted__forward_shift(value: u64);
            # [link_name = s2n_quic_dc__event__measure__replay_potentially_detected__gap]
            fn replay_potentially_detected__gap(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_address_cache_accessed_hit__age]
            fn path_secret_map_address_cache_accessed_hit__age(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_id_cache_accessed_hit__age]
            fn path_secret_map_id_cache_accessed_hit__age(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id]
            fn path_secret_map_cleaner_cycled__entries__id(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id__retired]
            fn path_secret_map_cleaner_cycled__entries__id__retired(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id__active]
            fn path_secret_map_cleaner_cycled__entries__id__active(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id__active__utilization]
            fn path_secret_map_cleaner_cycled__entries__id__active__utilization(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id__utilization]
            fn path_secret_map_cleaner_cycled__entries__id__utilization(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id__utilization__initial]
            fn path_secret_map_cleaner_cycled__entries__id__utilization__initial(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__address]
            fn path_secret_map_cleaner_cycled__entries__address(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__address__active]
            fn path_secret_map_cleaner_cycled__entries__address__active(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__address__active__utilization]
            fn path_secret_map_cleaner_cycled__entries__address__active__utilization(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__address__retired]
            fn path_secret_map_cleaner_cycled__entries__address__retired(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__address__utilization]
            fn path_secret_map_cleaner_cycled__entries__address__utilization(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__address__utilization__initial]
            fn path_secret_map_cleaner_cycled__entries__address__utilization__initial(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__handshake_requests]
            fn path_secret_map_cleaner_cycled__handshake_requests(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__handshake_requests__skipped]
            fn path_secret_map_cleaner_cycled__handshake_requests__skipped(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__handshake_lock_duration]
            fn path_secret_map_cleaner_cycled__handshake_lock_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__total_duration]
            fn path_secret_map_cleaner_cycled__total_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_id_write_lock__acquire]
            fn path_secret_map_id_write_lock__acquire(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_id_write_lock__duration]
            fn path_secret_map_id_write_lock__duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_address_write_lock__acquire]
            fn path_secret_map_address_write_lock__acquire(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_address_write_lock__duration]
            fn path_secret_map_address_write_lock__duration(value: u64);
        }
    );
}
mod gauge {
    #![allow(non_snake_case)]
    use super::*;
    use crate::event::metrics::aggregate::Metric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(crate) fn new(info: &'static Info) -> Self {
            unreachable!("invalid info: {info:?}")
        }
    }
    impl MetricRecorder for Recorder {
        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
            (self.0)(value.as_u64());
        }
    }
}
mod timer {
    #![allow(non_snake_case)]
    use super::*;
    use crate::event::metrics::aggregate::Metric;
    pub struct Recorder(fn(core::time::Duration));
    impl Recorder {
        pub(crate) fn new(info: &'static Info) -> Self {
            unreachable!("invalid info: {info:?}")
        }
    }
    impl MetricRecorder for Recorder {
        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
            (self.0)(value.as_duration());
        }
    }
    pub mod nominal {
        #![allow(non_snake_case)]
        use super::*;
        use crate::event::metrics::aggregate::Metric;
        pub struct Recorder(fn(core::time::Duration, u64, &info::Str));
        impl Recorder {
            pub(crate) fn new(info: &'static Info, _variant: &'static info::Variant) -> Self {
                unreachable!("invalid info: {info:?}")
            }
        }
        impl NominalRecorder for Recorder {
            fn record<T: Metric>(
                &self,
                _info: &'static Info,
                variant: &'static info::Variant,
                value: T,
            ) {
                (self.0)(value.as_duration(), variant.id as _, variant.name);
            }
        }
    }
}
#[derive(Default)]
pub struct Registry(());
impl aggregate::Registry for Registry {
    type Counter = counter::Recorder;
    type BoolCounter = counter::bool::Recorder;
    type NominalCounter = counter::nominal::Recorder;
    type Measure = measure::Recorder;
    type Gauge = gauge::Recorder;
    type Timer = timer::Recorder;
    type NominalTimer = timer::nominal::Recorder;
    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        counter::Recorder::new(info)
    }
    #[inline]
    fn register_bool_counter(&self, info: &'static Info) -> Self::BoolCounter {
        counter::bool::Recorder::new(info)
    }
    #[inline]
    fn register_nominal_counter(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalCounter {
        counter::nominal::Recorder::new(info, variant)
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
    #[inline]
    fn register_nominal_timer(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalTimer {
        timer::nominal::Recorder::new(info, variant)
    }
}
