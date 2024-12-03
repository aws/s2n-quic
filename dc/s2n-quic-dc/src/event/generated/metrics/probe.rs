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
                50usize => Self(stream_write_flushed__committed__total),
                53usize => Self(stream_write_fin_flushed),
                56usize => Self(stream_write_fin_flushed__committed__total),
                59usize => Self(stream_write_blocked),
                63usize => Self(stream_write_errored),
                67usize => Self(stream_write_shutdown),
                71usize => Self(stream_write_socket_flushed),
                73usize => Self(stream_write_socket_flushed__committed__total),
                75usize => Self(stream_write_socket_blocked),
                77usize => Self(stream_write_socket_errored),
                79usize => Self(stream_read_flushed),
                82usize => Self(stream_read_flushed__committed__total),
                85usize => Self(stream_read_fin_flushed),
                89usize => Self(stream_read_blocked),
                93usize => Self(stream_read_errored),
                97usize => Self(stream_read_shutdown),
                100usize => Self(stream_read_socket_flushed),
                102usize => Self(stream_read_socket_flushed__committed__total),
                104usize => Self(stream_read_socket_blocked),
                106usize => Self(stream_read_socket_errored),
                108usize => Self(endpoint_initialized),
                113usize => Self(path_secret_map_initialized),
                115usize => Self(path_secret_map_uninitialized),
                119usize => Self(path_secret_map_background_handshake_requested),
                121usize => Self(path_secret_map_entry_inserted),
                123usize => Self(path_secret_map_entry_ready),
                125usize => Self(path_secret_map_entry_replaced),
                127usize => Self(unknown_path_secret_packet_sent),
                129usize => Self(unknown_path_secret_packet_received),
                131usize => Self(unknown_path_secret_packet_accepted),
                133usize => Self(unknown_path_secret_packet_rejected),
                135usize => Self(unknown_path_secret_packet_dropped),
                137usize => Self(key_accepted),
                140usize => Self(replay_definitely_detected),
                141usize => Self(replay_potentially_detected),
                143usize => Self(replay_detected_packet_sent),
                145usize => Self(replay_detected_packet_received),
                147usize => Self(replay_detected_packet_accepted),
                149usize => Self(replay_detected_packet_rejected),
                151usize => Self(replay_detected_packet_dropped),
                153usize => Self(stale_key_packet_sent),
                155usize => Self(stale_key_packet_received),
                157usize => Self(stale_key_packet_accepted),
                159usize => Self(stale_key_packet_rejected),
                161usize => Self(stale_key_packet_dropped),
                163usize => Self(path_secret_map_address_cache_accessed),
                166usize => Self(path_secret_map_id_cache_accessed),
                168usize => Self(path_secret_map_cleaner_cycled),
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
            # [link_name = s2n_quic_dc__event__counter__path_secret_map_id_cache_accessed]
            fn path_secret_map_id_cache_accessed(value: u64);
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
                    70usize => Self(stream_write_shutdown__background),
                    99usize => Self(stream_read_shutdown__background),
                    111usize => Self(endpoint_initialized__tcp),
                    112usize => Self(endpoint_initialized__udp),
                    165usize => Self(path_secret_map_address_cache_accessed__hit),
                    167usize => Self(path_secret_map_id_cache_accessed__hit),
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
                    109usize => Self(endpoint_initialized__acceptor__protocol),
                    110usize => Self(endpoint_initialized__handshake__protocol),
                    120usize => {
                        Self(path_secret_map_background_handshake_requested__peer_address__protocol)
                    }
                    122usize => Self(path_secret_map_entry_inserted__peer_address__protocol),
                    124usize => Self(path_secret_map_entry_ready__peer_address__protocol),
                    126usize => Self(path_secret_map_entry_replaced__peer_address__protocol),
                    128usize => Self(unknown_path_secret_packet_sent__peer_address__protocol),
                    130usize => Self(unknown_path_secret_packet_received__peer_address__protocol),
                    132usize => Self(unknown_path_secret_packet_accepted__peer_address__protocol),
                    134usize => Self(unknown_path_secret_packet_rejected__peer_address__protocol),
                    136usize => Self(unknown_path_secret_packet_dropped__peer_address__protocol),
                    144usize => Self(replay_detected_packet_sent__peer_address__protocol),
                    146usize => Self(replay_detected_packet_received__peer_address__protocol),
                    148usize => Self(replay_detected_packet_accepted__peer_address__protocol),
                    150usize => Self(replay_detected_packet_rejected__peer_address__protocol),
                    152usize => Self(replay_detected_packet_dropped__peer_address__protocol),
                    154usize => Self(stale_key_packet_sent__peer_address__protocol),
                    156usize => Self(stale_key_packet_received__peer_address__protocol),
                    158usize => Self(stale_key_packet_accepted__peer_address__protocol),
                    160usize => Self(stale_key_packet_rejected__peer_address__protocol),
                    162usize => Self(stale_key_packet_dropped__peer_address__protocol),
                    164usize => {
                        Self(path_secret_map_address_cache_accessed__peer_address__protocol)
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
                49usize => Self(stream_write_flushed__provided),
                51usize => Self(stream_write_flushed__committed),
                52usize => Self(stream_write_flushed__processing_duration),
                55usize => Self(stream_write_fin_flushed__provided),
                57usize => Self(stream_write_fin_flushed__committed),
                58usize => Self(stream_write_fin_flushed__processing_duration),
                61usize => Self(stream_write_blocked__provided),
                62usize => Self(stream_write_blocked__processing_duration),
                65usize => Self(stream_write_errored__provided),
                66usize => Self(stream_write_errored__processing_duration),
                69usize => Self(stream_write_shutdown__buffer_len),
                72usize => Self(stream_write_socket_flushed__provided),
                74usize => Self(stream_write_socket_flushed__committed),
                76usize => Self(stream_write_socket_blocked__provided),
                78usize => Self(stream_write_socket_errored__provided),
                81usize => Self(stream_read_flushed__capacity),
                83usize => Self(stream_read_flushed__committed),
                84usize => Self(stream_read_flushed__processing_duration),
                87usize => Self(stream_read_fin_flushed__capacity),
                88usize => Self(stream_read_fin_flushed__processing_duration),
                91usize => Self(stream_read_blocked__capacity),
                92usize => Self(stream_read_blocked__processing_duration),
                95usize => Self(stream_read_errored__capacity),
                96usize => Self(stream_read_errored__processing_duration),
                101usize => Self(stream_read_socket_flushed__capacity),
                103usize => Self(stream_read_socket_flushed__committed),
                105usize => Self(stream_read_socket_blocked__capacity),
                107usize => Self(stream_read_socket_errored__capacity),
                114usize => Self(path_secret_map_initialized__capacity),
                116usize => Self(path_secret_map_uninitialized__capacity),
                117usize => Self(path_secret_map_uninitialized__entries),
                118usize => Self(path_secret_map_uninitialized__lifetime),
                138usize => Self(key_accepted__gap),
                139usize => Self(key_accepted__forward_shift),
                142usize => Self(replay_potentially_detected__gap),
                169usize => Self(path_secret_map_cleaner_cycled__entries__id),
                170usize => Self(path_secret_map_cleaner_cycled__entries__id__retired),
                171usize => Self(path_secret_map_cleaner_cycled__entries__id__utilization),
                172usize => Self(path_secret_map_cleaner_cycled__entries__id__utilization__initial),
                173usize => Self(path_secret_map_cleaner_cycled__entries__address),
                174usize => Self(path_secret_map_cleaner_cycled__entries__address__retired),
                175usize => Self(path_secret_map_cleaner_cycled__entries__address__utilization),
                176usize => {
                    Self(path_secret_map_cleaner_cycled__entries__address__utilization__initial)
                }
                177usize => Self(path_secret_map_cleaner_cycled__handshake_requests),
                178usize => Self(path_secret_map_cleaner_cycled__handshake_requests__retired),
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
            # [link_name = s2n_quic_dc__event__measure__stream_write_flushed__provided]
            fn stream_write_flushed__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_flushed__committed]
            fn stream_write_flushed__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_flushed__processing_duration]
            fn stream_write_flushed__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_fin_flushed__provided]
            fn stream_write_fin_flushed__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_fin_flushed__committed]
            fn stream_write_fin_flushed__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_fin_flushed__processing_duration]
            fn stream_write_fin_flushed__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_blocked__provided]
            fn stream_write_blocked__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_blocked__processing_duration]
            fn stream_write_blocked__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_errored__provided]
            fn stream_write_errored__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_errored__processing_duration]
            fn stream_write_errored__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_shutdown__buffer_len]
            fn stream_write_shutdown__buffer_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_flushed__provided]
            fn stream_write_socket_flushed__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_flushed__committed]
            fn stream_write_socket_flushed__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_blocked__provided]
            fn stream_write_socket_blocked__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_socket_errored__provided]
            fn stream_write_socket_errored__provided(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_flushed__capacity]
            fn stream_read_flushed__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_flushed__committed]
            fn stream_read_flushed__committed(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_flushed__processing_duration]
            fn stream_read_flushed__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_fin_flushed__capacity]
            fn stream_read_fin_flushed__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_fin_flushed__processing_duration]
            fn stream_read_fin_flushed__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_blocked__capacity]
            fn stream_read_blocked__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_blocked__processing_duration]
            fn stream_read_blocked__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_errored__capacity]
            fn stream_read_errored__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_errored__processing_duration]
            fn stream_read_errored__processing_duration(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_socket_flushed__capacity]
            fn stream_read_socket_flushed__capacity(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_read_socket_flushed__committed]
            fn stream_read_socket_flushed__committed(value: u64);
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
            # [link_name = s2n_quic_dc__event__measure__key_accepted__gap]
            fn key_accepted__gap(value: u64);
            # [link_name = s2n_quic_dc__event__measure__key_accepted__forward_shift]
            fn key_accepted__forward_shift(value: u64);
            # [link_name = s2n_quic_dc__event__measure__replay_potentially_detected__gap]
            fn replay_potentially_detected__gap(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id]
            fn path_secret_map_cleaner_cycled__entries__id(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id__retired]
            fn path_secret_map_cleaner_cycled__entries__id__retired(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id__utilization]
            fn path_secret_map_cleaner_cycled__entries__id__utilization(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__id__utilization__initial]
            fn path_secret_map_cleaner_cycled__entries__id__utilization__initial(value: u64);
            # [link_name = s2n_quic_dc__event__measure__path_secret_map_cleaner_cycled__entries__address]
            fn path_secret_map_cleaner_cycled__entries__address(value: u64);
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
                54usize => Self(stream_write_fin_flushed__latency),
                60usize => Self(stream_write_blocked__latency),
                64usize => Self(stream_write_errored__latency),
                68usize => Self(stream_write_shutdown__latency),
                80usize => Self(stream_read_flushed__latency),
                86usize => Self(stream_read_fin_flushed__latency),
                90usize => Self(stream_read_blocked__latency),
                94usize => Self(stream_read_errored__latency),
                98usize => Self(stream_read_shutdown__latency),
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
