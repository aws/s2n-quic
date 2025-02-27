// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    packet::{self, stream},
    socket::recv::descriptor,
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::inet::SocketAddress;

/// Routes incoming packet segments to the appropriate destination
pub trait Router {
    const TAG_LEN: usize = 16;

    #[inline]
    fn on_segment(&self, mut segment: descriptor::Filled) {
        let remote_address = segment.remote_address().get();
        let decoder = DecoderBufferMut::new(segment.payload_mut());
        match decoder.decode_parameterized(Self::TAG_LEN) {
            // We don't check `remaining` since we currently assume one packet per segment.
            // If we ever support multiple packets per segment, we'll need to split the segment up even
            // further and correctly dispatch to the right place.
            Ok((packet, _remaining)) => match packet {
                packet::Packet::Control(c) => {
                    let tag = c.tag();
                    let stream_id = c.stream_id().copied();
                    let credentials = *c.credentials();
                    self.on_control_packet(tag, stream_id, credentials, segment);
                }
                packet::Packet::Stream(packet) => {
                    let tag = packet.tag();
                    let stream_id = *packet.stream_id();
                    let credentials = *packet.credentials();
                    self.on_stream_packet(tag, stream_id, credentials, segment);
                }
                packet::Packet::Datagram(packet) => {
                    let tag = packet.tag();
                    let credentials = *packet.credentials();
                    self.on_datagram_packet(tag, credentials, segment);
                }
                packet::Packet::StaleKey(packet) => {
                    self.on_stale_key_packet(packet, remote_address);
                }
                packet::Packet::ReplayDetected(packet) => {
                    self.on_replay_detected_packet(packet, remote_address);
                }
                packet::Packet::UnknownPathSecret(packet) => {
                    self.on_unknown_path_secret_packet(packet, remote_address);
                }
            },
            Err(error) => {
                self.on_decode_error(error, remote_address, segment);
            }
        }
    }

    #[inline]
    fn on_control_packet(
        &self,
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

    #[inline]
    fn on_stream_packet(
        &self,
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

    #[inline]
    fn on_datagram_packet(
        &self,
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
    fn on_stale_key_packet(
        &self,
        packet: packet::secret_control::stale_key::Packet,
        remote_address: SocketAddress,
    ) {
        tracing::warn!(unhandled_packet = "stale_key", ?packet, ?remote_address,);
    }

    #[inline]
    fn on_replay_detected_packet(
        &self,
        packet: packet::secret_control::replay_detected::Packet,
        remote_address: SocketAddress,
    ) {
        tracing::warn!(
            unhandled_packet = "replay_detected",
            ?packet,
            ?remote_address,
        );
    }

    #[inline]
    fn on_unknown_path_secret_packet(
        &self,
        packet: packet::secret_control::unknown_path_secret::Packet,
        remote_address: SocketAddress,
    ) {
        tracing::warn!(
            unhandled_packet = "unknown_path_secret",
            ?packet,
            ?remote_address,
        );
    }

    #[inline]
    fn on_decode_error(
        &self,
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
