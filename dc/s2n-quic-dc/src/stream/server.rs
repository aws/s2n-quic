// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::type_complexity)]

use crate::{
    credentials::{self, Credentials},
    crypto::{open::Application as _, UninitSlice},
    msg::recv,
    packet,
    path::secret,
    stream::socket,
};
use s2n_codec::{DecoderBufferMut, DecoderError};
use s2n_quic_core::varint::VarInt;
use std::{io, net::SocketAddr};
use tracing::trace;
use zeroize::Zeroize as _;

pub mod accept;
pub mod application;
pub mod handshake;
pub mod manager;
pub mod stats;
pub mod tokio;
pub mod udp;

#[cfg(test)]
mod tests;

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
    #[expect(
        clippy::unwrap_used,
        reason = "VarInt::ZERO is provably in range for unreliable_unidirectional"
    )]
    pub fn empty() -> Self {
        Self {
            credentials: Credentials {
                id: credentials::Id::default(),
                key_id: VarInt::ZERO,
            },
            stream_id: packet::stream::Id::unreliable_unidirectional(VarInt::ZERO).unwrap(),
            source_queue_id: None,
            payload_len: 0,
            is_zero_offset: false,
            is_retransmission: false,
            is_fin: false,
            is_fin_known: false,
        }
    }
}

/// Verifies the first packet of a new stream before the acceptor creates any state for it.
///
/// A UDP acceptor picks the new stream's routing slot (the plaintext `(credential_id, key_id)`
/// pair) and its peer address (the datagram's source address) out of that packet, and neither can
/// be corrected once the stream exists. Both therefore have to wait on AEAD.
///
/// Owned by the acceptor rather than being a free function so the verification buffer is allocated
/// once instead of per packet.
#[derive(Debug, Default)]
pub(crate) struct Authenticator {
    /// Holds the packet copy and the plaintext sink back to back.
    scratch: Vec<u8>,
}

impl Authenticator {
    /// Returns `true` if `segment` is an authentic stream-opening packet for `crypto`'s
    /// credentials: it decodes as a stream packet, and its AEAD tag verifies under the keys derived
    /// for the `(credential_id, key_id)` named in its header.
    ///
    /// `false` means the sender does not hold the path secret those credentials belong to, so the
    /// acceptor must not open a stream for it.
    #[inline]
    pub(crate) fn authenticate_first_packet(
        &mut self,
        crypto: &secret::map::Bidirectional,
        segment: &[u8],
    ) -> bool {
        let Some(control) = crypto.control.as_ref() else {
            debug_assert!(
                false,
                "unreliable transports always derive stream control keys"
            );
            return false;
        };

        let tag_len = crypto.application.opener.tag_len();

        // The allocation is deliberately kept across calls, so reset the length first: the copy
        // below appends, and it has to land at offset 0 for the split to line up.
        //
        // That leaves one buffer in two halves: `candidate` is the copy `decrypt` may rewrite, and
        // `plaintext` is where it writes the decrypted payload. The payload is a subslice of the
        // packet, so the packet's length bounds both.
        let len = segment.len();
        self.scratch.clear();
        self.scratch.extend_from_slice(segment);
        self.scratch.resize(len * 2, 0);
        let (candidate, plaintext) = self.scratch.split_at_mut(len);

        let decoder = DecoderBufferMut::new(candidate);
        let is_authentic = match decoder.decode_parameterized(tag_len) {
            Ok((packet::Packet::Stream(mut packet), _remaining)) => {
                let payload_len = packet.payload().len();
                packet
                    .decrypt(
                        &crypto.application.opener,
                        &control.opener,
                        UninitSlice::new(&mut plaintext[..payload_len]),
                    )
                    .is_ok()
            }
            // anything that doesn't decode as a stream packet can't claim a stream
            _ => false,
        };

        // Don't leave the peer's plaintext sitting in the buffer. `Vec::zeroize` zeroes the full
        // capacity and empties the vec, so the allocation survives for the next call.
        self.scratch.zeroize();

        is_authentic
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

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(5);

    for iteration in 0..10_000 {
        if start.elapsed() >= timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "could not find free port after 5 seconds",
            ));
        }

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

    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "could not find free port after 10,000 attempts",
    ))
}
