// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! dcQUIC streams over UDP require forwarding incoming streams from a central 'handshake' port to
//! pooled/shared per-stream sockets. This contains the tracking data structure supporting that
//! forwarding.

use crate::{credentials, msg::recv};
use core::task::{Context, Poll};
use s2n_quic_core::varint::VarInt;
use std::sync::{Arc, Weak};
use tokio::sync::mpsc;

type Sender = mpsc::Sender<recv::Message>;
type ReceiverChan = mpsc::Receiver<recv::Message>;
type Key = (credentials::Id, VarInt);
type HashMap = flurry::HashMap<Key, Sender>;

pub struct Map {
    inner: Arc<HashMap>,
    next: Option<(Sender, ReceiverChan)>,
    channel_size: usize,
}

impl Default for Map {
    #[inline]
    fn default() -> Self {
        Self {
            inner: Default::default(),
            next: None,
            channel_size: 15,
        }
    }
}

impl Map {
    /// Hands `msg` to the stream that already owns `packet`'s credentials, if there is one.
    ///
    /// Returns `true` if the credentials were already claimed, meaning `msg` has been consumed -
    /// forwarded to the owning stream, or dropped if its channel was full or closed.
    ///
    /// Call this before [`Self::claim`], so only packets claiming a vacant slot get authenticated.
    /// The acceptor must not authenticate a packet belonging to an existing stream: that stream
    /// authenticates it itself, and running the replay check twice for one `key_id` would reject
    /// the peer's own packet.
    #[inline]
    pub(crate) fn try_forward(
        &mut self,
        packet: &super::InitialPacket,
        msg: &mut recv::Message,
    ) -> bool {
        let key = (packet.credentials.id, packet.credentials.key_id);

        let guard = self.inner.guard();
        let Some(sender) = self.inner.get(&key, &guard) else {
            return false;
        };

        tracing::trace!(action = "forward", credentials = ?&key);
        if let Err(err) = sender.try_send(msg.take()) {
            match err {
                mpsc::error::TrySendError::Closed(_) => {
                    // remove the channel from the map since we're closed
                    self.inner.remove(&key, &guard);
                    tracing::debug!(credentials = ?key, error = "channel_closed");
                }
                mpsc::error::TrySendError::Full(_) => {
                    // drop the packet
                    let _ = msg;
                    tracing::debug!(credentials = ?key, error = "channel_full");
                }
            }
        }

        // `msg` was taken either way, so the caller must not try to reuse it
        true
    }

    /// Claims the routing slot for `packet`'s credentials, returning the receiving half of the new
    /// stream's forwarding channel.
    ///
    /// Only call this once `packet` has authenticated: the slot routes the credentials' future
    /// packets, and whoever holds it also fixes the stream's peer address.
    ///
    /// Returns `None` if the slot is already taken, which no caller can observe today - each
    /// acceptor owns its map and inserts only from its own task, and the only other writer is
    /// [`Receiver::drop`], which removes. If it ever does happen the packet must be dropped rather
    /// than forwarded, since it was authenticated against this caller's keys and the holder's
    /// replay check would reject it.
    #[inline]
    pub(crate) fn claim(&mut self, packet: &super::InitialPacket) -> Option<Receiver> {
        let (sender, receiver) = self
            .next
            .take()
            .unwrap_or_else(|| mpsc::channel(self.channel_size));

        let key = (packet.credentials.id, packet.credentials.key_id);

        let guard = self.inner.guard();
        match self.inner.try_insert(key, sender, &guard) {
            Ok(_) => {
                drop(guard);
                let map = Arc::downgrade(&self.inner);
                tracing::trace!(action = "register", credentials = ?&key);
                let receiver = ReceiverState {
                    map,
                    key,
                    channel: receiver,
                };
                Some(Receiver(Box::new(receiver)))
            }
            Err(err) => {
                // recycle the channel we didn't end up needing
                self.next = Some((err.not_inserted, receiver));
                tracing::debug!(credentials = ?key, error = "slot_already_claimed");
                None
            }
        }
    }
}

#[derive(Debug)]
pub struct Receiver(Box<ReceiverState>);

#[derive(Debug)]
struct ReceiverState {
    map: Weak<HashMap>,
    key: Key,
    channel: ReceiverChan,
}

impl Receiver {
    #[inline]
    pub fn poll_recv(&mut self, cx: &mut Context) -> Poll<Option<recv::Message>> {
        self.0.channel.poll_recv(cx)
    }
}

impl Drop for Receiver {
    #[inline]
    fn drop(&mut self) {
        if let Some(map) = self.0.map.upgrade() {
            tracing::trace!(action = "unregister", credentials = ?&self.0.key);
            let _ = map.remove(&self.0.key, &map.guard());
        }
    }
}
