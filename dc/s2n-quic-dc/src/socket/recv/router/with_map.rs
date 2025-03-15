// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Router;
use crate::{
    credentials::Credentials,
    packet::{self, stream},
    path::secret,
    socket::recv::descriptor,
};
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

#[derive(Clone)]
pub struct WithMap<Inner> {
    inner: Inner,
    map: secret::Map,
}

impl<Inner> WithMap<Inner> {
    #[inline]
    pub fn new(inner: Inner, map: secret::Map) -> Self {
        Self { inner, map }
    }
}

impl<Inner: Router> Router for WithMap<Inner> {
    #[inline]
    fn is_open(&self) -> bool {
        self.inner.is_open()
    }

    #[inline]
    fn tag_len(&self) -> usize {
        self.inner.tag_len()
    }

    #[inline]
    fn handle_control_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::control::decoder::Packet,
    ) {
        self.inner
            .handle_control_packet(remote_address, ecn, packet);
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        tag: packet::control::Tag,
        id: Option<stream::Id>,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        self.inner
            .dispatch_control_packet(tag, id, credentials, segment);
    }

    #[inline]
    fn handle_stream_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::stream::decoder::Packet,
    ) {
        self.inner.handle_stream_packet(remote_address, ecn, packet);
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        tag: stream::Tag,
        id: stream::Id,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        self.inner
            .dispatch_stream_packet(tag, id, credentials, segment);
    }

    #[inline]
    fn handle_datagram_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::datagram::decoder::Packet,
    ) {
        self.inner
            .handle_datagram_packet(remote_address, ecn, packet);
    }

    #[inline]
    fn dispatch_datagram_packet(
        &mut self,
        tag: packet::datagram::Tag,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        self.inner
            .dispatch_datagram_packet(tag, credentials, segment);
    }

    #[inline]
    fn handle_stale_key_packet(
        &mut self,
        packet: packet::secret_control::stale_key::Packet,
        remote_address: SocketAddress,
    ) {
        // TODO check if the packet was authentic before forwarding the packet on to inner
        self.map.handle_control_packet(
            &packet::secret_control::Packet::StaleKey(packet),
            &remote_address.into(),
        );
        self.inner.handle_stale_key_packet(packet, remote_address);
    }

    #[inline]
    fn handle_replay_detected_packet(
        &mut self,
        packet: packet::secret_control::replay_detected::Packet,
        remote_address: SocketAddress,
    ) {
        // TODO check if the packet was authentic before forwarding the packet on to inner
        self.map.handle_control_packet(
            &packet::secret_control::Packet::ReplayDetected(packet),
            &remote_address.into(),
        );
        self.inner
            .handle_replay_detected_packet(packet, remote_address);
    }

    #[inline]
    fn handle_unknown_path_secret_packet(
        &mut self,
        packet: packet::secret_control::unknown_path_secret::Packet,
        remote_address: SocketAddress,
    ) {
        // TODO check if the packet was authentic before forwarding the packet on to inner
        self.map.handle_control_packet(
            &packet::secret_control::Packet::UnknownPathSecret(packet),
            &remote_address.into(),
        );
        self.inner
            .handle_unknown_path_secret_packet(packet, remote_address);
    }

    #[inline]
    fn on_unhandled_packet(&mut self, remote_address: SocketAddress, packet: packet::Packet) {
        self.inner.on_unhandled_packet(remote_address, packet);
    }

    #[inline]
    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: SocketAddress,
        segment: descriptor::Filled,
    ) {
        self.inner.on_decode_error(error, remote_address, segment);
    }
}
