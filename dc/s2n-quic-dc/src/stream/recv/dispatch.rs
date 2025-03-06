// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{credentials, packet, path, socket::recv::descriptor as desc, sync::mpsc};
use s2n_quic_core::inet::SocketAddress;
use tracing::debug;

mod descriptor;
mod pool;
mod sender;

#[cfg(test)]
mod tests;

/// Allocate this many channels at a time
const PAGE_SIZE: usize = 256;

pub type Control = descriptor::Control<desc::Filled>;
pub type Stream = descriptor::Stream<desc::Filled>;
pub type StreamSender = descriptor::StreamSender<desc::Filled>;

#[derive(Clone)]
pub struct Allocator {
    pool: pool::Pool<desc::Filled, PAGE_SIZE>,
    map: path::secret::Map,
}

impl Allocator {
    pub fn new(
        map: path::secret::Map,
        stream_capacity: impl Into<mpsc::Capacity>,
        control_capacity: impl Into<mpsc::Capacity>,
    ) -> Self {
        Self {
            pool: pool::Pool::new(stream_capacity.into(), control_capacity.into()),
            map,
        }
    }

    #[inline]
    pub fn dispatcher(&self) -> Dispatch {
        Dispatch {
            senders: self.pool.senders(),
            map: self.map.clone(),
            is_open: true,
        }
    }

    #[inline]
    pub fn alloc(&mut self) -> Option<(Control, Stream)> {
        self.pool.alloc()
    }

    #[inline]
    pub fn alloc_or_grow(&mut self) -> (Control, Stream) {
        self.pool.alloc_or_grow()
    }
}

#[derive(Clone)]
pub struct Dispatch {
    senders: sender::Senders<desc::Filled, PAGE_SIZE>,
    map: path::secret::Map,
    is_open: bool,
}

impl crate::socket::recv::router::Router for Dispatch {
    #[inline(always)]
    fn is_open(&self) -> bool {
        self.is_open
    }

    #[inline(always)]
    fn tag_len(&self) -> usize {
        16
    }

    /// implement this so we don't get warnings about not handling it
    #[inline(always)]
    fn handle_control_packet(
        &mut self,
        _remote_address: SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::control::decoder::Packet,
    ) {
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        _tag: packet::control::Tag,
        id: Option<packet::stream::Id>,
        credentials: credentials::Credentials,
        segment: desc::Filled,
    ) {
        let Some(id) = id else {
            return;
        };

        let mut did_send = false;
        let mut prev = None;
        self.senders.lookup(id.queue_id, |sender| {
            did_send = true;
            match sender.control.send_back(segment) {
                Ok(new_prev) => {
                    // drop the previous segment outside of the lookup call
                    prev = new_prev;
                }
                Err(_) => {
                    // if any channels are closed then the whole thing is dropped
                    self.is_open = false;
                }
            }
        });

        if !did_send {
            // TODO increment metrics
            debug!(stream_id = ?id, ?credentials, "unroutable control packet");
            return;
        }

        if prev.is_some() {
            // TODO increment metrics
            debug!(queue_id = %id.queue_id, "control queue overflow");
        }
    }

    /// implement this so we don't get warnings about not handling it
    #[inline(always)]
    fn handle_stream_packet(
        &mut self,
        _remote_address: SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::stream::decoder::Packet,
    ) {
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        _tag: packet::stream::Tag,
        id: packet::stream::Id,
        credentials: credentials::Credentials,
        segment: desc::Filled,
    ) {
        let mut did_send = false;
        let mut prev = None;
        self.senders.lookup(id.queue_id, |sender| {
            did_send = true;
            match sender.stream.send_back(segment) {
                Ok(new_prev) => {
                    // drop the previous segment outside of the lookup call
                    prev = new_prev;
                }
                Err(_) => {
                    // if any channels are closed then the whole thing is dropped
                    self.is_open = false;
                }
            }
        });

        if !did_send {
            // TODO increment metrics
            debug!(stream_id = ?id, ?credentials, "unroutable stream packet");
            return;
        }

        if prev.is_some() {
            // TODO increment metrics
            debug!(queue_id = %id.queue_id, "stream queue overflow");
        }
    }

    #[inline]
    fn handle_stale_key_packet(
        &mut self,
        packet: packet::secret_control::stale_key::Packet,
        remote_address: SocketAddress,
    ) {
        self.map
            .handle_control_packet(&packet.into(), &remote_address.into());
    }

    #[inline]
    fn handle_replay_detected_packet(
        &mut self,
        packet: packet::secret_control::replay_detected::Packet,
        remote_address: SocketAddress,
    ) {
        self.map
            .handle_control_packet(&packet.into(), &remote_address.into());
    }

    #[inline]
    fn handle_unknown_path_secret_packet(
        &mut self,
        packet: packet::secret_control::unknown_path_secret::Packet,
        remote_address: SocketAddress,
    ) {
        self.map
            .handle_control_packet(&packet.into(), &remote_address.into());
    }

    #[inline(always)]
    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: SocketAddress,
        segment: desc::Filled,
    ) {
        tracing::warn!(
            ?error,
            ?remote_address,
            packet_len = segment.len(),
            "failed to decode packet"
        );
    }
}
