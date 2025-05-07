// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{self, Credentials},
    packet,
    socket::recv::descriptor as desc,
    sync::ring_deque,
};
use s2n_quic_core::{inet::SocketAddress, varint::VarInt};
use tracing::debug;

mod descriptor;
mod free_list;
mod handle;
mod keys;
mod pool;
mod probes;
mod queue;
mod sender;

#[cfg(test)]
mod tests;

/// Allocate this many channels at a time
///
/// With `debug_assertions`, we allocate smaller pages to try and cover more
/// branches in the allocator logic around growth.
const PAGE_SIZE: usize = if cfg!(debug_assertions) { 8 } else { 256 };

pub type Error = queue::Error;
pub type Control = handle::Control<desc::Filled, Credentials>;
pub type Stream = handle::Stream<desc::Filled, Credentials>;

/// A queue allocator for registering a receiver to process packets
/// for a given ID.
#[derive(Clone)]
pub struct Allocator {
    pool: pool::Pool<desc::Filled, Credentials, PAGE_SIZE>,
}

impl Allocator {
    pub fn new(
        stream_capacity: impl Into<ring_deque::Capacity>,
        control_capacity: impl Into<ring_deque::Capacity>,
    ) -> Self {
        Self {
            pool: pool::Pool::new(
                VarInt::ZERO,
                stream_capacity.into(),
                control_capacity.into(),
            ),
        }
    }

    /// Creates an allocator with a non-zero queue id
    ///
    /// This is used for patterns where the `queue_id=0` is special and used to
    /// indicate newly initialized flows waiting to be assigned. For example,
    /// a client sends a packet with `queue_id=0` to a server and waits for the
    /// server to respond with an actual `queue_id` for future packets from the client.
    pub fn new_non_zero(
        stream_capacity: impl Into<ring_deque::Capacity>,
        control_capacity: impl Into<ring_deque::Capacity>,
    ) -> Self {
        Self {
            pool: pool::Pool::new(
                VarInt::from_u8(1),
                stream_capacity.into(),
                control_capacity.into(),
            ),
        }
    }

    #[inline]
    pub fn dispatcher(&self) -> Dispatch {
        Dispatch {
            senders: self.pool.senders(),
            keys: self.pool.keys(),
            is_open: true,
        }
    }

    #[inline]
    pub fn alloc(&self, key: Option<&Credentials>) -> Option<(Control, Stream)> {
        self.pool.alloc(key)
    }

    #[inline]
    pub fn alloc_or_grow(&mut self, key: Option<&Credentials>) -> (Control, Stream) {
        self.pool.alloc_or_grow(key)
    }
}

/// A dispatcher which routes packets to the specified queue, if
/// there is a registered receiver.
#[derive(Clone)]
pub struct Dispatch {
    senders: sender::Senders<desc::Filled, Credentials, PAGE_SIZE>,
    keys: keys::Keys<Credentials>,
    is_open: bool,
}

impl Dispatch {
    #[inline]
    pub fn send_control(
        &mut self,
        queue_id: VarInt,
        segment: desc::Filled,
    ) -> Result<Option<desc::Filled>, Error> {
        let payload_len = segment.len();
        let mut res = Err(Error::Unallocated);
        self.senders.lookup(queue_id, |sender| {
            res = sender.send_control(segment);
        });

        match &res {
            Ok(prev) => {
                tracing::trace!(
                    %queue_id,
                    payload_len,
                    overflow = prev.is_some(),
                    "send_control"
                );
            }
            Err(error) => {
                if matches!(error, Error::Closed) {
                    self.is_open = false;
                }
                // TODO increment metrics
                debug!(%queue_id, "unroutable control packet");
            }
        }

        res
    }

    #[inline]
    pub fn send_stream(
        &mut self,
        queue_id: VarInt,
        segment: desc::Filled,
    ) -> Result<Option<desc::Filled>, Error> {
        let payload_len = segment.len();
        let mut res = Err(Error::Unallocated);
        self.senders.lookup(queue_id, |sender| {
            res = sender.send_stream(segment);
        });

        match &res {
            Ok(prev) => {
                tracing::trace!(
                    %queue_id,
                    payload_len,
                    overflow = prev.is_some(),
                    "send_stream"
                );
            }
            Err(error) => {
                if matches!(error, Error::Closed) {
                    self.is_open = false;
                }
                // TODO increment metrics
                debug!(%queue_id, "unroutable stream packet");
            }
        }

        res
    }

    #[inline]
    pub fn queue_id_for_key(&self, key: &Credentials) -> Option<VarInt> {
        self.keys.get(key)
    }
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
        _credentials: credentials::Credentials,
        segment: desc::Filled,
    ) {
        let Some(id) = id else {
            return;
        };

        let _ = self.send_control(id.queue_id, segment);
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
        _credentials: credentials::Credentials,
        segment: desc::Filled,
    ) {
        let _ = self.send_stream(id.queue_id, segment);
    }

    #[inline]
    fn handle_replay_detected_packet(
        &mut self,
        packet: packet::secret_control::replay_detected::Packet,
        remote_address: SocketAddress,
    ) {
        // TODO reset the destination queue - currently secret control packets don't have queue_ids
        let _ = packet;
        let _ = remote_address;
    }

    #[inline]
    fn handle_stale_key_packet(
        &mut self,
        packet: packet::secret_control::stale_key::Packet,
        remote_address: SocketAddress,
    ) {
        // TODO reset the destination queue - currently secret control packets don't have queue_ids
        let _ = packet;
        let _ = remote_address;
    }

    #[inline]
    fn handle_unknown_path_secret_packet(
        &mut self,
        packet: packet::secret_control::unknown_path_secret::Packet,
        remote_address: SocketAddress,
    ) {
        // TODO reset the destination queue - currently secret control packets don't have queue_ids
        let _ = packet;
        let _ = remote_address;
    }
}
