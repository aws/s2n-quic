// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::{
    event::metrics::aggregate::{self, Info, Recorder},
    probe::define,
};
mod counter {
    #![allow(non_snake_case)]
    use super::Info;
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
                10usize => Self(frame_received),
                11usize => Self(packet_lost),
                12usize => Self(packet_lost__bytes_lost__total),
                14usize => Self(recovery_metrics),
                23usize => Self(congestion),
                24usize => Self(rx_ack_range_dropped),
                25usize => Self(ack_range_received),
                26usize => Self(ack_range_sent),
                27usize => Self(packet_dropped),
                28usize => Self(key_update),
                29usize => Self(key_space_discarded),
                30usize => Self(connection_started),
                31usize => Self(connection_closed),
                32usize => Self(duplicate_packet),
                33usize => Self(transport_parameters_received),
                34usize => Self(datagram_sent),
                35usize => Self(datagram_sent__bytes__total),
                38usize => Self(datagram_received),
                39usize => Self(datagram_received__bytes__total),
                41usize => Self(datagram_dropped),
                42usize => Self(datagram_dropped__bytes__total),
                44usize => Self(connection_id_updated),
                45usize => Self(ecn_state_changed),
                46usize => Self(connection_migration_denied),
                47usize => Self(handshake_status_updated),
                48usize => Self(tls_exporter_ready),
                49usize => Self(path_challenge_updated),
                50usize => Self(tls_client_hello),
                51usize => Self(tls_server_hello),
                52usize => Self(rx_stream_progress),
                53usize => Self(rx_stream_progress__bytes__total),
                55usize => Self(tx_stream_progress),
                56usize => Self(tx_stream_progress__bytes__total),
                58usize => Self(keep_alive_timer_expired),
                59usize => Self(mtu_updated),
                61usize => Self(slow_start_exited),
                63usize => Self(delivery_rate_sampled),
                64usize => Self(pacing_rate_updated),
                67usize => Self(bbr_state_changed),
                68usize => Self(dc_state_changed),
                69usize => Self(version_information),
                70usize => Self(endpoint_packet_sent),
                71usize => Self(endpoint_packet_received),
                72usize => Self(endpoint_datagram_sent),
                76usize => Self(endpoint_datagram_received),
                79usize => Self(endpoint_datagram_dropped),
                82usize => Self(endpoint_connection_attempt_failed),
                83usize => Self(platform_tx),
                84usize => Self(platform_tx__packets__total),
                86usize => Self(platform_tx__syscalls__total),
                88usize => Self(platform_tx__syscalls__blocked__total),
                90usize => Self(platform_tx__errors__total),
                92usize => Self(platform_tx__errors__dropped__total),
                94usize => Self(platform_tx_error),
                95usize => Self(platform_rx),
                96usize => Self(platform_rx__packets__total),
                98usize => Self(platform_rx__syscalls__total),
                100usize => Self(platform_rx__syscalls__blocked__total),
                102usize => Self(platform_rx__errors__total),
                104usize => Self(platform_rx__errors__dropped__total),
                106usize => Self(platform_rx_error),
                107usize => Self(platform_feature_configured),
                108usize => Self(platform_event_loop_wakeup),
                109usize => Self(platform_event_loop_sleep),
                111usize => Self(platform_event_loop_started),
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
            # [link_name = s2n_quic__event__counter__packet_lost__bytes_lost__total]
            fn packet_lost__bytes_lost__total(value: u64);
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
mod measure {
    #![allow(non_snake_case)]
    use super::Info;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(super) fn new(info: &'static Info) -> Self {
            match info.id {
                5usize => Self(packet_sent__bytes),
                13usize => Self(packet_lost__bytes_lost),
                15usize => Self(recovery_metrics__min_rtt),
                16usize => Self(recovery_metrics__smoothed_rtt),
                17usize => Self(recovery_metrics__latest_rtt),
                18usize => Self(recovery_metrics__rtt_variance),
                19usize => Self(recovery_metrics__max_ack_delay),
                20usize => Self(recovery_metrics__pto_count),
                21usize => Self(recovery_metrics__congestion_window),
                22usize => Self(recovery_metrics__bytes_in_flight),
                36usize => Self(datagram_sent__bytes),
                37usize => Self(datagram_sent__gso_offset),
                40usize => Self(datagram_received__bytes),
                43usize => Self(datagram_dropped__bytes),
                54usize => Self(rx_stream_progress__bytes),
                57usize => Self(tx_stream_progress__bytes),
                60usize => Self(mtu_updated__mtu),
                62usize => Self(slow_start_exited__congestion_window),
                65usize => Self(pacing_rate_updated__bytes_per_second),
                66usize => Self(pacing_rate_updated__burst_size),
                73usize => Self(endpoint_datagram_sent__bytes),
                74usize => Self(endpoint_datagram_sent__bytes__total),
                75usize => Self(endpoint_datagram_sent__gso_offset),
                77usize => Self(endpoint_datagram_received__bytes),
                78usize => Self(endpoint_datagram_received__bytes__total),
                80usize => Self(endpoint_datagram_dropped__bytes),
                81usize => Self(endpoint_datagram_dropped__bytes__total),
                85usize => Self(platform_tx__packets),
                87usize => Self(platform_tx__syscalls),
                89usize => Self(platform_tx__syscalls__blocked),
                91usize => Self(platform_tx__errors),
                93usize => Self(platform_tx__errors__dropped),
                97usize => Self(platform_rx__packets),
                99usize => Self(platform_rx__syscalls),
                101usize => Self(platform_rx__syscalls__blocked),
                103usize => Self(platform_rx__errors),
                105usize => Self(platform_rx__errors__dropped),
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
            # [link_name = s2n_quic__event__measure__packet_sent__bytes]
            fn packet_sent__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__packet_lost__bytes_lost]
            fn packet_lost__bytes_lost(value: u64);
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
            match info.id {
                110usize => Self(platform_event_loop_sleep__processing_duration),
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
            # [link_name = s2n_quic__event__timer__platform_event_loop_sleep__processing_duration]
            fn platform_event_loop_sleep__processing_duration(value: u64);
        }
    );
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
