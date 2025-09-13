// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::Message as _;
use core::task::{Context, Waker};
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    path::{LocalAddress, MaxMtu, Tuple},
};
use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::{
        atomic::{AtomicU16, AtomicU32, Ordering},
        Arc, Mutex,
    },
};
use tracing::{debug, debug_span, trace};

pub type PathHandle = Tuple;

pub trait Network {
    fn execute(&mut self, buffers: &Buffers) -> usize;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HostId(u64);

impl<A: Network, B: Network> Network for (A, B) {
    fn execute(&mut self, buffers: &Buffers) -> usize {
        let mut result = 0;
        result += self.0.execute(buffers);
        result += self.1.execute(buffers);
        result
    }
}

#[derive(Clone, Debug)]
pub struct Buffers {
    inner: Arc<Mutex<State>>,
    next_ip: Arc<AtomicU32>,
    next_port: Arc<AtomicU16>,
}

impl Default for Buffers {
    fn default() -> Self {
        Self {
            inner: Default::default(),
            next_ip: Arc::new(AtomicU32::new(u32::from_be_bytes([1, 0, 0, 0]))),
            //= https://www.rfc-editor.org/rfc/rfc6335#section-6
            //# o  the Dynamic Ports, also known as the Private or Ephemeral Ports,
            //#    from 49152-65535 (never assigned)
            next_port: Arc::new(AtomicU16::new(49152)),
        }
    }
}

impl Buffers {
    pub fn close(&self) {
        if let Ok(mut lock) = self.inner.lock() {
            lock.is_open = false;

            let state = &mut *lock;

            for entry in state.tx.values_mut().chain(state.rx.values_mut()) {
                if let Some(waker) = entry.waker.take() {
                    waker.wake();
                }
            }
        }
    }

    pub fn lookup_addr(&self, host: HostId) -> io::Result<std::net::SocketAddr> {
        let lock = self
            .inner
            .lock()
            .map_err(|err| io::Error::other(err.to_string()))?;

        let entries = lock.host_to_addr.get(&host).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("host {host:?} was not found"),
            )
        })?;

        let addr = *entries.first().unwrap();

        Ok(addr.into())
    }

    pub fn rebind(&self, host: HostId, addr: std::net::SocketAddr) {
        if let Ok(mut lock) = self.inner.lock() {
            let addr = addr.into();
            // can't rebind to an already used address
            if lock.addr_to_host.contains_key(&addr) {
                return;
            }

            lock.addr_to_host.insert(addr, host);
            let host_to_addr = lock.host_to_addr.get_mut(&host).unwrap();
            let prev = host_to_addr.pop().unwrap();
            host_to_addr.push(addr);

            lock.addr_to_host.remove(&prev);

            lock.tx.get_mut(&host).unwrap().local_address = addr.into();
            lock.rx.get_mut(&host).unwrap().local_address = addr.into();

            debug!("rebind {prev} -> {addr}");
        }
    }

    pub fn tx<F: FnOnce(&mut Queue)>(&self, handle: SocketAddress, f: F) {
        if let Ok(mut lock) = self.inner.lock() {
            let lock = &mut *lock;
            if let Some(host) = lock.addr_to_host.get(&handle) {
                if let Some(queue) = lock.tx.get_mut(host) {
                    f(queue)
                }
            }
        }
    }

    pub fn tx_host<F: FnOnce(&mut Queue)>(&self, host: HostId, f: F) -> io::Result<()> {
        let mut lock = self
            .inner
            .lock()
            .map_err(|err| io::Error::other(err.to_string()))?;

        if !lock.is_open {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "network is closed",
            ));
        }

        let lock = &mut *lock;
        if let Some(queue) = lock.tx.get_mut(&host) {
            f(queue)
        }

        Ok(())
    }

    pub fn rx<F: FnOnce(&mut Queue)>(&self, handle: SocketAddress, f: F) {
        if let Ok(mut lock) = self.inner.lock() {
            let lock = &mut *lock;
            if let Some(host) = lock.addr_to_host.get(&handle) {
                if let Some(queue) = lock.rx.get_mut(host) {
                    f(queue)
                }
            }
        }
    }

    pub fn rx_host<F: FnOnce(&mut Queue)>(&self, host: HostId, f: F) -> io::Result<()> {
        let mut lock = self
            .inner
            .lock()
            .map_err(|err| io::Error::other(err.to_string()))?;

        if !lock.is_open {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "host is closed",
            ));
        }

        let lock = &mut *lock;
        if let Some(queue) = lock.rx.get_mut(&host) {
            f(queue)
        }

        Ok(())
    }

    pub fn pending_transmission<F: FnMut(&Packet)>(&self, mut f: F) {
        if let Ok(lock) = self.inner.lock() {
            for queue in lock.tx.values() {
                for packet in &queue.packets {
                    f(packet);
                }
            }
        }
    }

    pub fn drain_pending_transmissions<F: FnMut(Packet) -> Result<(), ()>>(&self, mut f: F) {
        let mut lock = if let Ok(lock) = self.inner.lock() {
            lock
        } else {
            return;
        };

        let mut queues = vec![];
        let mut to_remove = vec![];

        // find all of the queues with at least one packet to transmit
        for (host, queue) in lock.tx.iter_mut() {
            if queue.packets.is_empty() {
                continue;
            }

            queues.push((*host, queue));
        }

        // shuffle the queue so each endpoint has a fair chance of transmitting
        super::rand::shuffle(&mut queues);

        'done: loop {
            let mut has_result = false;
            for (host, queue) in &mut queues {
                // transmit a single packet at a time per queue so they are fairly
                // transmitted
                if let Some(packet) = queue.packets.pop_front() {
                    let result = f(packet);
                    has_result = true;

                    // notify the endpoint that it can send now
                    if let Some(waker) = queue.waker.take() {
                        waker.wake();
                    }

                    if result.is_err() {
                        break 'done;
                    }
                } else if !queue.is_open {
                    // if the queue is both closed and empty, then remove it
                    to_remove.push(*host);
                }
            }

            // if all of the queues are empty then just return
            if !has_result {
                break 'done;
            }
        }

        // clean up any queues that are closed and empty
        for host in to_remove {
            lock.tx.remove(&host);
        }
    }

    pub fn execute<N: Network>(&self, n: &mut N) {
        n.execute(self);
    }

    /// Generate a unique address
    pub fn generate_addr(&self) -> SocketAddress {
        let ip = self
            .next_ip
            .fetch_add(1, Ordering::SeqCst)
            .to_be_bytes()
            .into();
        let port = self.next_port.fetch_add(1, Ordering::SeqCst);
        let addr = (ip, port);
        SocketAddress::IpV4(addr.into())
    }

    pub fn close_host(&mut self, host: HostId) {
        if let Ok(mut lock) = self.inner.lock() {
            lock.close_host(host)
        }
    }

    /// Register an address on the network
    pub fn register(&self, handle: SocketAddress, max_mtu: MaxMtu) -> super::socket::Socket {
        let mut lock = self.inner.lock().unwrap();

        let host = HostId(lock.next_host);
        lock.next_host += 1;

        let queue = Queue::new(handle, max_mtu.into());

        lock.addr_to_host.insert(handle, host);
        lock.host_to_addr.insert(host, vec![handle]);
        lock.tx.insert(host, queue.clone());
        lock.rx.insert(host, queue);

        super::socket::Socket::new(self.clone(), host)
    }
}

#[derive(Debug)]
struct State {
    is_open: bool,
    next_host: u64,
    addr_to_host: HashMap<SocketAddress, HostId>,
    host_to_addr: HashMap<HostId, Vec<SocketAddress>>,
    tx: HashMap<HostId, Queue>,
    rx: HashMap<HostId, Queue>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            is_open: true,
            next_host: 0,
            addr_to_host: Default::default(),
            host_to_addr: Default::default(),
            tx: Default::default(),
            rx: Default::default(),
        }
    }
}

impl State {
    pub fn close_host(&mut self, host: HostId) {
        tracing::trace!(closing = ?host);

        if let Some(tx) = self.tx.get_mut(&host) {
            // if we don't have any packets remaining, then remove the outgoing packets
            // immediately, otherwise mark it closed
            if tx.packets.is_empty() {
                self.tx.remove(&host);
            } else {
                tx.is_open = false;
            }
        }

        self.rx.remove(&host);

        if let Some(addrs) = self.host_to_addr.remove(&host) {
            for addr in addrs {
                tracing::trace!(closing = ?addr);
                self.addr_to_host.remove(&addr);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Queue {
    capacity: usize,
    mtu: u16,
    packets: VecDeque<Packet>,
    local_address: LocalAddress,
    waker: Option<Waker>,
    is_open: bool,
}

impl Queue {
    fn new(addr: SocketAddress, mtu: u16) -> Self {
        let local_address = addr.into();
        Self {
            capacity: 1024,
            mtu,
            packets: VecDeque::new(),
            local_address,
            waker: None,
            is_open: true,
        }
    }

    pub fn enqueue(&mut self, packet: Packet) {
        let _span = debug_span!(
            "packet",
            dest = %packet.path.local_address.0,
            src = %packet.path.remote_address.0,
            len = packet.payload.len()
        )
        .entered();

        if self.packets.len() < self.capacity {
            // Only enqueue packets if we have capacity.
            //
            // This matches the behavior of existing UDP stacks.
            // See https://github.com/tokio-rs/turmoil/pull/128#issuecomment-1638584711
            self.packets.push_back(packet);
            trace!("packet::enqueue");
        } else {
            debug!("packet::enqueue::drop capacity={}", self.capacity);
        }

        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }

    pub fn dequeue(&mut self, count: usize) -> impl Iterator<Item = Packet> + '_ {
        let count = self.packets.len().min(count);
        self.packets.drain(..count)
    }

    pub fn drain(&mut self) -> impl Iterator<Item = Packet> + '_ {
        self.packets.drain(..)
    }

    pub fn send(&mut self, msgs: &[super::message::Message]) -> usize {
        // Only send what capacity we have left. Drop the rest.
        //
        // This matches the behavior of existing UDP stacks.
        // See https://github.com/tokio-rs/turmoil/pull/128#issuecomment-1638584711
        let accept_len = self
            .capacity
            .saturating_sub(self.packets.len())
            .min(msgs.len());

        let (accepted, dropped) = msgs.split_at(accept_len);

        for msg in accepted {
            let path = *msg.handle();
            let ecn = msg.ecn();
            let payload = msg.payload().to_vec();
            let packet = Packet { path, ecn, payload };
            self.send_packet(packet);
        }

        // log all of the dropped packets
        for msg in dropped {
            let _span = debug_span!(
                "packet",
                dest = %msg.handle().local_address.0,
                src = %msg.handle().remote_address.0,
                len = msg.payload().len()
            )
            .entered();
            debug!("packet::enqueue::drop capacity={}", self.capacity);
        }

        msgs.len()
    }

    pub fn send_packet(&mut self, mut packet: Packet) {
        // update the path with the latest address
        packet.path.local_address = self.local_address;

        let _span = debug_span!(
            "packet",
            dest = %packet.path.remote_address.0,
            src = %packet.path.local_address.0,
            len = packet.payload.len()
        )
        .entered();

        // Only send what capacity we have left. Drop the rest.
        //
        // This matches the behavior of existing UDP stacks.
        // See https://github.com/tokio-rs/turmoil/pull/128#issuecomment-1638584711
        if self.capacity <= self.packets.len() {
            trace!("packet::send::drop capacity={}", self.capacity);
            return;
        }

        if packet.payload.len() > self.mtu as usize {
            trace!("packet::send::truncate mtu={}", self.mtu);
            packet.payload.truncate(self.mtu as usize);
        }

        trace!("packet::send");

        self.packets.push_back(packet);
    }

    pub fn recv(
        &mut self,
        cx: &mut Context,
        msgs: &mut [super::message::Message],
        max_mtu: MaxMtu,
    ) -> usize {
        let to_remove = self.packets.len().min(msgs.len());

        if to_remove == 0 {
            self.waker = Some(cx.waker().clone());
            return 0;
        }

        self.waker.take();

        for (packet, msg) in self.packets.drain(..to_remove).zip(msgs) {
            let _span = debug_span!(
                "packet",
                dest = %packet.path.remote_address.0,
                src = %packet.path.local_address.0,
                len = packet.payload.len()
            )
            .entered();

            *msg.handle_mut() = packet.path;
            *msg.ecn_mut() = packet.ecn;

            unsafe {
                // Safety: the message was allocated with the configured MaxMtu
                msg.reset(max_mtu.into());
            }

            let payload = msg.payload_mut();
            let to_copy = payload.len().min(packet.payload.len());
            payload[..to_copy].copy_from_slice(&packet.payload[..to_copy]);

            unsafe {
                msg.set_payload_len(to_copy);
            }

            if to_copy != packet.payload.len() {
                debug!("packet::truncate");
            }

            trace!("packet::recv");
        }

        to_remove
    }

    pub fn recv_packet(&mut self, cx: Option<&mut Context>) -> core::task::Poll<Packet> {
        if let Some(packet) = self.packets.pop_front() {
            let _span = debug_span!(
                "packet",
                dest = %packet.path.remote_address.0,
                src = %packet.path.local_address.0,
                len = packet.payload.len()
            )
            .entered();

            trace!("packet::recv");

            self.waker.take();
            packet.into()
        } else {
            if let Some(cx) = cx {
                self.waker = Some(cx.waker().clone());
            }
            core::task::Poll::Pending
        }
    }

    pub fn len(&self) -> usize {
        self.packets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Packet {
    pub path: Tuple,
    pub ecn: ExplicitCongestionNotification,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn switch(&mut self) {
        let path = self.path;
        let remote_address = path.local_address.0.into();
        let local_address = path.remote_address.0.into();
        self.path = Tuple {
            remote_address,
            local_address,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn address_generator() {
        let buffers = Buffers::default();

        let mut addrs = vec![];
        for _ in 0..10 {
            addrs.push(buffers.generate_addr());
        }

        if !cfg!(miri) {
            assert_debug_snapshot!(addrs);
        }
    }
}
