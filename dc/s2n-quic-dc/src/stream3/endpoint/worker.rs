// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Worker infrastructure for distributing packets across send/recv sockets.

use crate::{
    counter::Counter,
    intrusive_queue::Entry,
    packet::{self, datagram::RoutingInfo},
    socket::{channel, pool::descriptor, recv::router::Router},
    stream3::endpoint::routing,
};

// ── Packet Router ──────────────────────────────────────────────────────────

/// Routes decoded datagram packets to one of N dispatch queues based on a hash
/// of (credentials.id, source_sender_id).
///
/// This ensures that all packets from the same peer always land in the same
/// dispatch task, maintaining coherent ACK space and packet-number deduplication.
pub(crate) struct FanOutRouter<D, Route> {
    txs: Vec<D>,
    route: Route,
    decode_error_counter: Counter,
}

impl<D, Route: routing::SenderRoute> FanOutRouter<D, Route> {
    pub fn new(txs: Vec<D>, decode_error_counter: Counter) -> Self {
        let route = Route::new(txs.len());
        Self {
            txs,
            route,
            decode_error_counter,
        }
    }
}

impl<D, Route> Router for FanOutRouter<D, Route>
where
    D: channel::UnboundedSender<Entry<packet::datagram::decoder::Packet<descriptor::Filled>>>,
    Route: routing::SenderRoute,
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
        let _ = self.txs[idx].send(packet.into());
    }

    #[inline]
    fn handle_datagram_packet(
        &mut self,
        _remote_address: s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::datagram::decoder::Packet<&mut [u8]>,
    ) {
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
