// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Router;
use crate::{
    credentials::Credentials,
    packet::{self, stream},
    socket::recv::descriptor,
};
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    varint::VarInt,
};

/// Routes packets to a zero or non-zero queue ID handler
#[derive(Clone)]
pub struct ZeroRouter<Zero, NonZero> {
    pub zero: Zero,
    pub non_zero: NonZero,
}

impl<Zero, NonZero> Router for ZeroRouter<Zero, NonZero>
where
    Zero: Router,
    NonZero: Router,
{
    #[inline]
    fn is_open(&self) -> bool {
        self.zero.is_open() && self.non_zero.is_open()
    }

    #[inline]
    fn tag_len(&self) -> usize {
        debug_assert_eq!(self.zero.tag_len(), self.non_zero.tag_len());
        self.zero.tag_len()
    }

    #[inline]
    fn handle_control_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::control::decoder::Packet,
    ) {
        if packet
            .stream_id()
            .is_some_and(|id| id.queue_id == VarInt::ZERO)
        {
            self.zero.handle_control_packet(remote_address, ecn, packet);
        } else {
            self.non_zero
                .handle_control_packet(remote_address, ecn, packet);
        }
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        tag: packet::control::Tag,
        id: Option<stream::Id>,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        if id.is_some_and(|id| id.queue_id == VarInt::ZERO) {
            self.zero
                .dispatch_control_packet(tag, id, credentials, segment);
        } else {
            self.non_zero
                .dispatch_control_packet(tag, id, credentials, segment);
        }
    }

    #[inline]
    fn handle_stream_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::stream::decoder::Packet,
    ) {
        if packet.stream_id().queue_id == VarInt::ZERO {
            self.zero.handle_stream_packet(remote_address, ecn, packet);
        } else {
            self.non_zero
                .handle_stream_packet(remote_address, ecn, packet);
        }
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        tag: stream::Tag,
        id: stream::Id,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        if id.queue_id == VarInt::ZERO {
            self.zero
                .dispatch_stream_packet(tag, id, credentials, segment);
        } else {
            self.non_zero
                .dispatch_stream_packet(tag, id, credentials, segment);
        }
    }

    #[inline]
    fn handle_datagram_packet(
        &mut self,
        remote_address: SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::datagram::decoder::Packet,
    ) {
        self.non_zero
            .handle_datagram_packet(remote_address, ecn, packet);
    }

    #[inline]
    fn dispatch_datagram_packet(
        &mut self,
        tag: packet::datagram::Tag,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        self.non_zero
            .dispatch_datagram_packet(tag, credentials, segment);
    }

    #[inline]
    fn handle_stale_key_packet(
        &mut self,
        packet: packet::secret_control::stale_key::Packet,
        remote_address: SocketAddress,
    ) {
        self.non_zero
            .handle_stale_key_packet(packet, remote_address);
    }

    #[inline]
    fn dispatch_stale_key_packet(
        &mut self,
        queue_id: Option<VarInt>,
        credentials: crate::credentials::Id,
        segment: descriptor::Filled,
    ) {
        self.non_zero
            .dispatch_stale_key_packet(queue_id, credentials, segment);
    }

    #[inline]
    fn handle_replay_detected_packet(
        &mut self,
        packet: packet::secret_control::replay_detected::Packet,
        remote_address: SocketAddress,
    ) {
        self.non_zero
            .handle_replay_detected_packet(packet, remote_address);
    }

    #[inline]
    fn dispatch_replay_detected_packet(
        &mut self,
        queue_id: Option<VarInt>,
        credentials: crate::credentials::Id,
        segment: descriptor::Filled,
    ) {
        self.non_zero
            .dispatch_replay_detected_packet(queue_id, credentials, segment);
    }

    #[inline]
    fn handle_unknown_path_secret_packet(
        &mut self,
        packet: packet::secret_control::unknown_path_secret::Packet,
        remote_address: SocketAddress,
    ) {
        self.non_zero
            .handle_unknown_path_secret_packet(packet, remote_address);
    }

    #[inline]
    fn dispatch_unknown_path_secret_packet(
        &mut self,
        queue_id: Option<VarInt>,
        credentials: crate::credentials::Id,
        segment: descriptor::Filled,
    ) {
        self.non_zero
            .dispatch_unknown_path_secret_packet(queue_id, credentials, segment);
    }

    #[inline]
    fn on_unhandled_packet(&mut self, remote_address: SocketAddress, packet: packet::Packet) {
        self.non_zero.on_unhandled_packet(remote_address, packet);
    }

    #[inline]
    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: SocketAddress,
        segment: descriptor::Filled,
    ) {
        self.non_zero
            .on_decode_error(error, remote_address, segment);
    }
}
