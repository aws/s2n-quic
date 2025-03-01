// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{credentials, msg::recv};
use core::task::{Context, Poll};
use std::sync::{Arc, Weak};
use tokio::sync::mpsc;

type Sender = mpsc::Sender<recv::Message>;
type ReceiverChan = mpsc::Receiver<recv::Message>;
type Key = credentials::Id;
type HashMap = flurry::HashMap<Key, Sender>;

pub enum Outcome {
    Forwarded,
    Created { receiver: Receiver },
}

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
    #[inline]
    pub fn handle(&mut self, packet: &super::InitialPacket, msg: &mut recv::Message) -> Outcome {
        let (sender, receiver) = self
            .next
            .take()
            .unwrap_or_else(|| mpsc::channel(self.channel_size));

        let key = packet.credentials.id;

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
                let receiver = Receiver(Box::new(receiver));
                Outcome::Created { receiver }
            }
            Err(err) => {
                self.next = Some((err.not_inserted, receiver));

                tracing::trace!(action = "forward", credentials = ?&key);
                if let Err(err) = err.current.try_send(msg.take()) {
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

                Outcome::Forwarded
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
