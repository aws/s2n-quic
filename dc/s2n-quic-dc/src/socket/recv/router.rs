// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    packet::{self, stream},
    socket::recv::descriptor,
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

/// Routes incoming packet segments to the appropriate destination
pub trait Router {
    fn is_open(&self) -> bool;

    #[inline(always)]
    fn tag_len(&self) -> usize {
        16
    }

    #[inline]
    fn on_segment(&mut self, mut segment: descriptor::Filled) {
        let remote_address = segment.remote_address().get();
        let ecn = segment.ecn();
        let decoder = DecoderBufferMut::new(segment.payload_mut());
        match decoder.decode_parameterized(self.tag_len()) {
            // We don't check `remaining` since we currently assume one packet per segment.
            // If we ever support multiple packets per segment, we'll need to split the segment up even
            // further and correctly dispatch to the right place.
            Ok((packet, _remaining)) => match packet {
                packet::Packet::Control(packet) => {
                    let tag = packet.tag();
                    let stream_id = packet.stream_id().copied();
                    let credentials = *packet.credentials();
                    self.handle_control_packet(remote_address, ecn, packet);
                    self.dispatch_control_packet(tag, stream_id, credentials, segment);
                }
                packet::Packet::Stream(packet) => {
                    let tag = packet.tag();
                    let stream_id = *packet.stream_id();
                    let credentials = *packet.credentials();
                    self.handle_stream_packet(remote_address, ecn, packet);
                    self.dispatch_stream_packet(tag, stream_id, credentials, segment);
                }
                packet::Packet::Datagram(packet) => {
                    let tag = packet.tag();
                    let credentials = *packet.credentials();
                    self.handle_datagram_packet(remote_address, ecn, packet);
                    self.dispatch_datagram_packet(tag, credentials, segment);
                }
                packet::Packet::StaleKey(packet) => {
                    self.handle_stale_key_packet(packet, remote_address);
                }
                packet::Packet::ReplayDetected(packet) => {
                    self.handle_replay_detected_packet(packet, remote_address);
                }
                packet::Packet::UnknownPathSecret(packet) => {
                    self.handle_unknown_path_secret_packet(packet, remote_address);
                }
            },
            Err(error) => {
                self.on_decode_error(error, remote_address, segment);
            }
        }
    }

    #[inline(always)]
    fn handle_control_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::control::decoder::Packet,
    ) {
        let _ = ecn;
        self.on_unhandled_packet(remote_address, packet::Packet::Control(packet));
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        tag: packet::control::Tag,
        id: Option<stream::Id>,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        tracing::warn!(
            unhandled_packet = "control",
            ?tag,
            ?id,
            ?credentials,
            remote_address = ?segment.remote_address(),
            packet_len = segment.len()
        );
    }

    #[inline(always)]
    fn handle_stream_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::stream::decoder::Packet,
    ) {
        let _ = ecn;
        self.on_unhandled_packet(remote_address, packet::Packet::Stream(packet));
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        tag: stream::Tag,
        id: stream::Id,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        tracing::warn!(
            unhandled_packet = "stream",
            ?tag,
            ?id,
            ?credentials,
            remote_address = ?segment.remote_address(),
            packet_len = segment.len()
        );
    }

    #[inline(always)]
    fn handle_datagram_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::datagram::decoder::Packet,
    ) {
        let _ = ecn;
        self.on_unhandled_packet(remote_address, packet::Packet::Datagram(packet));
    }

    #[inline]
    fn dispatch_datagram_packet(
        &mut self,
        tag: packet::datagram::Tag,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        tracing::warn!(
            unhandled_packet = "datagram",
            ?tag,
            ?credentials,
            remote_address = ?segment.remote_address(),
            packet_len = segment.len()
        );
    }

    #[inline]
    fn handle_stale_key_packet(
        &mut self,
        packet: packet::secret_control::stale_key::Packet,
        remote_address: SocketAddress,
    ) {
        self.on_unhandled_packet(remote_address, packet::Packet::StaleKey(packet));
    }

    #[inline]
    fn handle_replay_detected_packet(
        &mut self,
        packet: packet::secret_control::replay_detected::Packet,
        remote_address: SocketAddress,
    ) {
        self.on_unhandled_packet(remote_address, packet::Packet::ReplayDetected(packet));
    }

    #[inline]
    fn handle_unknown_path_secret_packet(
        &mut self,
        packet: packet::secret_control::unknown_path_secret::Packet,
        remote_address: SocketAddress,
    ) {
        self.on_unhandled_packet(remote_address, packet::Packet::UnknownPathSecret(packet));
    }

    #[inline]
    fn on_unhandled_packet(&mut self, remote_address: SocketAddress, packet: packet::Packet) {
        tracing::warn!(unhandled_packet = ?packet, ?remote_address)
    }

    #[inline]
    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: SocketAddress,
        segment: descriptor::Filled,
    ) {
        tracing::warn!(
            ?error,
            ?remote_address,
            packet_len = segment.len(),
            "failed to decode packet"
        );
    }
}
