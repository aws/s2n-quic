// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::{
    event::metrics::aggregate::{self, info, Info, NominalRecorder, Recorder},
    probe::define,
};
mod counter {
    #![allow(non_snake_case)]
    use super::Info;
    use crate::event::metrics::aggregate::AsMetric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            match info.id {
                0usize => Self(application_protocol_information),
                1usize => Self(server_name_information),
                2usize => Self(packet_skipped),
                3usize => Self(packet_sent),
                4usize => Self(packet_sent__bytes__total),
                6usize => Self(packet_received),
                7usize => Self(active_path_updated),
                8usize => Self(path_created),
                9usize => Self(frame_sent),
                11usize => Self(frame_received),
                13usize => Self(packet_lost),
                14usize => Self(packet_lost__bytes__total),
                17usize => Self(recovery_metrics),
                27usize => Self(congestion),
                29usize => Self(rx_ack_range_dropped),
                30usize => Self(ack_range_received),
                31usize => Self(ack_range_sent),
                32usize => Self(packet_dropped),
                34usize => Self(key_update),
                35usize => Self(key_space_discarded),
                37usize => Self(connection_started),
                38usize => Self(connection_closed),
                40usize => Self(duplicate_packet),
                42usize => Self(transport_parameters_received),
                43usize => Self(datagram_sent),
                44usize => Self(datagram_sent__bytes__total),
                47usize => Self(datagram_received),
                48usize => Self(datagram_received__bytes__total),
                50usize => Self(datagram_dropped),
                51usize => Self(datagram_dropped__bytes__total),
                54usize => Self(connection_id_updated),
                55usize => Self(ecn_state_changed),
                57usize => Self(connection_migration_denied),
                59usize => Self(handshake_status_updated),
                61usize => Self(tls_exporter_ready),
                62usize => Self(path_challenge_updated),
                63usize => Self(tls_client_hello),
                64usize => Self(tls_server_hello),
                65usize => Self(rx_stream_progress),
                66usize => Self(rx_stream_progress__bytes__total),
                68usize => Self(tx_stream_progress),
                69usize => Self(tx_stream_progress__bytes__total),
                71usize => Self(keep_alive_timer_expired),
                72usize => Self(mtu_updated),
                76usize => Self(slow_start_exited),
                79usize => Self(delivery_rate_sampled),
                80usize => Self(pacing_rate_updated),
                83usize => Self(bbr_state_changed),
                85usize => Self(dc_state_changed),
                87usize => Self(version_information),
                88usize => Self(endpoint_packet_sent),
                89usize => Self(endpoint_packet_received),
                90usize => Self(endpoint_datagram_sent),
                94usize => Self(endpoint_datagram_received),
                97usize => Self(endpoint_datagram_dropped),
                101usize => Self(endpoint_connection_attempt_failed),
                103usize => Self(platform_tx),
                104usize => Self(platform_tx__packets__total),
                106usize => Self(platform_tx__syscalls__total),
                108usize => Self(platform_tx__syscalls__blocked__total),
                110usize => Self(platform_tx__errors__total),
                112usize => Self(platform_tx__errors__dropped__total),
                114usize => Self(platform_tx_error),
                115usize => Self(platform_rx),
                116usize => Self(platform_rx__packets__total),
                118usize => Self(platform_rx__syscalls__total),
                120usize => Self(platform_rx__syscalls__blocked__total),
                122usize => Self(platform_rx__errors__total),
                124usize => Self(platform_rx__errors__dropped__total),
                126usize => Self(platform_rx_error),
                127usize => Self(platform_feature_configured),
                128usize => Self(platform_event_loop_wakeup),
                129usize => Self(platform_event_loop_sleep),
                131usize => Self(platform_event_loop_started),
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
            # [link_name = s2n_quic__event__counter__application_protocol_information]
            fn application_protocol_information(value: u64);
            # [link_name = s2n_quic__event__counter__server_name_information]
            fn server_name_information(value: u64);
            # [link_name = s2n_quic__event__counter__packet_skipped]
            fn packet_skipped(value: u64);
            # [link_name = s2n_quic__event__counter__packet_sent]
            fn packet_sent(value: u64);
            # [link_name = s2n_quic__event__counter__packet_sent__bytes__total]
            fn packet_sent__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__packet_received]
            fn packet_received(value: u64);
            # [link_name = s2n_quic__event__counter__active_path_updated]
            fn active_path_updated(value: u64);
            # [link_name = s2n_quic__event__counter__path_created]
            fn path_created(value: u64);
            # [link_name = s2n_quic__event__counter__frame_sent]
            fn frame_sent(value: u64);
            # [link_name = s2n_quic__event__counter__frame_received]
            fn frame_received(value: u64);
            # [link_name = s2n_quic__event__counter__packet_lost]
            fn packet_lost(value: u64);
            # [link_name = s2n_quic__event__counter__packet_lost__bytes__total]
            fn packet_lost__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__recovery_metrics]
            fn recovery_metrics(value: u64);
            # [link_name = s2n_quic__event__counter__congestion]
            fn congestion(value: u64);
            # [link_name = s2n_quic__event__counter__rx_ack_range_dropped]
            fn rx_ack_range_dropped(value: u64);
            # [link_name = s2n_quic__event__counter__ack_range_received]
            fn ack_range_received(value: u64);
            # [link_name = s2n_quic__event__counter__ack_range_sent]
            fn ack_range_sent(value: u64);
            # [link_name = s2n_quic__event__counter__packet_dropped]
            fn packet_dropped(value: u64);
            # [link_name = s2n_quic__event__counter__key_update]
            fn key_update(value: u64);
            # [link_name = s2n_quic__event__counter__key_space_discarded]
            fn key_space_discarded(value: u64);
            # [link_name = s2n_quic__event__counter__connection_started]
            fn connection_started(value: u64);
            # [link_name = s2n_quic__event__counter__connection_closed]
            fn connection_closed(value: u64);
            # [link_name = s2n_quic__event__counter__duplicate_packet]
            fn duplicate_packet(value: u64);
            # [link_name = s2n_quic__event__counter__transport_parameters_received]
            fn transport_parameters_received(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_sent]
            fn datagram_sent(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_sent__bytes__total]
            fn datagram_sent__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_received]
            fn datagram_received(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_received__bytes__total]
            fn datagram_received__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_dropped]
            fn datagram_dropped(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_dropped__bytes__total]
            fn datagram_dropped__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__connection_id_updated]
            fn connection_id_updated(value: u64);
            # [link_name = s2n_quic__event__counter__ecn_state_changed]
            fn ecn_state_changed(value: u64);
            # [link_name = s2n_quic__event__counter__connection_migration_denied]
            fn connection_migration_denied(value: u64);
            # [link_name = s2n_quic__event__counter__handshake_status_updated]
            fn handshake_status_updated(value: u64);
            # [link_name = s2n_quic__event__counter__tls_exporter_ready]
            fn tls_exporter_ready(value: u64);
            # [link_name = s2n_quic__event__counter__path_challenge_updated]
            fn path_challenge_updated(value: u64);
            # [link_name = s2n_quic__event__counter__tls_client_hello]
            fn tls_client_hello(value: u64);
            # [link_name = s2n_quic__event__counter__tls_server_hello]
            fn tls_server_hello(value: u64);
            # [link_name = s2n_quic__event__counter__rx_stream_progress]
            fn rx_stream_progress(value: u64);
            # [link_name = s2n_quic__event__counter__rx_stream_progress__bytes__total]
            fn rx_stream_progress__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__tx_stream_progress]
            fn tx_stream_progress(value: u64);
            # [link_name = s2n_quic__event__counter__tx_stream_progress__bytes__total]
            fn tx_stream_progress__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__keep_alive_timer_expired]
            fn keep_alive_timer_expired(value: u64);
            # [link_name = s2n_quic__event__counter__mtu_updated]
            fn mtu_updated(value: u64);
            # [link_name = s2n_quic__event__counter__slow_start_exited]
            fn slow_start_exited(value: u64);
            # [link_name = s2n_quic__event__counter__delivery_rate_sampled]
            fn delivery_rate_sampled(value: u64);
            # [link_name = s2n_quic__event__counter__pacing_rate_updated]
            fn pacing_rate_updated(value: u64);
            # [link_name = s2n_quic__event__counter__bbr_state_changed]
            fn bbr_state_changed(value: u64);
            # [link_name = s2n_quic__event__counter__dc_state_changed]
            fn dc_state_changed(value: u64);
            # [link_name = s2n_quic__event__counter__version_information]
            fn version_information(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_packet_sent]
            fn endpoint_packet_sent(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_packet_received]
            fn endpoint_packet_received(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_datagram_sent]
            fn endpoint_datagram_sent(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_datagram_received]
            fn endpoint_datagram_received(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_datagram_dropped]
            fn endpoint_datagram_dropped(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_connection_attempt_failed]
            fn endpoint_connection_attempt_failed(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx]
            fn platform_tx(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__packets__total]
            fn platform_tx__packets__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__syscalls__total]
            fn platform_tx__syscalls__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__syscalls__blocked__total]
            fn platform_tx__syscalls__blocked__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__errors__total]
            fn platform_tx__errors__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__errors__dropped__total]
            fn platform_tx__errors__dropped__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx_error]
            fn platform_tx_error(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx]
            fn platform_rx(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__packets__total]
            fn platform_rx__packets__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__syscalls__total]
            fn platform_rx__syscalls__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__syscalls__blocked__total]
            fn platform_rx__syscalls__blocked__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__errors__total]
            fn platform_rx__errors__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__errors__dropped__total]
            fn platform_rx__errors__dropped__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx_error]
            fn platform_rx_error(value: u64);
            # [link_name = s2n_quic__event__counter__platform_feature_configured]
            fn platform_feature_configured(value: u64);
            # [link_name = s2n_quic__event__counter__platform_event_loop_wakeup]
            fn platform_event_loop_wakeup(value: u64);
            # [link_name = s2n_quic__event__counter__platform_event_loop_sleep]
            fn platform_event_loop_sleep(value: u64);
            # [link_name = s2n_quic__event__counter__platform_event_loop_started]
            fn platform_event_loop_started(value: u64);
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
                10usize => Self(frame_sent__frame),
                12usize => Self(frame_received__frame),
                16usize => Self(packet_lost__is_mtu_probe),
                26usize => Self(recovery_metrics__congestion_limited),
                28usize => Self(congestion__source),
                33usize => Self(packet_dropped__reason),
                36usize => Self(key_space_discarded__space),
                39usize => Self(connection_closed__error),
                41usize => Self(duplicate_packet__error),
                53usize => Self(datagram_dropped__reason),
                56usize => Self(ecn_state_changed__state),
                58usize => Self(connection_migration_denied__reason),
                60usize => Self(handshake_status_updated__status),
                74usize => Self(mtu_updated__cause),
                75usize => Self(mtu_updated__search_complete),
                77usize => Self(slow_start_exited__cause),
                84usize => Self(bbr_state_changed__state),
                86usize => Self(dc_state_changed__state),
                100usize => Self(endpoint_datagram_dropped__reason),
                102usize => Self(endpoint_connection_attempt_failed__error),
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
            # [link_name = s2n_quic__event__nominal_counter__frame_sent__frame]
            fn frame_sent__frame(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__frame_received__frame]
            fn frame_received__frame(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__packet_lost__is_mtu_probe]
            fn packet_lost__is_mtu_probe(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__recovery_metrics__congestion_limited]
            fn recovery_metrics__congestion_limited(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic__event__nominal_counter__congestion__source]
            fn congestion__source(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__packet_dropped__reason]
            fn packet_dropped__reason(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__key_space_discarded__space]
            fn key_space_discarded__space(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__connection_closed__error]
            fn connection_closed__error(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__duplicate_packet__error]
            fn duplicate_packet__error(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__datagram_dropped__reason]
            fn datagram_dropped__reason(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__ecn_state_changed__state]
            fn ecn_state_changed__state(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__connection_migration_denied__reason]
            fn connection_migration_denied__reason(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic__event__nominal_counter__handshake_status_updated__status]
            fn handshake_status_updated__status(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__mtu_updated__cause]
            fn mtu_updated__cause(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__mtu_updated__search_complete]
            fn mtu_updated__search_complete(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__slow_start_exited__cause]
            fn slow_start_exited__cause(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__bbr_state_changed__state]
            fn bbr_state_changed__state(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__dc_state_changed__state]
            fn dc_state_changed__state(value: u64, variant: u64, variant_name: &info::Str);
            # [link_name = s2n_quic__event__nominal_counter__endpoint_datagram_dropped__reason]
            fn endpoint_datagram_dropped__reason(
                value: u64,
                variant: u64,
                variant_name: &info::Str,
            );
            # [link_name = s2n_quic__event__nominal_counter__endpoint_connection_attempt_failed__error]
            fn endpoint_connection_attempt_failed__error(
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
                5usize => Self(packet_sent__bytes),
                15usize => Self(packet_lost__bytes),
                18usize => Self(recovery_metrics__min_rtt),
                19usize => Self(recovery_metrics__smoothed_rtt),
                20usize => Self(recovery_metrics__latest_rtt),
                21usize => Self(recovery_metrics__rtt_variance),
                22usize => Self(recovery_metrics__max_ack_delay),
                23usize => Self(recovery_metrics__pto_count),
                24usize => Self(recovery_metrics__congestion_window),
                25usize => Self(recovery_metrics__bytes_in_flight),
                45usize => Self(datagram_sent__bytes),
                46usize => Self(datagram_sent__gso_offset),
                49usize => Self(datagram_received__bytes),
                52usize => Self(datagram_dropped__bytes),
                67usize => Self(rx_stream_progress__bytes),
                70usize => Self(tx_stream_progress__bytes),
                73usize => Self(mtu_updated__mtu),
                78usize => Self(slow_start_exited__congestion_window),
                81usize => Self(pacing_rate_updated__bytes_per_second),
                82usize => Self(pacing_rate_updated__burst_size),
                91usize => Self(endpoint_datagram_sent__bytes),
                92usize => Self(endpoint_datagram_sent__bytes__total),
                93usize => Self(endpoint_datagram_sent__gso_offset),
                95usize => Self(endpoint_datagram_received__bytes),
                96usize => Self(endpoint_datagram_received__bytes__total),
                98usize => Self(endpoint_datagram_dropped__bytes),
                99usize => Self(endpoint_datagram_dropped__bytes__total),
                105usize => Self(platform_tx__packets),
                107usize => Self(platform_tx__syscalls),
                109usize => Self(platform_tx__syscalls__blocked),
                111usize => Self(platform_tx__errors),
                113usize => Self(platform_tx__errors__dropped),
                117usize => Self(platform_rx__packets),
                119usize => Self(platform_rx__syscalls),
                121usize => Self(platform_rx__syscalls__blocked),
                123usize => Self(platform_rx__errors),
                125usize => Self(platform_rx__errors__dropped),
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
            # [link_name = s2n_quic__event__measure__packet_sent__bytes]
            fn packet_sent__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__packet_lost__bytes]
            fn packet_lost__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__min_rtt]
            fn recovery_metrics__min_rtt(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__smoothed_rtt]
            fn recovery_metrics__smoothed_rtt(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__latest_rtt]
            fn recovery_metrics__latest_rtt(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__rtt_variance]
            fn recovery_metrics__rtt_variance(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__max_ack_delay]
            fn recovery_metrics__max_ack_delay(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__pto_count]
            fn recovery_metrics__pto_count(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__congestion_window]
            fn recovery_metrics__congestion_window(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__bytes_in_flight]
            fn recovery_metrics__bytes_in_flight(value: u64);
            # [link_name = s2n_quic__event__measure__datagram_sent__bytes]
            fn datagram_sent__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__datagram_sent__gso_offset]
            fn datagram_sent__gso_offset(value: u64);
            # [link_name = s2n_quic__event__measure__datagram_received__bytes]
            fn datagram_received__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__datagram_dropped__bytes]
            fn datagram_dropped__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__rx_stream_progress__bytes]
            fn rx_stream_progress__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__tx_stream_progress__bytes]
            fn tx_stream_progress__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__mtu_updated__mtu]
            fn mtu_updated__mtu(value: u64);
            # [link_name = s2n_quic__event__measure__slow_start_exited__congestion_window]
            fn slow_start_exited__congestion_window(value: u64);
            # [link_name = s2n_quic__event__measure__pacing_rate_updated__bytes_per_second]
            fn pacing_rate_updated__bytes_per_second(value: u64);
            # [link_name = s2n_quic__event__measure__pacing_rate_updated__burst_size]
            fn pacing_rate_updated__burst_size(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_sent__bytes]
            fn endpoint_datagram_sent__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_sent__bytes__total]
            fn endpoint_datagram_sent__bytes__total(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_sent__gso_offset]
            fn endpoint_datagram_sent__gso_offset(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_received__bytes]
            fn endpoint_datagram_received__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_received__bytes__total]
            fn endpoint_datagram_received__bytes__total(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_dropped__bytes]
            fn endpoint_datagram_dropped__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_dropped__bytes__total]
            fn endpoint_datagram_dropped__bytes__total(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__packets]
            fn platform_tx__packets(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__syscalls]
            fn platform_tx__syscalls(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__syscalls__blocked]
            fn platform_tx__syscalls__blocked(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__errors]
            fn platform_tx__errors(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__errors__dropped]
            fn platform_tx__errors__dropped(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__packets]
            fn platform_rx__packets(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__syscalls]
            fn platform_rx__syscalls(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__syscalls__blocked]
            fn platform_rx__syscalls__blocked(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__errors]
            fn platform_rx__errors(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__errors__dropped]
            fn platform_rx__errors__dropped(value: u64);
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
            match info.id {
                130usize => Self(platform_event_loop_sleep__processing_duration),
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
            # [link_name = s2n_quic__event__timer__platform_event_loop_sleep__processing_duration]
            fn platform_event_loop_sleep__processing_duration(value: u64);
        }
    );
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
