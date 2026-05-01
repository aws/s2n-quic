// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{self, Credentials},
    packet,
    socket::pool::descriptor as desc,
    sync::{mpsc, ring_deque},
};
use s2n_quic_core::{inet::SocketAddress, varint::VarInt};

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

pub type Error<T = desc::Filled> = queue::Error<T>;
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
    pub fn dispatcher(&self, on_unroutable: mpsc::Sender<desc::Filled>) -> Dispatch {
        Dispatch {
            senders: self.pool.senders(),
            keys: self.pool.keys(),
            on_unroutable,
            is_open: true,
        }
    }

    #[inline]
    pub fn alloc(&self, key: &Credentials) -> Option<(Control, Stream)> {
        self.pool.alloc(key)
    }

    #[inline]
    pub fn alloc_or_grow(&mut self, key: &Credentials) -> (Control, Stream) {
        self.pool.alloc_or_grow(key)
    }
}

/// A dispatcher which routes packets to the specified queue, if
/// there is a registered receiver.
#[derive(Clone)]
pub struct Dispatch {
    senders: sender::Senders<desc::Filled, Credentials, PAGE_SIZE>,
    keys: keys::Keys<Credentials>,
    on_unroutable: mpsc::Sender<desc::Filled>,
    is_open: bool,
}

impl Dispatch {
    #[inline]
    pub fn send_control(
        &mut self,
        queue_id: VarInt,
        credentials: Option<&Credentials>,
        segment: desc::Filled,
    ) -> Result<(), Error<()>> {
        let payload_len = segment.len();
        let res = self.senders.lookup(queue_id, segment, |sender, segment| {
            let key = sender.key();
            if credentials.is_some() && key.as_ref() != credentials {
                tracing::debug!(%queue_id, expected = %credentials.unwrap(), actual = ?key, space = "control", "credential mismatch");
                return Err(Error::Unallocated(segment));
            }

            sender.send_control(segment)
        });

        match res {
            Ok(prev) => {
                // TODO add overflow event for metrics
                tracing::trace!(
                    %queue_id,
                    payload_len,
                    overflow = prev.is_some(),
                    "send_control"
                );
                Ok(())
            }
            Err(Error::Closed) => {
                self.is_open = false;
                Err(queue::Error::Closed)
            }
            Err(Error::Unallocated(segment)) => {
                tracing::debug!(remote_addr = %segment.remote_address().get(), "unroutable packet");

                let _ = self.on_unroutable.send_back(segment);
                Err(queue::Error::Unallocated(()))
            }
        }
    }

    #[inline]
    pub fn send_stream(
        &mut self,
        queue_id: VarInt,
        credentials: Option<&Credentials>,
        segment: desc::Filled,
    ) -> Result<(), Error<()>> {
        let payload_len = segment.len();
        let res = self.senders.lookup(queue_id, segment, |sender, segment| {
            let key = sender.key();
            if credentials.is_some() && key.as_ref() != credentials {
                tracing::debug!(%queue_id, expected = %credentials.unwrap(), actual = ?key, space = "stream", "credential mismatch");
                return Err(Error::Unallocated(segment));
            }

            sender.send_stream(segment)
        });

        match res {
            Ok(prev) => {
                // TODO add overflow event for metrics
                tracing::trace!(
                    %queue_id,
                    payload_len,
                    overflow = prev.is_some(),
                    "send_stream"
                );
                Ok(())
            }
            Err(Error::Closed) => {
                self.is_open = false;
                Err(queue::Error::Closed)
            }
            Err(Error::Unallocated(segment)) => {
                tracing::debug!(remote_addr = %segment.remote_address().get(), "unroutable packet");

                let _ = self.on_unroutable.send_back(segment);
                Err(queue::Error::Unallocated(()))
            }
        }
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
        _packet: packet::control::decoder::Packet<&mut [u8]>,
    ) {
    }

    #[inline]
    fn dispatch_control_packet(&mut self, packet: packet::control::decoder::Packet<desc::Filled>) {
        let Some(id) = packet.stream_id().copied() else {
            return;
        };

        let credentials = *packet.credentials();
        let segment = packet.into_parts().1;
        let _ = self.send_control(id.queue_id, Some(&credentials), segment);
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
        let _ = self.send_stream(id.queue_id, Some(&credentials), segment);
    }

    #[inline(always)]
    fn handle_flow_reset_packet(
        &mut self,
        _remote_address: SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::secret_control::flow_reset::Packet,
    ) {
    }

    #[inline]
    fn dispatch_flow_reset_packet(
        &mut self,
        _tag: packet::secret_control::flow_reset::Tag,
        queue_id: VarInt,
        credentials: Credentials,
        trigger: packet::secret_control::flow_reset::Trigger,
        segment: desc::Filled,
    ) {
        let payload_len = segment.len();
        let res = self.senders.lookup(queue_id, segment, |sender, segment| {
            let key = sender.key();
            if key != Some(credentials) {
                return Err(Error::Unallocated(segment));
            }

            // Route FlowReset to the correct queue based on what triggered it:
            // - Stream trigger → send worker needs it → control queue
            // - Control trigger → recv worker needs it → stream queue
            match trigger {
                packet::secret_control::flow_reset::Trigger::Stream => sender.send_control(segment),
                packet::secret_control::flow_reset::Trigger::Control => sender.send_stream(segment),
            }
        });

        match res {
            Ok(prev) => {
                // TODO add overflow event for metrics
                tracing::trace!(
                    %queue_id,
                    payload_len,
                    overflow = prev.is_some(),
                    ?trigger,
                    "dispatch_flow_reset"
                );
            }
            Err(Error::Closed) => {
                self.is_open = false;
            }
            Err(Error::Unallocated(segment)) => {
                // Don't route these since they are sent in response to unroutable packets
                let _ = segment;
            }
        }
    }

    /// implement this so we don't get warnings about not handling it
    #[inline]
    fn handle_replay_detected_packet(
        &mut self,
        _packet: packet::secret_control::replay_detected::Packet,
        _remote_address: SocketAddress,
    ) {
    }

    #[inline]
    fn dispatch_replay_detected_packet(
        &mut self,
        queue_id: Option<VarInt>,
        _credentials: credentials::Id,
        segment: desc::Filled,
    ) {
        let Some(queue_id) = queue_id else {
            return;
        };
        let _ = self.send_control(queue_id, None, segment);
    }

    /// implement this so we don't get warnings about not handling it
    #[inline]
    fn handle_stale_key_packet(
        &mut self,
        _packet: packet::secret_control::stale_key::Packet,
        _remote_address: SocketAddress,
    ) {
    }

    #[inline]
    fn dispatch_stale_key_packet(
        &mut self,
        queue_id: Option<VarInt>,
        _credentials: credentials::Id,
        segment: desc::Filled,
    ) {
        let Some(queue_id) = queue_id else {
            return;
        };
        let _ = self.send_control(queue_id, None, segment);
    }

    #[inline]
    fn handle_unknown_path_secret_packet(
        &mut self,
        _packet: packet::secret_control::unknown_path_secret::Packet,
        _remote_address: SocketAddress,
    ) {
    }

    #[inline]
    fn dispatch_unknown_path_secret_packet(
        &mut self,
        queue_id: Option<VarInt>,
        _credentials: credentials::Id,
        segment: desc::Filled,
    ) {
        let Some(queue_id) = queue_id else {
            return;
        };
        let _ = self.send_control(queue_id, None, segment);
    }
}
