// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Worker infrastructure for distributing packets across send/recv sockets.

use crate::{
    counter::{Counter, Registry},
    credentials,
    intrusive::Entry,
    packet::{self, datagram::RoutingInfo},
    socket::{channel, pool::descriptor, recv::router::Router},
    stream::endpoint::routing,
};
use s2n_quic_core::varint::VarInt;

// ── Packet Router ──────────────────────────────────────────────────────────

/// Routes decoded datagram packets to one of N dispatch queues based on a hash
/// of (credentials.id, source_sender_id).
///
/// This ensures that all packets from the same peer always land in the same
/// dispatch task, maintaining coherent ACK space and packet-number deduplication.
pub(crate) struct FanOutRouter<D, Route, Inv> {
    txs: Vec<D>,
    route: Route,
    invalidation_tx: Inv,
    decode_error_counter: Counter,
    routed_counter: Counter,
    route_send_err_counter: Counter,
    per_worker_routed: Vec<Counter>,
}

impl<D, Route: routing::SenderRoute, Inv> FanOutRouter<D, Route, Inv> {
    pub fn new(txs: Vec<D>, invalidation_tx: Inv, counters: &Registry) -> Self {
        let route = Route::new(txs.len());
        let per_worker_routed = (0..txs.len())
            .map(|i| counters.register_nominal("router.routed", format_args!("recv.{i}")))
            .collect();
        Self {
            txs,
            route,
            invalidation_tx,
            decode_error_counter: counters.register("!router.decode_err"),
            routed_counter: counters.register("router.routed"),
            route_send_err_counter: counters.register("!router.send_err"),
            per_worker_routed,
        }
    }
}

impl<D, Route, Inv> Router for FanOutRouter<D, Route, Inv>
where
    D: channel::UnboundedSender<Entry<packet::datagram::decoder::Packet<descriptor::Filled>>>,
    Route: routing::SenderRoute,
    Inv: channel::UnboundedSender<Entry<descriptor::Filled>>,
{
    fn is_open(&self) -> bool {
        true
    }

    #[inline]
    fn dispatch_datagram_packet(
        &mut self,
        packet: packet::datagram::decoder::Packet<descriptor::Filled>,
    ) {
        let RoutingInfo::SenderId { source_sender_id } = packet.routing_info() else {
            tracing::info!(?packet, "invalid packet routing info");
            return;
        };
        let idx = self
            .route
            .worker_id_for_recv(packet.credentials(), source_sender_id);
        self.routed_counter.add(1);
        self.per_worker_routed[idx].add(1);
        if self.txs[idx].send(packet.into()).is_err() {
            self.route_send_err_counter.add(1);
        }
    }

    #[inline]
    fn handle_datagram_packet(
        &mut self,
        _remote_address: s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::datagram::decoder::Packet<&mut [u8]>,
    ) {
    }

    fn dispatch_unknown_path_secret_packet(
        &mut self,
        _queue_id: Option<VarInt>,
        _credentials: credentials::Id,
        segment: descriptor::Filled,
    ) {
        let _ = self.invalidation_tx.send(segment.into());
    }

    fn dispatch_stale_key_packet(
        &mut self,
        _sender_id: Option<VarInt>,
        _credentials: credentials::Id,
        segment: descriptor::Filled,
    ) {
        let _ = self.invalidation_tx.send(segment.into());
    }

    fn dispatch_replay_detected_packet(
        &mut self,
        _queue_id: Option<VarInt>,
        _credentials: credentials::Id,
        segment: descriptor::Filled,
    ) {
        let _ = self.invalidation_tx.send(segment.into());
    }

    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: s2n_quic_core::inet::SocketAddress,
        segment: descriptor::Filled,
    ) {
        self.decode_error_counter.add(1);
        tracing::debug!(
            ?error,
            %remote_address,
            packet_len = segment.len(),
            "failed to decode packet"
        );
    }
}
