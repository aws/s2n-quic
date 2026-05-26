// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{self, Credentials},
    packet::{self, stream},
    path::secret,
    socket::pool::descriptor,
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    varint::VarInt,
};

use crate::tracing::trace;

// Use `debug` logging for unhandled packets in non-test builds to reduce noise
#[cfg(not(test))]
use crate::tracing::debug as warn;
#[cfg(test)]
use crate::tracing::warn;

mod with_map;

pub use with_map::WithMap;

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
        match decoder.decode_parameterized::<packet::Packet>(self.tag_len()) {
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
                    packet::Packet::Control(control_packet) => {
                        let tag = control_packet.tag();
                        let stream_id = control_packet.stream_id().copied();
                        let credentials = *control_packet.credentials();

                        // #[cfg(debug_assertions)]
                        // let _span = tracing::info_span!("recv::control", peer_addr = %remote_address, flow_id = %credentials).entered();

                        trace!(?tag, ?stream_id, %credentials, "parsed_control_packet");
                        let meta = *control_packet.meta();
                        self.handle_control_packet(remote_address, ecn, control_packet);

                        // Convert packet storage from &mut [u8] to Filled
                        let packet = meta.with_storage(segment).expect("storage should be valid");
                        self.dispatch_control_packet(packet);
                    }
                    packet::Packet::Stream(stream_packet) => {
                        let tag = stream_packet.tag();
                        let stream_id = *stream_packet.stream_id();
                        let credentials = *stream_packet.credentials();

                        // #[cfg(debug_assertions)]
                        // let _span = tracing::info_span!("recv::stream", peer_addr = %remote_address, flow_id = %credentials).entered();

                        trace!(?tag, ?stream_id, %credentials, "parsed_stream_packet");

                        self.handle_stream_packet(remote_address, ecn, stream_packet);
                        self.dispatch_stream_packet(tag, stream_id, credentials, segment);
                    }
                    packet::Packet::Datagram(datagram_packet) => {
                        let tag = datagram_packet.tag();
                        let credentials = *datagram_packet.credentials();

                        // #[cfg(debug_assertions)]
                        // let _span = tracing::info_span!("recv::datagram", peer_addr = %remote_address, flow_id = %credentials).entered();

                        trace!(?tag, %credentials, "parsed_datagram_packet");
                        let meta = *datagram_packet.meta();
                        self.handle_datagram_packet(remote_address, ecn, datagram_packet);

                        // Convert packet storage from &mut [u8] to Filled
                        let packet = meta.with_storage(segment).expect("storage should be valid");
                        self.dispatch_datagram_packet(packet);
                    }
                    packet::Packet::QueueReset(packet) => {
                        let tag = packet.tag();
                        let queue_id = packet.queue_id();
                        let credentials = *packet.credentials();
                        let trigger = packet.trigger();

                        // #[cfg(debug_assertions)]
                        // let _span = tracing::info_span!("recv::queue_reset", peer_addr = %remote_address, flow_id = %credentials).entered();

                        trace!(?tag, ?queue_id, %credentials, ?trigger, "parsed_queue_reset_packet");
                        self.handle_queue_reset_packet(remote_address, ecn, packet);
                        self.dispatch_queue_reset_packet(
                            tag,
                            queue_id,
                            credentials,
                            trigger,
                            segment,
                        );
                    }
                    packet::Packet::StaleKey(packet) => {
                        trace!(?packet, "parsed_stale_key_packet");
                        let sender_id = packet.sender_id();
                        let credentials = *packet.credential_id();
                        self.handle_stale_key_packet(packet, remote_address);
                        self.dispatch_stale_key_packet(sender_id, credentials, segment);
                    }
                    packet::Packet::ReplayDetected(packet) => {
                        trace!(?packet, "parsed_replay_detected_packet");
                        let sender_id = packet.sender_id();
                        let credentials = *packet.credential_id();
                        self.handle_replay_detected_packet(packet, remote_address);
                        self.dispatch_replay_detected_packet(sender_id, credentials, segment);
                    }
                    packet::Packet::UnknownPathSecret(packet) => {
                        trace!(?packet, "parsed_unknown_path_secret_packet");
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
        packet: packet::control::decoder::Packet<&mut [u8]>,
    ) {
        let _ = ecn;
        self.on_unhandled_packet(remote_address, packet::Packet::Control(packet));
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        packet: packet::control::decoder::Packet<descriptor::Filled>,
    ) {
        let (meta, segment) = packet.into_parts();
        warn!(
            unhandled_packet = "control",
            router = core::any::type_name::<Self>(),
            tag = ?meta.tag(),
            id = ?meta.stream_id(),
            credentials = %meta.credentials(),
            remote_address = %segment.remote_address(),
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
            router = core::any::type_name::<Self>(),
            ?tag,
            ?id,
            %credentials,
            remote_address = %segment.remote_address(),
            packet_len = segment.len()
        );
    }

    #[inline(always)]
    fn handle_datagram_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::datagram::decoder::Packet<&mut [u8]>,
    ) {
        let _ = ecn;
        self.on_unhandled_packet(remote_address, packet::Packet::Datagram(packet));
    }

    #[inline]
    fn dispatch_datagram_packet(
        &mut self,
        packet: packet::datagram::decoder::Packet<descriptor::Filled>,
    ) {
        let (meta, segment) = packet.into_parts();
        warn!(
            unhandled_packet = "datagram",
            router = core::any::type_name::<Self>(),
            tag = ?meta.tag(),
            credentials = %meta.credentials(),
            remote_address = %segment.remote_address(),
            packet_len = segment.len()
        );
    }

    #[inline(always)]
    fn handle_queue_reset_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::secret_control::queue_reset::Packet,
    ) {
        let _ = ecn;
        self.on_unhandled_packet(remote_address, packet::Packet::QueueReset(packet));
    }

    #[inline]
    fn dispatch_queue_reset_packet(
        &mut self,
        tag: packet::secret_control::queue_reset::Tag,
        queue_id: VarInt,
        credentials: Credentials,
        trigger: packet::secret_control::queue_reset::Trigger,
        segment: descriptor::Filled,
    ) {
        warn!(
            unhandled_packet = "queue_reset",
            router = core::any::type_name::<Self>(),
            ?tag,
            queue_id = queue_id.as_u64(),
            %credentials,
            ?trigger,
            remote_address = %segment.remote_address(),
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
        sender_id: Option<VarInt>,
        credentials: credentials::Id,
        segment: descriptor::Filled,
    ) {
        warn!(
            unhandled_packet = "stale_key",
            router = core::any::type_name::<Self>(),
            ?sender_id,
            %credentials,
            remote_address = %segment.remote_address(),
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
        sender_id: Option<VarInt>,
        credentials: credentials::Id,
        segment: descriptor::Filled,
    ) {
        warn!(
            unhandled_packet = "replay_detected",
            router = core::any::type_name::<Self>(),
            ?sender_id,
            %credentials,
            remote_address = %segment.remote_address(),
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
            router = core::any::type_name::<Self>(),
            ?queue_id,
            %credentials,
            remote_address = %segment.remote_address(),
            packet_len = segment.len()
        );
    }

    #[inline]
    fn on_unhandled_packet(&mut self, remote_address: SocketAddress, packet: packet::Packet) {
        warn!(unhandled_packet = ?packet, %remote_address)
    }

    #[inline]
    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: SocketAddress,
        segment: descriptor::Filled,
    ) {
        warn!(
            router = core::any::type_name::<Self>(),
            ?error,
            %remote_address,
            packet_len = segment.len(),
            "failed to decode packet"
        );
    }
}
