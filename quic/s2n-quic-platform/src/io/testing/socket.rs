// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    message::Message,
    network::{Buffers, HostId},
};
use crate::{
    features::Gso,
    socket::{
        ring, task,
        task::{rx, tx},
    },
    syscall::SocketEvents,
};
use core::task::Context;
use std::{io, sync::Arc};

/// A task to receive on a socket
pub async fn rx(socket: Socket, producer: ring::Producer<Message>) -> io::Result<()> {
    let result = task::Receiver::new(producer, socket, Default::default()).await;
    if let Some(err) = result {
        Err(err)
    } else {
        Ok(())
    }
}

/// A task to send on a socket
pub async fn tx(socket: Socket, consumer: ring::Consumer<Message>, gso: Gso) -> io::Result<()> {
    let result = task::Sender::new(consumer, socket, gso, Default::default()).await;
    if let Some(err) = result {
        Err(err)
    } else {
        Ok(())
    }
}

#[derive(Clone)]
pub struct Socket(Arc<State>);

impl Socket {
    pub(super) fn new(buffers: Buffers, host: HostId) -> Self {
        Self(Arc::new(State { buffers, host }))
    }

    /// Returns the current local address
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.0.buffers.lookup_addr(self.0.host)
    }

    /// Rebinds the address to a new address
    pub fn rebind(&self, addr: std::net::SocketAddr) {
        self.0.buffers.rebind(self.0.host, addr);
    }
}

struct State {
    host: HostId,
    buffers: Buffers,
}

impl Drop for State {
    fn drop(&mut self) {
        self.buffers.close_host(self.host);
    }
}

impl tx::Socket<Message> for Socket {
    type Error = io::Error;

    #[inline]
    fn send(
        &mut self,
        _cx: &mut Context,
        entries: &mut [Message],
        events: &mut tx::Events,
    ) -> io::Result<()> {
        self.0.buffers.tx_host(self.0.host, |queue| {
            let count = queue.send(entries);
            events.on_complete(count);
        })
    }
}

impl rx::Socket<Message> for Socket {
    type Error = io::Error;

    #[inline]
    fn recv(
        &mut self,
        cx: &mut Context,
        entries: &mut [Message],
        events: &mut rx::Events,
    ) -> io::Result<()> {
        self.0.buffers.rx_host(self.0.host, |queue| {
            let count = queue.recv(cx, entries);
            if count > 0 {
                events.on_complete(count);
            } else {
                events.blocked()
            }
        })
    }
}
