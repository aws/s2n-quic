// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::event::{self, api, metrics::Recorder};
#[cfg(feature = "alloc")]
pub(crate) mod aggregate;
pub(crate) mod probe;
#[derive(Debug)]
pub struct Subscriber<S: event::Subscriber>
where
    S::ConnectionContext: Recorder,
{
    subscriber: S,
}
impl<S: event::Subscriber> Subscriber<S>
where
    S::ConnectionContext: Recorder,
{
    pub fn new(subscriber: S) -> Self {
        Self { subscriber }
    }
}
pub struct Context<R: Recorder> {
    recorder: R,
    application_protocol_information: u64,
    server_name_information: u64,
    key_exchange_group: u64,
    packet_skipped: u64,
    packet_sent: u64,
    packet_received: u64,
    active_path_updated: u64,
    path_created: u64,
    frame_sent: u64,
    frame_received: u64,
    connection_close_frame_received: u64,
    packet_lost: u64,
    recovery_metrics: u64,
    congestion: u64,
    ack_processed: u64,
    rx_ack_range_dropped: u64,
    ack_range_received: u64,
    ack_range_sent: u64,
    packet_dropped: u64,
    key_update: u64,
    key_space_discarded: u64,
    connection_started: u64,
    duplicate_packet: u64,
    transport_parameters_received: u64,
    datagram_sent: u64,
    datagram_received: u64,
    datagram_dropped: u64,
    handshake_remote_address_change_observed: u64,
    connection_id_updated: u64,
    ecn_state_changed: u64,
    connection_migration_denied: u64,
    handshake_status_updated: u64,
    tls_exporter_ready: u64,
    tls_handshake_failed: u64,
    path_challenge_updated: u64,
    tls_client_hello: u64,
    tls_server_hello: u64,
    rx_stream_progress: u64,
    tx_stream_progress: u64,
    keep_alive_timer_expired: u64,
    mtu_updated: u64,
    slow_start_exited: u64,
    delivery_rate_sampled: u64,
    pacing_rate_updated: u64,
    bbr_state_changed: u64,
    dc_state_changed: u64,
    dc_path_created: u64,
    connection_closed: u64,
}
impl<R: Recorder> Context<R> {
    pub fn inner(&self) -> &R {
        &self.recorder
    }
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.recorder
    }
}
impl<S: event::Subscriber> event::Subscriber for Subscriber<S>
where
    S::ConnectionContext: Recorder,
{
    type ConnectionContext = Context<S::ConnectionContext>;
    fn create_connection_context(
        &mut self,
        meta: &api::ConnectionMeta,
        info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
        Context {
            recorder: self.subscriber.create_connection_context(meta, info),
            application_protocol_information: 0,
            server_name_information: 0,
            key_exchange_group: 0,
            packet_skipped: 0,
            packet_sent: 0,
            packet_received: 0,
            active_path_updated: 0,
            path_created: 0,
            frame_sent: 0,
            frame_received: 0,
            connection_close_frame_received: 0,
            packet_lost: 0,
            recovery_metrics: 0,
            congestion: 0,
            ack_processed: 0,
            rx_ack_range_dropped: 0,
            ack_range_received: 0,
            ack_range_sent: 0,
            packet_dropped: 0,
            key_update: 0,
            key_space_discarded: 0,
            connection_started: 0,
            duplicate_packet: 0,
            transport_parameters_received: 0,
            datagram_sent: 0,
            datagram_received: 0,
            datagram_dropped: 0,
            handshake_remote_address_change_observed: 0,
            connection_id_updated: 0,
            ecn_state_changed: 0,
            connection_migration_denied: 0,
            handshake_status_updated: 0,
            tls_exporter_ready: 0,
            tls_handshake_failed: 0,
            path_challenge_updated: 0,
            tls_client_hello: 0,
            tls_server_hello: 0,
            rx_stream_progress: 0,
            tx_stream_progress: 0,
            keep_alive_timer_expired: 0,
            mtu_updated: 0,
            slow_start_exited: 0,
            delivery_rate_sampled: 0,
            pacing_rate_updated: 0,
            bbr_state_changed: 0,
            dc_state_changed: 0,
            dc_path_created: 0,
            connection_closed: 0,
        }
    }
    #[inline]
    fn on_application_protocol_information(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ApplicationProtocolInformation,
    ) {
        context.application_protocol_information += 1;
        self.subscriber
            .on_application_protocol_information(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_server_name_information(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ServerNameInformation,
    ) {
        context.server_name_information += 1;
        self.subscriber
            .on_server_name_information(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_key_exchange_group(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeyExchangeGroup,
    ) {
        context.key_exchange_group += 1;
        self.subscriber
            .on_key_exchange_group(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_packet_skipped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketSkipped,
    ) {
        context.packet_skipped += 1;
        self.subscriber
            .on_packet_skipped(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_packet_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketSent,
    ) {
        context.packet_sent += 1;
        self.subscriber
            .on_packet_sent(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_packet_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketReceived,
    ) {
        context.packet_received += 1;
        self.subscriber
            .on_packet_received(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_active_path_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ActivePathUpdated,
    ) {
        context.active_path_updated += 1;
        self.subscriber
            .on_active_path_updated(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_path_created(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PathCreated,
    ) {
        context.path_created += 1;
        self.subscriber
            .on_path_created(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_frame_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::FrameSent,
    ) {
        context.frame_sent += 1;
        self.subscriber
            .on_frame_sent(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_frame_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::FrameReceived,
    ) {
        context.frame_received += 1;
        self.subscriber
            .on_frame_received(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_connection_close_frame_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionCloseFrameReceived,
    ) {
        context.connection_close_frame_received += 1;
        self.subscriber
            .on_connection_close_frame_received(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_packet_lost(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketLost,
    ) {
        context.packet_lost += 1;
        self.subscriber
            .on_packet_lost(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_recovery_metrics(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::RecoveryMetrics,
    ) {
        context.recovery_metrics += 1;
        self.subscriber
            .on_recovery_metrics(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_congestion(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::Congestion,
    ) {
        context.congestion += 1;
        self.subscriber
            .on_congestion(&mut context.recorder, meta, event);
    }
    #[inline]
    #[allow(deprecated)]
    fn on_ack_processed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::AckProcessed,
    ) {
        context.ack_processed += 1;
        self.subscriber
            .on_ack_processed(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_rx_ack_range_dropped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::RxAckRangeDropped,
    ) {
        context.rx_ack_range_dropped += 1;
        self.subscriber
            .on_rx_ack_range_dropped(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_ack_range_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::AckRangeReceived,
    ) {
        context.ack_range_received += 1;
        self.subscriber
            .on_ack_range_received(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_ack_range_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::AckRangeSent,
    ) {
        context.ack_range_sent += 1;
        self.subscriber
            .on_ack_range_sent(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_packet_dropped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketDropped,
    ) {
        context.packet_dropped += 1;
        self.subscriber
            .on_packet_dropped(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_key_update(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeyUpdate,
    ) {
        context.key_update += 1;
        self.subscriber
            .on_key_update(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_key_space_discarded(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeySpaceDiscarded,
    ) {
        context.key_space_discarded += 1;
        self.subscriber
            .on_key_space_discarded(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_connection_started(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionStarted,
    ) {
        context.connection_started += 1;
        self.subscriber
            .on_connection_started(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_duplicate_packet(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DuplicatePacket,
    ) {
        context.duplicate_packet += 1;
        self.subscriber
            .on_duplicate_packet(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_transport_parameters_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TransportParametersReceived,
    ) {
        context.transport_parameters_received += 1;
        self.subscriber
            .on_transport_parameters_received(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_datagram_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DatagramSent,
    ) {
        context.datagram_sent += 1;
        self.subscriber
            .on_datagram_sent(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_datagram_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DatagramReceived,
    ) {
        context.datagram_received += 1;
        self.subscriber
            .on_datagram_received(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_datagram_dropped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DatagramDropped,
    ) {
        context.datagram_dropped += 1;
        self.subscriber
            .on_datagram_dropped(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_handshake_remote_address_change_observed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::HandshakeRemoteAddressChangeObserved,
    ) {
        context.handshake_remote_address_change_observed += 1;
        self.subscriber.on_handshake_remote_address_change_observed(
            &mut context.recorder,
            meta,
            event,
        );
    }
    #[inline]
    fn on_connection_id_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionIdUpdated,
    ) {
        context.connection_id_updated += 1;
        self.subscriber
            .on_connection_id_updated(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_ecn_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::EcnStateChanged,
    ) {
        context.ecn_state_changed += 1;
        self.subscriber
            .on_ecn_state_changed(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_connection_migration_denied(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionMigrationDenied,
    ) {
        context.connection_migration_denied += 1;
        self.subscriber
            .on_connection_migration_denied(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_handshake_status_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::HandshakeStatusUpdated,
    ) {
        context.handshake_status_updated += 1;
        self.subscriber
            .on_handshake_status_updated(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_tls_exporter_ready(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsExporterReady,
    ) {
        context.tls_exporter_ready += 1;
        self.subscriber
            .on_tls_exporter_ready(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_tls_handshake_failed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsHandshakeFailed,
    ) {
        context.tls_handshake_failed += 1;
        self.subscriber
            .on_tls_handshake_failed(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_path_challenge_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PathChallengeUpdated,
    ) {
        context.path_challenge_updated += 1;
        self.subscriber
            .on_path_challenge_updated(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_tls_client_hello(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsClientHello,
    ) {
        context.tls_client_hello += 1;
        self.subscriber
            .on_tls_client_hello(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_tls_server_hello(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsServerHello,
    ) {
        context.tls_server_hello += 1;
        self.subscriber
            .on_tls_server_hello(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_rx_stream_progress(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::RxStreamProgress,
    ) {
        context.rx_stream_progress += 1;
        self.subscriber
            .on_rx_stream_progress(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_tx_stream_progress(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TxStreamProgress,
    ) {
        context.tx_stream_progress += 1;
        self.subscriber
            .on_tx_stream_progress(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_keep_alive_timer_expired(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeepAliveTimerExpired,
    ) {
        context.keep_alive_timer_expired += 1;
        self.subscriber
            .on_keep_alive_timer_expired(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_mtu_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::MtuUpdated,
    ) {
        context.mtu_updated += 1;
        self.subscriber
            .on_mtu_updated(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_slow_start_exited(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::SlowStartExited,
    ) {
        context.slow_start_exited += 1;
        self.subscriber
            .on_slow_start_exited(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_delivery_rate_sampled(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DeliveryRateSampled,
    ) {
        context.delivery_rate_sampled += 1;
        self.subscriber
            .on_delivery_rate_sampled(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_pacing_rate_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacingRateUpdated,
    ) {
        context.pacing_rate_updated += 1;
        self.subscriber
            .on_pacing_rate_updated(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_bbr_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::BbrStateChanged,
    ) {
        context.bbr_state_changed += 1;
        self.subscriber
            .on_bbr_state_changed(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_dc_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DcStateChanged,
    ) {
        context.dc_state_changed += 1;
        self.subscriber
            .on_dc_state_changed(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_dc_path_created(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DcPathCreated,
    ) {
        context.dc_path_created += 1;
        self.subscriber
            .on_dc_path_created(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_connection_closed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionClosed,
    ) {
        context.connection_closed += 1;
        self.subscriber
            .on_connection_closed(&mut context.recorder, meta, event);
    }
}
impl<R: Recorder> Drop for Context<R> {
    fn drop(&mut self) {
        self.recorder.increment_counter(
            "application_protocol_information",
            self.application_protocol_information as _,
        );
        self.recorder
            .increment_counter("server_name_information", self.server_name_information as _);
        self.recorder
            .increment_counter("key_exchange_group", self.key_exchange_group as _);
        self.recorder
            .increment_counter("packet_skipped", self.packet_skipped as _);
        self.recorder
            .increment_counter("packet_sent", self.packet_sent as _);
        self.recorder
            .increment_counter("packet_received", self.packet_received as _);
        self.recorder
            .increment_counter("active_path_updated", self.active_path_updated as _);
        self.recorder
            .increment_counter("path_created", self.path_created as _);
        self.recorder
            .increment_counter("frame_sent", self.frame_sent as _);
        self.recorder
            .increment_counter("frame_received", self.frame_received as _);
        self.recorder.increment_counter(
            "connection_close_frame_received",
            self.connection_close_frame_received as _,
        );
        self.recorder
            .increment_counter("packet_lost", self.packet_lost as _);
        self.recorder
            .increment_counter("recovery_metrics", self.recovery_metrics as _);
        self.recorder
            .increment_counter("congestion", self.congestion as _);
        self.recorder
            .increment_counter("ack_processed", self.ack_processed as _);
        self.recorder
            .increment_counter("rx_ack_range_dropped", self.rx_ack_range_dropped as _);
        self.recorder
            .increment_counter("ack_range_received", self.ack_range_received as _);
        self.recorder
            .increment_counter("ack_range_sent", self.ack_range_sent as _);
        self.recorder
            .increment_counter("packet_dropped", self.packet_dropped as _);
        self.recorder
            .increment_counter("key_update", self.key_update as _);
        self.recorder
            .increment_counter("key_space_discarded", self.key_space_discarded as _);
        self.recorder
            .increment_counter("connection_started", self.connection_started as _);
        self.recorder
            .increment_counter("duplicate_packet", self.duplicate_packet as _);
        self.recorder.increment_counter(
            "transport_parameters_received",
            self.transport_parameters_received as _,
        );
        self.recorder
            .increment_counter("datagram_sent", self.datagram_sent as _);
        self.recorder
            .increment_counter("datagram_received", self.datagram_received as _);
        self.recorder
            .increment_counter("datagram_dropped", self.datagram_dropped as _);
        self.recorder.increment_counter(
            "handshake_remote_address_change_observed",
            self.handshake_remote_address_change_observed as _,
        );
        self.recorder
            .increment_counter("connection_id_updated", self.connection_id_updated as _);
        self.recorder
            .increment_counter("ecn_state_changed", self.ecn_state_changed as _);
        self.recorder.increment_counter(
            "connection_migration_denied",
            self.connection_migration_denied as _,
        );
        self.recorder.increment_counter(
            "handshake_status_updated",
            self.handshake_status_updated as _,
        );
        self.recorder
            .increment_counter("tls_exporter_ready", self.tls_exporter_ready as _);
        self.recorder
            .increment_counter("tls_handshake_failed", self.tls_handshake_failed as _);
        self.recorder
            .increment_counter("path_challenge_updated", self.path_challenge_updated as _);
        self.recorder
            .increment_counter("tls_client_hello", self.tls_client_hello as _);
        self.recorder
            .increment_counter("tls_server_hello", self.tls_server_hello as _);
        self.recorder
            .increment_counter("rx_stream_progress", self.rx_stream_progress as _);
        self.recorder
            .increment_counter("tx_stream_progress", self.tx_stream_progress as _);
        self.recorder.increment_counter(
            "keep_alive_timer_expired",
            self.keep_alive_timer_expired as _,
        );
        self.recorder
            .increment_counter("mtu_updated", self.mtu_updated as _);
        self.recorder
            .increment_counter("slow_start_exited", self.slow_start_exited as _);
        self.recorder
            .increment_counter("delivery_rate_sampled", self.delivery_rate_sampled as _);
        self.recorder
            .increment_counter("pacing_rate_updated", self.pacing_rate_updated as _);
        self.recorder
            .increment_counter("bbr_state_changed", self.bbr_state_changed as _);
        self.recorder
            .increment_counter("dc_state_changed", self.dc_state_changed as _);
        self.recorder
            .increment_counter("dc_path_created", self.dc_path_created as _);
        self.recorder
            .increment_counter("connection_closed", self.connection_closed as _);
    }
}
