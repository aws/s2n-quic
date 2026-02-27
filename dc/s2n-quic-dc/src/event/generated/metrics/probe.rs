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
                0usize => Self(acceptor_tcp_started),
                1usize => Self(acceptor_tcp_loop_iteration_completed),
                7usize => Self(acceptor_tcp_fresh_enqueued),
                8usize => Self(acceptor_tcp_fresh_batch_completed),
                12usize => Self(acceptor_tcp_stream_dropped),
                14usize => Self(acceptor_tcp_stream_replaced),
                17usize => Self(acceptor_tcp_packet_received),
                22usize => Self(acceptor_tcp_tls_started),
                24usize => Self(acceptor_tcp_tls_stream_enqueued),
                26usize => Self(acceptor_tcp_tls_stream_rejected),
                28usize => Self(acceptor_tcp_packet_dropped),
                31usize => Self(acceptor_tcp_stream_enqueued),
                34usize => Self(acceptor_tcp_io_error),
                36usize => Self(acceptor_tcp_socket_sent),
                38usize => Self(acceptor_tcp_socket_sent__blocked_count_host),
                41usize => Self(acceptor_tcp_socket_received),
                44usize => Self(acceptor_udp_started),
                45usize => Self(acceptor_udp_datagram_received),
                47usize => Self(acceptor_udp_packet_received),
                53usize => Self(acceptor_udp_packet_dropped),
                55usize => Self(acceptor_udp_stream_enqueued),
                56usize => Self(acceptor_udp_io_error),
                57usize => Self(acceptor_stream_pruned),
                60usize => Self(acceptor_stream_dequeued),
                62usize => Self(stream_write_flushed),
                66usize => Self(stream_write_flushed__committed__total),
                71usize => Self(stream_write_fin_flushed),
                75usize => Self(stream_write_fin_flushed__committed__total),
                80usize => Self(stream_write_blocked),
                86usize => Self(stream_write_errored),
                91usize => Self(stream_write_key_updated),
                92usize => Self(stream_write_allocated),
                96usize => Self(stream_write_shutdown),
                100usize => Self(stream_write_socket_flushed),
                103usize => Self(stream_write_socket_flushed__committed__total),
                106usize => Self(stream_write_socket_blocked),
                109usize => Self(stream_write_socket_errored),
                111usize => Self(stream_read_flushed),
                115usize => Self(stream_read_flushed__committed__total),
                120usize => Self(stream_read_fin_flushed),
                126usize => Self(stream_read_blocked),
                131usize => Self(stream_read_errored),
                136usize => Self(stream_read_key_updated),
                137usize => Self(stream_read_shutdown),
                140usize => Self(stream_read_socket_flushed),
                143usize => Self(stream_read_socket_flushed__committed__total),
                146usize => Self(stream_read_socket_blocked),
                149usize => Self(stream_read_socket_errored),
                151usize => Self(stream_decrypt_packet),
                155usize => Self(stream_tcp_connect),
                158usize => Self(stream_connect),
                162usize => Self(stream_connect_error),
                165usize => Self(stream_packet_transmitted),
                167usize => Self(stream_packet_transmitted__payload_len__total),
                171usize => Self(stream_probe_transmitted),
                173usize => Self(stream_packet_received),
                175usize => Self(stream_packet_received__payload_len__total),
                179usize => Self(stream_packet_lost),
                181usize => Self(stream_packet_lost__payload_len__total),
                186usize => Self(stream_packet_acked),
                188usize => Self(stream_packet_acked__payload_len__total),
                193usize => Self(stream_packet_spuriously_retransmitted),
                195usize => Self(stream_packet_spuriously_retransmitted__payload_len__total),
                199usize => Self(stream_max_data_received),
                200usize => Self(stream_max_data_received__increase__total),
                202usize => Self(stream_control_packet_transmitted),
                205usize => Self(stream_control_packet_received),
                209usize => Self(stream_receiver_errored),
                210usize => Self(stream_sender_errored),
                211usize => Self(connection_closed),
                212usize => Self(endpoint_initialized),
                217usize => Self(dc_connection_timeout),
                219usize => Self(path_secret_map_initialized),
                221usize => Self(path_secret_map_uninitialized),
                225usize => Self(path_secret_map_background_handshake_requested),
                227usize => Self(path_secret_map_entry_inserted),
                229usize => Self(path_secret_map_entry_ready),
                231usize => Self(path_secret_map_entry_replaced),
                233usize => Self(path_secret_map_id_entry_evicted),
                236usize => Self(path_secret_map_address_entry_evicted),
                239usize => Self(unknown_path_secret_packet_sent),
                241usize => Self(unknown_path_secret_packet_received),
                243usize => Self(unknown_path_secret_packet_accepted),
                245usize => Self(unknown_path_secret_packet_rejected),
                247usize => Self(unknown_path_secret_packet_dropped),
                249usize => Self(key_accepted),
                252usize => Self(replay_definitely_detected),
                253usize => Self(replay_potentially_detected),
                255usize => Self(replay_detected_packet_sent),
                257usize => Self(replay_detected_packet_received),
                259usize => Self(replay_detected_packet_accepted),
                261usize => Self(replay_detected_packet_rejected),
                263usize => Self(replay_detected_packet_dropped),
                265usize => Self(stale_key_packet_sent),
                267usize => Self(stale_key_packet_received),
                269usize => Self(stale_key_packet_accepted),
                271usize => Self(stale_key_packet_rejected),
                273usize => Self(stale_key_packet_dropped),
                275usize => Self(path_secret_map_address_cache_accessed),
                278usize => Self(path_secret_map_address_cache_accessed_hit),
                281usize => Self(path_secret_map_id_cache_accessed),
                283usize => Self(path_secret_map_id_cache_accessed_hit),
                285usize => Self(path_secret_map_cleaner_cycled),
                302usize => Self(path_secret_map_id_write_lock),
                305usize => Self(path_secret_map_address_write_lock),
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
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_tls_started]
            fn acceptor_tcp_tls_started(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_tls_stream_enqueued]
            fn acceptor_tcp_tls_stream_enqueued(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_tls_stream_rejected]
            fn acceptor_tcp_tls_stream_rejected(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_packet_dropped]
            fn acceptor_tcp_packet_dropped(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_stream_enqueued]
            fn acceptor_tcp_stream_enqueued(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_io_error]
            fn acceptor_tcp_io_error(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_socket_sent]
            fn acceptor_tcp_socket_sent(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_socket_sent__blocked_count_host]
            fn acceptor_tcp_socket_sent__blocked_count_host(value: u64);
            # [link_name = s2n_quic_dc__event__counter__acceptor_tcp_socket_received]
            fn acceptor_tcp_socket_received(value: u64);
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
            # [link_name = s2n_quic_dc__event__counter__stream_write_allocated]
            fn stream_write_allocated(value: u64);
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
            # [link_name = s2n_quic_dc__event__counter__stream_decrypt_packet]
            fn stream_decrypt_packet(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_tcp_connect]
            fn stream_tcp_connect(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_connect]
            fn stream_connect(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_connect_error]
            fn stream_connect_error(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_transmitted]
            fn stream_packet_transmitted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_transmitted__payload_len__total]
            fn stream_packet_transmitted__payload_len__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_probe_transmitted]
            fn stream_probe_transmitted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_received]
            fn stream_packet_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_received__payload_len__total]
            fn stream_packet_received__payload_len__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_lost]
            fn stream_packet_lost(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_lost__payload_len__total]
            fn stream_packet_lost__payload_len__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_acked]
            fn stream_packet_acked(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_acked__payload_len__total]
            fn stream_packet_acked__payload_len__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_spuriously_retransmitted]
            fn stream_packet_spuriously_retransmitted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_packet_spuriously_retransmitted__payload_len__total]
            fn stream_packet_spuriously_retransmitted__payload_len__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_max_data_received]
            fn stream_max_data_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_max_data_received__increase__total]
            fn stream_max_data_received__increase__total(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_control_packet_transmitted]
            fn stream_control_packet_transmitted(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_control_packet_received]
            fn stream_control_packet_received(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_receiver_errored]
            fn stream_receiver_errored(value: u64);
            # [link_name = s2n_quic_dc__event__counter__stream_sender_errored]
            fn stream_sender_errored(value: u64);
            # [link_name = s2n_quic_dc__event__counter__connection_closed]
            fn connection_closed(value: u64);
            # [link_name = s2n_quic_dc__event__counter__endpoint_initialized]
            fn endpoint_initialized(value: u64);
            # [link_name = s2n_quic_dc__event__counter__dc_connection_timeout]
            fn dc_connection_timeout(value: u64);
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
                    19usize => Self(acceptor_tcp_packet_received__is_fin),
                    20usize => Self(acceptor_tcp_packet_received__is_fin_known),
                    49usize => Self(acceptor_udp_packet_received__is_zero_offset),
                    50usize => Self(acceptor_udp_packet_received__is_retransmission),
                    51usize => Self(acceptor_udp_packet_received__is_fin),
                    52usize => Self(acceptor_udp_packet_received__is_fin_known),
                    99usize => Self(stream_write_shutdown__background),
                    139usize => Self(stream_read_shutdown__background),
                    152usize => Self(stream_decrypt_packet__decrypted_in_place),
                    156usize => Self(stream_tcp_connect__error),
                    159usize => Self(stream_connect__error),
                    170usize => Self(stream_packet_transmitted__retransmission),
                    178usize => Self(stream_packet_received__retransmission),
                    185usize => Self(stream_packet_lost__retransmission),
                    192usize => Self(stream_packet_acked__retransmission),
                    198usize => Self(stream_packet_spuriously_retransmitted__retransmission),
                    208usize => Self(stream_control_packet_received__authenticated),
                    215usize => Self(endpoint_initialized__tcp),
                    216usize => Self(endpoint_initialized__udp),
                    277usize => Self(path_secret_map_address_cache_accessed__hit),
                    282usize => Self(path_secret_map_id_cache_accessed__hit),
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
                # [link_name = s2n_quic_dc__event__counter__bool__acceptor_udp_packet_received__is_retransmission]
                fn acceptor_udp_packet_received__is_retransmission(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__acceptor_udp_packet_received__is_fin]
                fn acceptor_udp_packet_received__is_fin(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__acceptor_udp_packet_received__is_fin_known]
                fn acceptor_udp_packet_received__is_fin_known(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_write_shutdown__background]
                fn stream_write_shutdown__background(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_read_shutdown__background]
                fn stream_read_shutdown__background(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_decrypt_packet__decrypted_in_place]
                fn stream_decrypt_packet__decrypted_in_place(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_tcp_connect__error]
                fn stream_tcp_connect__error(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_connect__error]
                fn stream_connect__error(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_packet_transmitted__retransmission]
                fn stream_packet_transmitted__retransmission(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_packet_received__retransmission]
                fn stream_packet_received__retransmission(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_packet_lost__retransmission]
                fn stream_packet_lost__retransmission(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_packet_acked__retransmission]
                fn stream_packet_acked__retransmission(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_packet_spuriously_retransmitted__retransmission]
                fn stream_packet_spuriously_retransmitted__retransmission(value: bool);
                # [link_name = s2n_quic_dc__event__counter__bool__stream_control_packet_received__authenticated]
                fn stream_control_packet_received__authenticated(value: bool);
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
                    29usize => Self(acceptor_tcp_packet_dropped__reason),
                    35usize => Self(acceptor_tcp_io_error__source),
                    54usize => Self(acceptor_udp_packet_dropped__reason),
                    59usize => Self(acceptor_stream_pruned__reason),
                    160usize => Self(stream_connect__tcp),
                    161usize => Self(stream_connect__handshake),
                    163usize => Self(stream_connect_error__reason),
                    213usize => Self(endpoint_initialized__acceptor__protocol),
                    214usize => Self(endpoint_initialized__handshake__protocol),
                    218usize => Self(dc_connection_timeout__peer_address__protocol),
                    226usize => {
                        Self(path_secret_map_background_handshake_requested__peer_address__protocol)
                    }
                    228usize => Self(path_secret_map_entry_inserted__peer_address__protocol),
                    230usize => Self(path_secret_map_entry_ready__peer_address__protocol),
                    232usize => Self(path_secret_map_entry_replaced__peer_address__protocol),
                    234usize => Self(path_secret_map_id_entry_evicted__peer_address__protocol),
                    237usize => Self(path_secret_map_address_entry_evicted__peer_address__protocol),
                    240usize => Self(unknown_path_secret_packet_sent__peer_address__protocol),
                    242usize => Self(unknown_path_secret_packet_received__peer_address__protocol),
                    244usize => Self(unknown_path_secret_packet_accepted__peer_address__protocol),
                    246usize => Self(unknown_path_secret_packet_rejected__peer_address__protocol),
                    248usize => Self(unknown_path_secret_packet_dropped__peer_address__protocol),
                    256usize => Self(replay_detected_packet_sent__peer_address__protocol),
                    258usize => Self(replay_detected_packet_received__peer_address__protocol),
                    260usize => Self(replay_detected_packet_accepted__peer_address__protocol),
                    262usize => Self(replay_detected_packet_rejected__peer_address__protocol),
                    264usize => Self(replay_detected_packet_dropped__peer_address__protocol),
                    266usize => Self(stale_key_packet_sent__peer_address__protocol),
                    268usize => Self(stale_key_packet_received__peer_address__protocol),
                    270usize => Self(stale_key_packet_accepted__peer_address__protocol),
                    272usize => Self(stale_key_packet_rejected__peer_address__protocol),
                    274usize => Self(stale_key_packet_dropped__peer_address__protocol),
                    276usize => {
                        Self(path_secret_map_address_cache_accessed__peer_address__protocol)
                    }
                    279usize => {
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
                # [link_name = s2n_quic_dc__event__counter__nominal__acceptor_tcp_io_error__source]
                fn acceptor_tcp_io_error__source(
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
                # [link_name = s2n_quic_dc__event__counter__nominal__dc_connection_timeout__peer_address__protocol]
                fn dc_connection_timeout__peer_address__protocol(
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
                33usize => Self(acceptor_tcp_stream_enqueued__blocked_count),
                39usize => Self(acceptor_tcp_socket_sent__blocked_count_stream),
                40usize => Self(acceptor_tcp_socket_sent__len),
                43usize => Self(acceptor_tcp_socket_received__len),
                46usize => Self(acceptor_udp_datagram_received__len),
                48usize => Self(acceptor_udp_packet_received__payload_len),
                64usize => Self(stream_write_flushed__conn),
                65usize => Self(stream_write_flushed__provided),
                67usize => Self(stream_write_flushed__committed),
                68usize => Self(stream_write_flushed__committed__conn),
                69usize => Self(stream_write_flushed__processing_duration),
                70usize => Self(stream_write_flushed__processing_duration__conn),
                73usize => Self(stream_write_fin_flushed__conn),
                74usize => Self(stream_write_fin_flushed__provided),
                76usize => Self(stream_write_fin_flushed__committed),
                77usize => Self(stream_write_fin_flushed__committed__conn),
                78usize => Self(stream_write_fin_flushed__processing_duration),
                79usize => Self(stream_write_fin_flushed__processing_duration__conn),
                82usize => Self(stream_write_blocked__conn),
                83usize => Self(stream_write_blocked__provided),
                84usize => Self(stream_write_blocked__processing_duration),
                85usize => Self(stream_write_blocked__processing_duration__conn),
                88usize => Self(stream_write_errored__provided),
                89usize => Self(stream_write_errored__processing_duration),
                90usize => Self(stream_write_errored__processing_duration__conn),
                93usize => Self(stream_write_allocated__conn),
                94usize => Self(stream_write_allocated__allocated_len),
                95usize => Self(stream_write_allocated__allocated_len__conn),
                98usize => Self(stream_write_shutdown__buffer_len),
                101usize => Self(stream_write_socket_flushed__conn),
                102usize => Self(stream_write_socket_flushed__provided),
                104usize => Self(stream_write_socket_flushed__committed),
                105usize => Self(stream_write_socket_flushed__committed__conn),
                107usize => Self(stream_write_socket_blocked__conn),
                108usize => Self(stream_write_socket_blocked__provided),
                110usize => Self(stream_write_socket_errored__provided),
                113usize => Self(stream_read_flushed__conn),
                114usize => Self(stream_read_flushed__capacity),
                116usize => Self(stream_read_flushed__committed),
                117usize => Self(stream_read_flushed__committed__conn),
                118usize => Self(stream_read_flushed__processing_duration),
                119usize => Self(stream_read_flushed__processing_duration__conn),
                122usize => Self(stream_read_fin_flushed__conn),
                123usize => Self(stream_read_fin_flushed__capacity),
                124usize => Self(stream_read_fin_flushed__processing_duration),
                125usize => Self(stream_read_fin_flushed__processing_duration__conn),
                128usize => Self(stream_read_blocked__capacity),
                129usize => Self(stream_read_blocked__processing_duration),
                130usize => Self(stream_read_blocked__processing_duration__conn),
                133usize => Self(stream_read_errored__capacity),
                134usize => Self(stream_read_errored__processing_duration),
                135usize => Self(stream_read_errored__processing_duration__conn),
                141usize => Self(stream_read_socket_flushed__conn),
                142usize => Self(stream_read_socket_flushed__capacity),
                144usize => Self(stream_read_socket_flushed__committed),
                145usize => Self(stream_read_socket_flushed__committed__conn),
                147usize => Self(stream_read_socket_blocked__conn),
                148usize => Self(stream_read_socket_blocked__capacity),
                150usize => Self(stream_read_socket_errored__capacity),
                153usize => Self(stream_decrypt_packet__forced_copy),
                154usize => Self(stream_decrypt_packet__required_application_buffer),
                166usize => Self(stream_packet_transmitted__packet_len),
                168usize => Self(stream_packet_transmitted__payload_len),
                169usize => Self(stream_packet_transmitted__payload_len__conn),
                172usize => Self(stream_probe_transmitted__packet_len),
                174usize => Self(stream_packet_received__packet_len),
                176usize => Self(stream_packet_received__payload_len),
                177usize => Self(stream_packet_received__payload_len__conn),
                180usize => Self(stream_packet_lost__packet_len),
                182usize => Self(stream_packet_lost__payload_len),
                183usize => Self(stream_packet_lost__payload_len__conn),
                184usize => Self(stream_packet_lost__lifetime),
                187usize => Self(stream_packet_acked__packet_len),
                189usize => Self(stream_packet_acked__payload_len),
                190usize => Self(stream_packet_acked__payload_len__conn),
                191usize => Self(stream_packet_acked__lifetime),
                194usize => Self(stream_packet_spuriously_retransmitted__packet_len),
                196usize => Self(stream_packet_spuriously_retransmitted__payload_len),
                197usize => Self(stream_packet_spuriously_retransmitted__payload_len__conn),
                201usize => Self(stream_max_data_received__increase),
                203usize => Self(stream_control_packet_transmitted__packet_len),
                204usize => Self(stream_control_packet_transmitted__control_data_len),
                206usize => Self(stream_control_packet_received__packet_len),
                207usize => Self(stream_control_packet_received__control_data_len),
                220usize => Self(path_secret_map_initialized__capacity),
                222usize => Self(path_secret_map_uninitialized__capacity),
                223usize => Self(path_secret_map_uninitialized__entries),
                224usize => Self(path_secret_map_uninitialized__lifetime),
                235usize => Self(path_secret_map_id_entry_evicted__age),
                238usize => Self(path_secret_map_address_entry_evicted__age),
                250usize => Self(key_accepted__gap),
                251usize => Self(key_accepted__forward_shift),
                254usize => Self(replay_potentially_detected__gap),
                280usize => Self(path_secret_map_address_cache_accessed_hit__age),
                284usize => Self(path_secret_map_id_cache_accessed_hit__age),
                286usize => Self(path_secret_map_cleaner_cycled__entries__id),
                287usize => Self(path_secret_map_cleaner_cycled__entries__id__retired),
                288usize => Self(path_secret_map_cleaner_cycled__entries__id__active),
                289usize => Self(path_secret_map_cleaner_cycled__entries__id__active__utilization),
                290usize => Self(path_secret_map_cleaner_cycled__entries__id__utilization),
                291usize => Self(path_secret_map_cleaner_cycled__entries__id__utilization__initial),
                292usize => Self(path_secret_map_cleaner_cycled__entries__address),
                293usize => Self(path_secret_map_cleaner_cycled__entries__address__active),
                294usize => {
                    Self(path_secret_map_cleaner_cycled__entries__address__active__utilization)
                }
                295usize => Self(path_secret_map_cleaner_cycled__entries__address__retired),
                296usize => Self(path_secret_map_cleaner_cycled__entries__address__utilization),
                297usize => {
                    Self(path_secret_map_cleaner_cycled__entries__address__utilization__initial)
                }
                298usize => Self(path_secret_map_cleaner_cycled__handshake_requests),
                299usize => Self(path_secret_map_cleaner_cycled__handshake_requests__skipped),
                300usize => Self(path_secret_map_cleaner_cycled__handshake_lock_duration),
                301usize => Self(path_secret_map_cleaner_cycled__total_duration),
                303usize => Self(path_secret_map_id_write_lock__acquire),
                304usize => Self(path_secret_map_id_write_lock__duration),
                306usize => Self(path_secret_map_address_write_lock__acquire),
                307usize => Self(path_secret_map_address_write_lock__duration),
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
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_socket_sent__blocked_count_stream]
            fn acceptor_tcp_socket_sent__blocked_count_stream(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_socket_sent__len]
            fn acceptor_tcp_socket_sent__len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__acceptor_tcp_socket_received__len]
            fn acceptor_tcp_socket_received__len(value: u64);
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
            # [link_name = s2n_quic_dc__event__measure__stream_write_allocated__conn]
            fn stream_write_allocated__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_allocated__allocated_len]
            fn stream_write_allocated__allocated_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_write_allocated__allocated_len__conn]
            fn stream_write_allocated__allocated_len__conn(value: u64);
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
            # [link_name = s2n_quic_dc__event__measure__stream_decrypt_packet__forced_copy]
            fn stream_decrypt_packet__forced_copy(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_decrypt_packet__required_application_buffer]
            fn stream_decrypt_packet__required_application_buffer(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_transmitted__packet_len]
            fn stream_packet_transmitted__packet_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_transmitted__payload_len]
            fn stream_packet_transmitted__payload_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_transmitted__payload_len__conn]
            fn stream_packet_transmitted__payload_len__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_probe_transmitted__packet_len]
            fn stream_probe_transmitted__packet_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_received__packet_len]
            fn stream_packet_received__packet_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_received__payload_len]
            fn stream_packet_received__payload_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_received__payload_len__conn]
            fn stream_packet_received__payload_len__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_lost__packet_len]
            fn stream_packet_lost__packet_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_lost__payload_len]
            fn stream_packet_lost__payload_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_lost__payload_len__conn]
            fn stream_packet_lost__payload_len__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_lost__lifetime]
            fn stream_packet_lost__lifetime(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_acked__packet_len]
            fn stream_packet_acked__packet_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_acked__payload_len]
            fn stream_packet_acked__payload_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_acked__payload_len__conn]
            fn stream_packet_acked__payload_len__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_acked__lifetime]
            fn stream_packet_acked__lifetime(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_spuriously_retransmitted__packet_len]
            fn stream_packet_spuriously_retransmitted__packet_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_spuriously_retransmitted__payload_len]
            fn stream_packet_spuriously_retransmitted__payload_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_packet_spuriously_retransmitted__payload_len__conn]
            fn stream_packet_spuriously_retransmitted__payload_len__conn(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_max_data_received__increase]
            fn stream_max_data_received__increase(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_control_packet_transmitted__packet_len]
            fn stream_control_packet_transmitted__packet_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_control_packet_transmitted__control_data_len]
            fn stream_control_packet_transmitted__control_data_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_control_packet_received__packet_len]
            fn stream_control_packet_received__packet_len(value: u64);
            # [link_name = s2n_quic_dc__event__measure__stream_control_packet_received__control_data_len]
            fn stream_control_packet_received__control_data_len(value: u64);
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
            match info.id {
                5usize => Self(acceptor_tcp_loop_iteration_completed__processing_duration),
                15usize => Self(acceptor_tcp_stream_replaced__sojourn_time),
                21usize => Self(acceptor_tcp_packet_received__sojourn_time),
                23usize => Self(acceptor_tcp_tls_started__sojourn_time),
                25usize => Self(acceptor_tcp_tls_stream_enqueued__sojourn_time),
                27usize => Self(acceptor_tcp_tls_stream_rejected__sojourn_time),
                30usize => Self(acceptor_tcp_packet_dropped__sojourn_time),
                32usize => Self(acceptor_tcp_stream_enqueued__sojourn_time),
                37usize => Self(acceptor_tcp_socket_sent__sojourn_time),
                42usize => Self(acceptor_tcp_socket_received__transfer_time),
                58usize => Self(acceptor_stream_pruned__sojourn_time),
                61usize => Self(acceptor_stream_dequeued__sojourn_time),
                63usize => Self(stream_write_flushed__latency),
                72usize => Self(stream_write_fin_flushed__latency),
                81usize => Self(stream_write_blocked__latency),
                87usize => Self(stream_write_errored__latency),
                97usize => Self(stream_write_shutdown__latency),
                112usize => Self(stream_read_flushed__latency),
                121usize => Self(stream_read_fin_flushed__latency),
                127usize => Self(stream_read_blocked__latency),
                132usize => Self(stream_read_errored__latency),
                138usize => Self(stream_read_shutdown__latency),
                157usize => Self(stream_tcp_connect__tcp_latency),
                164usize => Self(stream_connect_error__latency),
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
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_tls_started__sojourn_time]
            fn acceptor_tcp_tls_started__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_tls_stream_enqueued__sojourn_time]
            fn acceptor_tcp_tls_stream_enqueued__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_tls_stream_rejected__sojourn_time]
            fn acceptor_tcp_tls_stream_rejected__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_packet_dropped__sojourn_time]
            fn acceptor_tcp_packet_dropped__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_stream_enqueued__sojourn_time]
            fn acceptor_tcp_stream_enqueued__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_socket_sent__sojourn_time]
            fn acceptor_tcp_socket_sent__sojourn_time(value: core::time::Duration);
            # [link_name = s2n_quic_dc__event__timer__acceptor_tcp_socket_received__transfer_time]
            fn acceptor_tcp_socket_received__transfer_time(value: core::time::Duration);
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
            # [link_name = s2n_quic_dc__event__timer__stream_connect_error__latency]
            fn stream_connect_error__latency(value: core::time::Duration);
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
