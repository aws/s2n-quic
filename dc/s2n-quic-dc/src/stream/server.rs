// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::type_complexity)]

use crate::{
    credentials::{self, Credentials},
    msg::recv,
    packet,
    stream::socket,
};
use s2n_codec::{DecoderBufferMut, DecoderError};
use s2n_quic_core::varint::VarInt;
use std::{io, net::SocketAddr};
use tracing::trace;

pub mod accept;
pub mod application;
pub mod handshake;
pub mod manager;
pub mod stats;
pub mod tokio;
pub mod udp;

#[derive(Clone, Copy, Debug)]
pub struct InitialPacket {
    pub credentials: Credentials,
    pub stream_id: packet::stream::Id,
    pub source_queue_id: Option<VarInt>,
    pub payload_len: usize,
    pub is_zero_offset: bool,
    pub is_retransmission: bool,
    pub is_fin: bool,
    pub is_fin_known: bool,
}

impl InitialPacket {
    #[inline]
    pub fn peek(recv: &mut recv::Message, tag_len: usize) -> Result<Self, DecoderError> {
        let segment = recv
            .peek_segments()
            .next()
            .ok_or(DecoderError::UnexpectedEof(1))?;

        let decoder = DecoderBufferMut::new(segment);
        // we're just going to assume that all of the packets in this datagram
        // pertain to the same stream
        let (packet, _remaining) = decoder.decode_parameterized(tag_len)?;

        let packet::Packet::Stream(packet) = packet else {
            return Err(DecoderError::InvariantViolation("unexpected packet type"));
        };

        let packet: InitialPacket = packet.into();

        Ok(packet)
    }

    #[inline]
    pub fn empty() -> Self {
        Self {
            credentials: Credentials {
                id: credentials::Id::default(),
                key_id: VarInt::ZERO,
            },
            stream_id: packet::stream::Id {
                queue_id: VarInt::ZERO,
                is_bidirectional: false,
                is_reliable: false,
            },
            source_queue_id: None,
            payload_len: 0,
            is_zero_offset: false,
            is_retransmission: false,
            is_fin: false,
            is_fin_known: false,
        }
    }
}

impl<'a> From<packet::stream::decoder::Packet<'a>> for InitialPacket {
    #[inline]
    fn from(packet: packet::stream::decoder::Packet<'a>) -> Self {
        let credentials = *packet.credentials();
        let stream_id = *packet.stream_id();
        let source_queue_id = packet.source_queue_id();
        let payload_len = packet.payload().len();
        let is_zero_offset = packet.stream_offset().as_u64() == 0;
        let is_retransmission = packet.is_retransmission();
        let is_fin = packet.is_fin();
        let is_fin_known = packet.final_offset().is_some();
        Self {
            credentials,
            stream_id,
            source_queue_id,
            is_zero_offset,
            payload_len,
            is_retransmission,
            is_fin,
            is_fin_known,
        }
    }
}

pub(crate) fn spawn_initial_wildcard_pair(
    local_addr: SocketAddr,
    socket_opts: impl Fn(SocketAddr) -> socket::Options,
) -> io::Result<(SocketAddr, std::net::UdpSocket, std::net::TcpListener)> {
    debug_assert_eq!(local_addr.port(), 0);

    for iteration in 0..10 {
        trace!(wildcard_search_iteration = iteration);
        let udp_socket = socket_opts(local_addr).build_udp()?;
        let candidate_addr = udp_socket.local_addr()?;
        trace!(candidate = %candidate_addr);
        match socket_opts(candidate_addr).build_tcp_listener() {
            Ok(tcp_socket) => {
                trace!(selected = %candidate_addr);
                return Ok((candidate_addr, udp_socket, tcp_socket));
            }
            Err(err) if err.kind() == io::ErrorKind::AddrInUse => continue,
            Err(err) => return Err(err),
        }
    }

    Err(io::ErrorKind::AddrInUse.into())
}
