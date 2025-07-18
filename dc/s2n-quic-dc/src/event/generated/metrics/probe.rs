// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
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
                0usize => Self(acceptor_tcp_started),
                1usize => Self(acceptor_tcp_loop_iteration_completed),
                7usize => Self(acceptor_tcp_fresh_enqueued),
                8usize => Self(acceptor_tcp_fresh_batch_completed),
                12usize => Self(acceptor_tcp_stream_dropped),
                14usize => Self(acceptor_tcp_stream_replaced),
                17usize => Self(acceptor_tcp_packet_received),
                22usize => Self(acceptor_tcp_packet_dropped),
                25usize => Self(acceptor_tcp_stream_enqueued),
                28usize => Self(acceptor_tcp_io_error),
                29usize => Self(acceptor_udp_started),
                30usize => Self(acceptor_udp_datagram_received),
                32usize => Self(acceptor_udp_packet_received),
                38usize => Self(acceptor_udp_packet_dropped),
                40usize => Self(acceptor_udp_stream_enqueued),
                41usize => Self(acceptor_udp_io_error),
                42usize => Self(acceptor_stream_pruned),
                45usize => Self(acceptor_stream_dequeued),
                47usize => Self(stream_write_flushed),
                51usize => Self(stream_write_flushed__committed__total),
                56usize => Self(stream_write_fin_flushed),
                60usize => Self(stream_write_fin_flushed__committed__total),
                65usize => Self(stream_write_blocked),
                71usize => Self(stream_write_errored),
                76usize => Self(stream_write_key_updated),
                77usize => Self(stream_write_shutdown),
                81usize => Self(stream_write_socket_flushed),
                84usize => Self(stream_write_socket_flushed__committed__total),
                87usize => Self(stream_write_socket_blocked),
                90usize => Self(stream_write_socket_errored),
                92usize => Self(stream_read_flushed),
                96usize => Self(stream_read_flushed__committed__total),
                101usize => Self(stream_read_fin_flushed),
                107usize => Self(stream_read_blocked),
                112usize => Self(stream_read_errored),
                117usize => Self(stream_read_key_updated),
                118usize => Self(stream_read_shutdown),
                121usize => Self(stream_read_socket_flushed),
                124usize => Self(stream_read_socket_flushed__committed__total),
                127usize => Self(stream_read_socket_blocked),
                130usize => Self(stream_read_socket_errored),
                132usize => Self(stream_tcp_connect),
                135usize => Self(stream_connect),
                139usize => Self(stream_connect_error),
                141usize => Self(connection_closed),
                142usize => Self(endpoint_initialized),
                147usize => Self(path_secret_map_initialized),
                149usize => Self(path_secret_map_uninitialized),
                153usize => Self(path_secret_map_background_handshake_requested),
                155usize => Self(path_secret_map_entry_inserted),
                157usize => Self(path_secret_map_entry_ready),
                159usize => Self(path_secret_map_entry_replaced),
                161usize => Self(path_secret_map_id_entry_evicted),
                164usize => Self(path_secret_map_address_entry_evicted),
                167usize => Self(unknown_path_secret_packet_sent),
                169usize => Self(unknown_path_secret_packet_received),
                171usize => Self(unknown_path_secret_packet_accepted),
                173usize => Self(unknown_path_secret_packet_rejected),
                175usize => Self(unknown_path_secret_packet_dropped),
                177usize => Self(key_accepted),
                180usize => Self(replay_definitely_detected),
                181usize => Self(replay_potentially_detected),
                183usize => Self(replay_detected_packet_sent),
                185usize => Self(replay_detected_packet_received),
                187usize => Self(replay_detected_packet_accepted),
                189usize => Self(replay_detected_packet_rejected),
                191usize => Self(replay_detected_packet_dropped),
                193usize => Self(stale_key_packet_sent),
                195usize => Self(stale_key_packet_received),
                197usize => Self(stale_key_packet_accepted),
                199usize => Self(stale_key_packet_rejected),
                201usize => Self(stale_key_packet_dropped),
                203usize => Self(path_secret_map_address_cache_accessed),
                206usize => Self(path_secret_map_address_cache_accessed_hit),
                209usize => Self(path_secret_map_id_cache_accessed),
                211usize => Self(path_secret_map_id_cache_accessed_hit),
                213usize => Self(path_secret_map_cleaner_cycled),
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
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_started]
            fn acceptor_tcp_started(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_loop_iteration_completed]
            fn acceptor_tcp_loop_iteration_completed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_fresh_enqueued]
            fn acceptor_tcp_fresh_enqueued(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_fresh_batch_completed]
            fn acceptor_tcp_fresh_batch_completed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_stream_dropped]
            fn acceptor_tcp_stream_dropped(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_stream_replaced]
            fn acceptor_tcp_stream_replaced(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_packet_received]
            fn acceptor_tcp_packet_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_packet_dropped]
            fn acceptor_tcp_packet_dropped(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_stream_enqueued]
            fn acceptor_tcp_stream_enqueued(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_io_error]
            fn acceptor_tcp_io_error(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_udp_started]
            fn acceptor_udp_started(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_udp_datagram_received]
            fn acceptor_udp_datagram_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_udp_packet_received]
            fn acceptor_udp_packet_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_udp_packet_dropped]
            fn acceptor_udp_packet_dropped(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_udp_stream_enqueued]
            fn acceptor_udp_stream_enqueued(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_udp_io_error]
            fn acceptor_udp_io_error(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_stream_pruned]
            fn acceptor_stream_pruned(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_stream_dequeued]
            fn acceptor_stream_dequeued(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_flushed]
            fn stream_write_flushed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_flushed__committed__total]
            fn stream_write_flushed__committed__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_fin_flushed]
            fn stream_write_fin_flushed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_fin_flushed__committed__total]
            fn stream_write_fin_flushed__committed__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_blocked]
            fn stream_write_blocked(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_errored]
            fn stream_write_errored(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_key_updated]
            fn stream_write_key_updated(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_shutdown]
            fn stream_write_shutdown(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_socket_flushed]
            fn stream_write_socket_flushed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_socket_flushed__committed__total]
            fn stream_write_socket_flushed__committed__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_socket_blocked]
            fn stream_write_socket_blocked(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_write_socket_errored]
            fn stream_write_socket_errored(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_flushed]
            fn stream_read_flushed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_flushed__committed__total]
            fn stream_read_flushed__committed__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_fin_flushed]
            fn stream_read_fin_flushed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_blocked]
            fn stream_read_blocked(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_errored]
            fn stream_read_errored(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_key_updated]
            fn stream_read_key_updated(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_shutdown]
            fn stream_read_shutdown(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_socket_flushed]
            fn stream_read_socket_flushed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_socket_flushed__committed__total]
            fn stream_read_socket_flushed__committed__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_socket_blocked]
            fn stream_read_socket_blocked(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_read_socket_errored]
            fn stream_read_socket_errored(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_tcp_connect]
            fn stream_tcp_connect(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_connect]
            fn stream_connect(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_connect_error]
            fn stream_connect_error(value: u64);
            # [link_name = s2n_quic_dc__event__counter__connection_closed]
            fn connection_closed(value: u64);
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
        }
    );
    pub mod bool {
        #![allow(non_snake_case)]
        use super::*;
        pub struct Recorder(fn(bool));
        impl Recorder {
            pub(crate) fn new(info: &'static Info) -> Self {
                match info.id {
                    19usize => Self(acceptor_tcp_packet_received__is_fin),
                    20usize => Self(acceptor_tcp_packet_received__is_fin_known),
                    34usize => Self(acceptor_udp_packet_received__is_zero_offset),
                    35usize => Self(acceptor_udp_packet_received__is_retransmisson),
                    36usize => Self(acceptor_udp_packet_received__is_fin),
                    37usize => Self(acceptor_udp_packet_received__is_fin_known),
                    80usize => Self(stream_write_shutdown__background),
                    120usize => Self(stream_read_shutdown__background),
                    133usize => Self(stream_tcp_connect__error),
                    136usize => Self(stream_connect__error),
                    145usize => Self(endpoint_initialized__tcp),
                    146usize => Self(endpoint_initialized__udp),
                    205usize => Self(path_secret_map_address_cache_accessed__hit),
                    210usize => Self(path_secret_map_id_cache_accessed__hit),
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
                # [link_name = s2n_quic_dc__event__counter__bool__acceptor_tcp_packet_received__is_fin]
                fn acceptor_tcp_packet_received__is_fin(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__acceptor_tcp_packet_received__is_fin_known]
                fn acceptor_tcp_packet_received__is_fin_known(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__acceptor_udp_packet_received__is_zero_offset]
                fn acceptor_udp_packet_received__is_zero_offset(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__acceptor_udp_packet_received__is_retransmisson]
                fn acceptor_udp_packet_received__is_retransmisson(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__acceptor_udp_packet_received__is_fin]
                fn acceptor_udp_packet_received__is_fin(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__acceptor_udp_packet_received__is_fin_known]
                fn acceptor_udp_packet_received__is_fin_known(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_write_shutdown__background]
                fn stream_write_shutdown__background(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_read_shutdown__background]
                fn stream_read_shutdown__background(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_tcp_connect__error]
                fn stream_tcp_connect__error(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_connect__error]
                fn stream_connect__error(value: bool);
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
                    13usize => Self(acceptor_tcp_stream_dropped__reason),
                    23usize => Self(acceptor_tcp_packet_dropped__reason),
                    39usize => Self(acceptor_udp_packet_dropped__reason),
                    44usize => Self(acceptor_stream_pruned__reason),
                    137usize => Self(stream_connect__tcp),
                    138usize => Self(stream_connect__handshake),
                    140usize => Self(stream_connect_error__reason),
                    143usize => Self(endpoint_initialized__acceptor__protocol),
                    144usize => Self(endpoint_initialized__handshake__protocol),
                    154usize => {
                        Self(path_secret_map_background_handshake_requested__peer_address__protocol)
                    }
                    156usize => Self(path_secret_map_entry_inserted__peer_address__protocol),
                    158usize => Self(path_secret_map_entry_ready__peer_address__protocol),
                    160usize => Self(path_secret_map_entry_replaced__peer_address__protocol),
                    162usize => Self(path_secret_map_id_entry_evicted__peer_address__protocol),
                    165usize => Self(path_secret_map_address_entry_evicted__peer_address__protocol),
                    168usize => Self(unknown_path_secret_packet_sent__peer_address__protocol),
                    170usize => Self(unknown_path_secret_packet_received__peer_address__protocol),
                    172usize => Self(unknown_path_secret_packet_accepted__peer_address__protocol),
                    174usize => Self(unknown_path_secret_packet_rejected__peer_address__protocol),
                    176usize => Self(unknown_path_secret_packet_dropped__peer_address__protocol),
                    184usize => Self(replay_detected_packet_sent__peer_address__protocol),
                    186usize => Self(replay_detected_packet_received__peer_address__protocol),
                    188usize => Self(replay_detected_packet_accepted__peer_address__protocol),
                    190usize => Self(replay_detected_packet_rejected__peer_address__protocol),
                    192usize => Self(replay_detected_packet_dropped__peer_address__protocol),
                    194usize => Self(stale_key_packet_sent__peer_address__protocol),
                    196usize => Self(stale_key_packet_received__peer_address__protocol),
                    198usize => Self(stale_key_packet_accepted__peer_address__protocol),
                    200usize => Self(stale_key_packet_rejected__peer_address__protocol),
                    202usize => Self(stale_key_packet_dropped__peer_address__protocol),
                    204usize => {
                        Self(path_secret_map_address_cache_accessed__peer_address__protocol)
                    }
                    207usize => {
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
                # [link_name = s2n_quic_dc__event__counter__nominal__acceptor_tcp_stream_dropped__reason]
                fn acceptor_tcp_stream_dropped__reason(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__acceptor_tcp_packet_dropped__reason]
                fn acceptor_tcp_packet_dropped__reason(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__acceptor_udp_packet_dropped__reason]
                fn acceptor_udp_packet_dropped__reason(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__acceptor_stream_pruned__reason]
                fn acceptor_stream_pruned__reason(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic_dc__event__counter__nominal__stream_connect__tcp]
                fn stream_connect__tcp(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic_dc__event__counter__nominal__stream_connect__handshake]
                fn stream_connect__handshake(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic_dc__event__counter__nominal__stream_connect_error__reason]
                fn stream_connect_error__reason(value: u64, variant: u64, variant_name: &info::Str);
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
                2usize => Self(acceptor_tcp_loop_iteration_completed__pending_streams),
                3usize => Self(acceptor_tcp_loop_iteration_completed__slots_idle),
                4usize => Self(acceptor_tcp_loop_iteration_completed__slot_utilization),
                6usize => Self(acceptor_tcp_loop_iteration_completed__max_sojourn_time),
                9usize => Self(acceptor_tcp_fresh_batch_completed__enqueued),
                10usize => Self(acceptor_tcp_fresh_batch_completed__dropped),
                11usize => Self(acceptor_tcp_fresh_batch_completed__errored),
                16usize => Self(acceptor_tcp_stream_replaced__buffer_len),
                18usize => Self(acceptor_tcp_packet_received__payload_len),
                27usize => Self(acceptor_tcp_stream_enqueued__blocked_count),
                31usize => Self(acceptor_udp_datagram_received__len),
                33usize => Self(acceptor_udp_packet_received__payload_len),
                49usize => Self(stream_write_flushed__conn),
                50usize => Self(stream_write_flushed__provided),
                52usize => Self(stream_write_flushed__committed),
                53usize => Self(stream_write_flushed__committed__conn),
                54usize => Self(stream_write_flushed__processing_duration),
                55usize => Self(stream_write_flushed__processing_duration__conn),
                58usize => Self(stream_write_fin_flushed__conn),
                59usize => Self(stream_write_fin_flushed__provided),
                61usize => Self(stream_write_fin_flushed__committed),
                62usize => Self(stream_write_fin_flushed__committed__conn),
                63usize => Self(stream_write_fin_flushed__processing_duration),
                64usize => Self(stream_write_fin_flushed__processing_duration__conn),
                67usize => Self(stream_write_blocked__conn),
                68usize => Self(stream_write_blocked__provided),
                69usize => Self(stream_write_blocked__processing_duration),
                70usize => Self(stream_write_blocked__processing_duration__conn),
                73usize => Self(stream_write_errored__provided),
                74usize => Self(stream_write_errored__processing_duration),
                75usize => Self(stream_write_errored__processing_duration__conn),
                79usize => Self(stream_write_shutdown__buffer_len),
                82usize => Self(stream_write_socket_flushed__conn),
                83usize => Self(stream_write_socket_flushed__provided),
                85usize => Self(stream_write_socket_flushed__committed),
                86usize => Self(stream_write_socket_flushed__committed__conn),
                88usize => Self(stream_write_socket_blocked__conn),
                89usize => Self(stream_write_socket_blocked__provided),
                91usize => Self(stream_write_socket_errored__provided),
                94usize => Self(stream_read_flushed__conn),
                95usize => Self(stream_read_flushed__capacity),
                97usize => Self(stream_read_flushed__committed),
                98usize => Self(stream_read_flushed__committed__conn),
                99usize => Self(stream_read_flushed__processing_duration),
                100usize => Self(stream_read_flushed__processing_duration__conn),
                103usize => Self(stream_read_fin_flushed__conn),
                104usize => Self(stream_read_fin_flushed__capacity),
                105usize => Self(stream_read_fin_flushed__processing_duration),
                106usize => Self(stream_read_fin_flushed__processing_duration__conn),
                109usize => Self(stream_read_blocked__capacity),
                110usize => Self(stream_read_blocked__processing_duration),
                111usize => Self(stream_read_blocked__processing_duration__conn),
                114usize => Self(stream_read_errored__capacity),
                115usize => Self(stream_read_errored__processing_duration),
                116usize => Self(stream_read_errored__processing_duration__conn),
                122usize => Self(stream_read_socket_flushed__conn),
                123usize => Self(stream_read_socket_flushed__capacity),
                125usize => Self(stream_read_socket_flushed__committed),
                126usize => Self(stream_read_socket_flushed__committed__conn),
                128usize => Self(stream_read_socket_blocked__conn),
                129usize => Self(stream_read_socket_blocked__capacity),
                131usize => Self(stream_read_socket_errored__capacity),
                148usize => Self(path_secret_map_initialized__capacity),
                150usize => Self(path_secret_map_uninitialized__capacity),
                151usize => Self(path_secret_map_uninitialized__entries),
                152usize => Self(path_secret_map_uninitialized__lifetime),
                163usize => Self(path_secret_map_id_entry_evicted__age),
                166usize => Self(path_secret_map_address_entry_evicted__age),
                178usize => Self(key_accepted__gap),
                179usize => Self(key_accepted__forward_shift),
                182usize => Self(replay_potentially_detected__gap),
                208usize => Self(path_secret_map_address_cache_accessed_hit__age),
                212usize => Self(path_secret_map_id_cache_accessed_hit__age),
                214usize => Self(path_secret_map_cleaner_cycled__entries__id),
                215usize => Self(path_secret_map_cleaner_cycled__entries__id__retired),
                216usize => Self(path_secret_map_cleaner_cycled__entries__id__active),
                217usize => Self(path_secret_map_cleaner_cycled__entries__id__active__utilization),
                218usize => Self(path_secret_map_cleaner_cycled__entries__id__utilization),
                219usize => Self(path_secret_map_cleaner_cycled__entries__id__utilization__initial),
                220usize => Self(path_secret_map_cleaner_cycled__entries__address),
                221usize => Self(path_secret_map_cleaner_cycled__entries__address__active),
                222usize => {
                    Self(path_secret_map_cleaner_cycled__entries__address__active__utilization)
                }
                223usize => Self(path_secret_map_cleaner_cycled__entries__address__retired),
                224usize => Self(path_secret_map_cleaner_cycled__entries__address__utilization),
                225usize => {
                    Self(path_secret_map_cleaner_cycled__entries__address__utilization__initial)
                }
                226usize => Self(path_secret_map_cleaner_cycled__handshake_requests),
                227usize => Self(path_secret_map_cleaner_cycled__handshake_requests__retired),
                228usize => Self(path_secret_map_cleaner_cycled__handshake_lock_duration),
                229usize => Self(path_secret_map_cleaner_cycled__total_duration),
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
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_loop_iteration_completed__pending_streams]
            fn acceptor_tcp_loop_iteration_completed__pending_streams(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_loop_iteration_completed__slots_idle]
            fn acceptor_tcp_loop_iteration_completed__slots_idle(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_loop_iteration_completed__slot_utilization]
            fn acceptor_tcp_loop_iteration_completed__slot_utilization(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_loop_iteration_completed__max_sojourn_time]
            fn acceptor_tcp_loop_iteration_completed__max_sojourn_time(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_fresh_batch_completed__enqueued]
            fn acceptor_tcp_fresh_batch_completed__enqueued(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_fresh_batch_completed__dropped]
            fn acceptor_tcp_fresh_batch_completed__dropped(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_fresh_batch_completed__errored]
            fn acceptor_tcp_fresh_batch_completed__errored(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_stream_replaced__buffer_len]
            fn acceptor_tcp_stream_replaced__buffer_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_packet_received__payload_len]
            fn acceptor_tcp_packet_received__payload_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_stream_enqueued__blocked_count]
            fn acceptor_tcp_stream_enqueued__blocked_count(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_udp_datagram_received__len]
            fn acceptor_udp_datagram_received__len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_udp_packet_received__payload_len]
            fn acceptor_udp_packet_received__payload_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_flushed__conn]
            fn stream_write_flushed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_flushed__provided]
            fn stream_write_flushed__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_flushed__committed]
            fn stream_write_flushed__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_flushed__committed__conn]
            fn stream_write_flushed__committed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_flushed__processing_duration]
            fn stream_write_flushed__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_flushed__processing_duration__conn]
            fn stream_write_flushed__processing_duration__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_fin_flushed__conn]
            fn stream_write_fin_flushed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_fin_flushed__provided]
            fn stream_write_fin_flushed__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_fin_flushed__committed]
            fn stream_write_fin_flushed__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_fin_flushed__committed__conn]
            fn stream_write_fin_flushed__committed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_fin_flushed__processing_duration]
            fn stream_write_fin_flushed__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_fin_flushed__processing_duration__conn]
            fn stream_write_fin_flushed__processing_duration__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_blocked__conn]
            fn stream_write_blocked__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_blocked__provided]
            fn stream_write_blocked__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_blocked__processing_duration]
            fn stream_write_blocked__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_blocked__processing_duration__conn]
            fn stream_write_blocked__processing_duration__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_errored__provided]
            fn stream_write_errored__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_errored__processing_duration]
            fn stream_write_errored__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_errored__processing_duration__conn]
            fn stream_write_errored__processing_duration__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_shutdown__buffer_len]
            fn stream_write_shutdown__buffer_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_flushed__conn]
            fn stream_write_socket_flushed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_flushed__provided]
            fn stream_write_socket_flushed__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_flushed__committed]
            fn stream_write_socket_flushed__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_flushed__committed__conn]
            fn stream_write_socket_flushed__committed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_blocked__conn]
            fn stream_write_socket_blocked__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_blocked__provided]
            fn stream_write_socket_blocked__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_errored__provided]
            fn stream_write_socket_errored__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_flushed__conn]
            fn stream_read_flushed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_flushed__capacity]
            fn stream_read_flushed__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_flushed__committed]
            fn stream_read_flushed__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_flushed__committed__conn]
            fn stream_read_flushed__committed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_flushed__processing_duration]
            fn stream_read_flushed__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_flushed__processing_duration__conn]
            fn stream_read_flushed__processing_duration__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_fin_flushed__conn]
            fn stream_read_fin_flushed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_fin_flushed__capacity]
            fn stream_read_fin_flushed__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_fin_flushed__processing_duration]
            fn stream_read_fin_flushed__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_fin_flushed__processing_duration__conn]
            fn stream_read_fin_flushed__processing_duration__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_blocked__capacity]
            fn stream_read_blocked__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_blocked__processing_duration]
            fn stream_read_blocked__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_blocked__processing_duration__conn]
            fn stream_read_blocked__processing_duration__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_errored__capacity]
            fn stream_read_errored__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_errored__processing_duration]
            fn stream_read_errored__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_errored__processing_duration__conn]
            fn stream_read_errored__processing_duration__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_socket_flushed__conn]
            fn stream_read_socket_flushed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_socket_flushed__capacity]
            fn stream_read_socket_flushed__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_socket_flushed__committed]
            fn stream_read_socket_flushed__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_socket_flushed__committed__conn]
            fn stream_read_socket_flushed__committed__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_socket_blocked__conn]
            fn stream_read_socket_blocked__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_socket_blocked__capacity]
            fn stream_read_socket_blocked__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_socket_errored__capacity]
            fn stream_read_socket_errored__capacity(value: u64);
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
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__handshake_requests__retired]
            fn path_secret_map_cleaner_cycled__handshake_requests__retired(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__handshake_lock_duration]
            fn path_secret_map_cleaner_cycled__handshake_lock_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__total_duration]
            fn path_secret_map_cleaner_cycled__total_duration(value: u64);
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
            match info.id {
                5usize => Self(acceptor_tcp_loop_iteration_completed__processing_duration),
                15usize => Self(acceptor_tcp_stream_replaced__sojourn_time),
                21usize => Self(acceptor_tcp_packet_received__sojourn_time),
                24usize => Self(acceptor_tcp_packet_dropped__sojourn_time),
                26usize => Self(acceptor_tcp_stream_enqueued__sojourn_time),
                43usize => Self(acceptor_stream_pruned__sojourn_time),
                46usize => Self(acceptor_stream_dequeued__sojourn_time),
                48usize => Self(stream_write_flushed__latency),
                57usize => Self(stream_write_fin_flushed__latency),
                66usize => Self(stream_write_blocked__latency),
                72usize => Self(stream_write_errored__latency),
                78usize => Self(stream_write_shutdown__latency),
                93usize => Self(stream_read_flushed__latency),
                102usize => Self(stream_read_fin_flushed__latency),
                108usize => Self(stream_read_blocked__latency),
                113usize => Self(stream_read_errored__latency),
                119usize => Self(stream_read_shutdown__latency),
                134usize => Self(stream_tcp_connect__tcp_latency),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl MetricRecorder for Recorder {
        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
            (self.0)(value.as_duration());
        }
    }
    define!(
        extern "probe" {
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_loop_iteration_completed__processing_duration]
            fn acceptor_tcp_loop_iteration_completed__processing_duration(
                value: core::time::Duration,
            );
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_stream_replaced__sojourn_time]
            fn acceptor_tcp_stream_replaced__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_packet_received__sojourn_time]
            fn acceptor_tcp_packet_received__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_packet_dropped__sojourn_time]
            fn acceptor_tcp_packet_dropped__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_stream_enqueued__sojourn_time]
            fn acceptor_tcp_stream_enqueued__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_stream_pruned__sojourn_time]
            fn acceptor_stream_pruned__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_stream_dequeued__sojourn_time]
            fn acceptor_stream_dequeued__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_write_flushed__latency]
            fn stream_write_flushed__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_write_fin_flushed__latency]
            fn stream_write_fin_flushed__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_write_blocked__latency]
            fn stream_write_blocked__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_write_errored__latency]
            fn stream_write_errored__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_write_shutdown__latency]
            fn stream_write_shutdown__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_read_flushed__latency]
            fn stream_read_flushed__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_read_fin_flushed__latency]
            fn stream_read_fin_flushed__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_read_blocked__latency]
            fn stream_read_blocked__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_read_errored__latency]
            fn stream_read_errored__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_read_shutdown__latency]
            fn stream_read_shutdown__latency(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__stream_tcp_connect__tcp_latency]
            fn stream_tcp_connect__tcp_latency(value: core::time::Duration);
        }
    );
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
