// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{self, Credentials},
    packet::{self, stream},
    path::secret,
    socket::recv::descriptor,
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    varint::VarInt,
};

// Use `debug` logging for unhandled packets in non-test builds to reduce noise
#[cfg(not(test))]
use tracing::debug as warn;
#[cfg(test)]
use tracing::warn;

mod with_map;
mod zero_router;

pub use with_map::WithMap;
pub use zero_router::ZeroRouter;

/// Routes incoming packet segments to the appropriate destination
pub trait Router {
    /// Wraps `self` in a router that intercepts secret control messages and forwards
    /// them to the provided [`secret::Map`].
    #[inline]
    fn with_map(self, map: secret::Map) -> WithMap<Self>
    where
        Self: Sized,
    {
        WithMap::new(self, map)
    }

    /// Wraps `self` in a router that intercepts packets with a `0` queue ID and routes
    /// it to the provides `zero` router.
    #[inline]
    fn with_zero_router<Zero: Router>(self, zero: Zero) -> ZeroRouter<Zero, Self>
    where
        Self: Sized,
    {
        ZeroRouter {
            zero,
            non_zero: self,
        }
    }

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
            Ok((packet, remaining)) => {
                if cfg!(test) {
                    assert!(
                        remaining.is_empty(),
                        "packet = {packet:?}, remaining = {remaining:?}"
                    );
                }
                match packet {
                    packet::Packet::Control(packet) => {
                        let tag = packet.tag();
                        let stream_id = packet.stream_id().copied();
                        let credentials = *packet.credentials();
                        tracing::trace!(?tag, ?stream_id, ?credentials, "parsed_control_packet");
                        self.handle_control_packet(remote_address, ecn, packet);
                        self.dispatch_control_packet(tag, stream_id, credentials, segment);
                    }
                    packet::Packet::Stream(packet) => {
                        let tag = packet.tag();
                        let stream_id = *packet.stream_id();
                        let credentials = *packet.credentials();
                        tracing::trace!(?tag, ?stream_id, ?credentials, "parsed_stream_packet");
                        self.handle_stream_packet(remote_address, ecn, packet);
                        self.dispatch_stream_packet(tag, stream_id, credentials, segment);
                    }
                    packet::Packet::Datagram(packet) => {
                        let tag = packet.tag();
                        let credentials = *packet.credentials();
                        tracing::trace!(?tag, ?credentials, "parsed_datagram_packet");
                        self.handle_datagram_packet(remote_address, ecn, packet);
                        self.dispatch_datagram_packet(tag, credentials, segment);
                    }
                    packet::Packet::StaleKey(packet) => {
                        tracing::trace!(?packet, "parsed_stale_key_packet");
                        let queue_id = packet.queue_id();
                        let credentials = *packet.credential_id();
                        self.handle_stale_key_packet(packet, remote_address);
                        self.dispatch_stale_key_packet(queue_id, credentials, segment);
                    }
                    packet::Packet::ReplayDetected(packet) => {
                        tracing::trace!(?packet, "parsed_replay_detected_packet");
                        let queue_id = packet.queue_id();
                        let credentials = *packet.credential_id();
                        self.handle_replay_detected_packet(packet, remote_address);
                        self.dispatch_replay_detected_packet(queue_id, credentials, segment);
                    }
                    packet::Packet::UnknownPathSecret(packet) => {
                        tracing::trace!(?packet, "parsed_unknown_path_secret_packet");
                        let queue_id = packet.queue_id();
                        let credentials = *packet.credential_id();
                        self.handle_unknown_path_secret_packet(packet, remote_address);
                        self.dispatch_unknown_path_secret_packet(queue_id, credentials, segment);
                    }
                }
            }
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
        warn!(
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
        warn!(
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
        warn!(
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
    fn dispatch_stale_key_packet(
        &mut self,
        queue_id: Option<VarInt>,
        credentials: credentials::Id,
        segment: descriptor::Filled,
    ) {
        warn!(
            unhandled_packet = "stale_key",
            ?queue_id,
            ?credentials,
            remote_address = ?segment.remote_address(),
            packet_len = segment.len()
        );
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
    fn dispatch_replay_detected_packet(
        &mut self,
        queue_id: Option<VarInt>,
        credentials: credentials::Id,
        segment: descriptor::Filled,
    ) {
        warn!(
            unhandled_packet = "replay_detected",
            ?queue_id,
            ?credentials,
            remote_address = ?segment.remote_address(),
            packet_len = segment.len()
        );
    }

    #[inline]
    fn handle_unknown_path_secret_packet(
        &mut self,
        packet: packet::secret_control::unknown_path_secret::Packet,
        remote_address: SocketAddress,
    ) {
        self.on_unhandled_packet(remote_address, packet::Packet::UnknownPathSecret(packet));
    }

    fn dispatch_unknown_path_secret_packet(
        &mut self,
        queue_id: Option<VarInt>,
        credentials: credentials::Id,
        segment: descriptor::Filled,
    ) {
        warn!(
            unhandled_packet = "unknown_path_secret",
            ?queue_id,
            ?credentials,
            remote_address = ?segment.remote_address(),
            packet_len = segment.len()
        );
    }

    #[inline]
    fn on_unhandled_packet(&mut self, remote_address: SocketAddress, packet: packet::Packet) {
        warn!(unhandled_packet = ?packet, ?remote_address)
    }

    #[inline]
    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: SocketAddress,
        segment: descriptor::Filled,
    ) {
        warn!(
            ?error,
            ?remote_address,
            packet_len = segment.len(),
            "failed to decode packet"
        );
    }
}
