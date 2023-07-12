// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{message::Message as _, socket};
use core::task::{Context, Waker};
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::{rx, tx},
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

// This constant is used to size the buffer for packet payloads
// we use 10_000 since there are unit tests for jumbo frames, which
// have MTU's up to approximately 9_001
const MAX_TESTED_MTU: u16 = 10_000;

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
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

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

            eprintln!("{prev} -> {addr}");
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
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

        if !lock.is_open {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "host is closed",
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
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

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

        // find all of the queues with at least one packet to transmit
        for queue in lock.tx.values_mut() {
            if queue.packets.is_empty() {
                continue;
            }

            queues.push(queue);
        }

        // shuffle the queue so each endpoint has a fair chance of transmitting
        super::rand::shuffle(&mut queues);

        loop {
            let mut has_result = false;
            for queue in &mut queues {
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
                        return;
                    }
                }
            }

            // if all of the queues are empty then just return
            if !has_result {
                return;
            }
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
            lock.tx.remove(&host);
            lock.rx.remove(&host);
            if let Some(addrs) = lock.host_to_addr.remove(&host) {
                for addr in addrs {
                    lock.addr_to_host.remove(&addr);
                }
            }
        }
    }

    /// Register an address on the network
    pub fn register(
        &self,
        handle: SocketAddress,
        max_mtu: MaxMtu,
    ) -> (
        impl tx::Tx<PathHandle = PathHandle>,
        impl rx::Rx<PathHandle = PathHandle>,
        super::socket::Socket,
    ) {
        let mut lock = self.inner.lock().unwrap();

        let host = HostId(lock.next_host);
        lock.next_host += 1;

        let queue = Queue::new(handle);

        lock.addr_to_host.insert(handle, host);
        lock.host_to_addr.insert(host, vec![handle]);
        lock.tx.insert(host, queue.clone());
        lock.rx.insert(host, queue);

        // TODO allow configuration of this
        let queue_recv_buffer_size = None;
        let queue_send_buffer_size = None;

        let socket = super::socket::Socket::new(self.clone(), host);

        let rx = {
            let payload_len = {
                let max_mtu: u16 = max_mtu.into();
                max_mtu as u32
            };

            let rx_buffer_size = queue_recv_buffer_size.unwrap_or(8u32 * (1 << 20));
            let entries = rx_buffer_size / payload_len;
            let entries = if entries.is_power_of_two() {
                entries
            } else {
                // round up to the nearest power of two, since the ring buffers require it
                entries.next_power_of_two()
            };

            let mut consumers = vec![];

            let (producer, consumer) = socket::ring::pair(entries, payload_len);
            consumers.push(consumer);

            // spawn a task that actually reads from the socket into the ring buffer
            super::spawn(super::socket::rx(socket.clone(), producer));

            // construct the RX side for the endpoint event loop
            let max_mtu = MaxMtu::try_from(payload_len as u16).unwrap();
            socket::io::rx::Rx::new(consumers, max_mtu, handle.into())
        };

        let tx = {
            let gso = crate::features::Gso::default();
            gso.disable();

            // compute the payload size for each message from the number of GSO segments we can
            // fill
            let payload_len = {
                let max_mtu: u16 = max_mtu.into();
                (max_mtu as u32 * gso.max_segments() as u32).min(u16::MAX as u32)
            };

            let tx_buffer_size = queue_send_buffer_size.unwrap_or(128 * 1024);
            let entries = tx_buffer_size / payload_len;
            let entries = if entries.is_power_of_two() {
                entries
            } else {
                // round up to the nearest power of two, since the ring buffers require it
                entries.next_power_of_two()
            };

            let mut producers = vec![];

            let (producer, consumer) = socket::ring::pair(entries, payload_len);
            producers.push(producer);

            // spawn a task that actually flushes the ring buffer to the socket
            super::spawn(super::socket::tx(socket.clone(), consumer, gso.clone()));

            // construct the TX side for the endpoint event loop
            socket::io::tx::Tx::new(producers, gso, max_mtu)
        };

        (tx, rx, socket)
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

#[derive(Clone, Debug)]
pub struct Queue {
    capacity: usize,
    mtu: u16,
    packets: VecDeque<Packet>,
    local_address: LocalAddress,
    waker: Option<Waker>,
}

impl Queue {
    fn new(addr: SocketAddress) -> Self {
        let mtu = MAX_TESTED_MTU;
        let local_address = addr.into();
        Self {
            capacity: 1024,
            mtu,
            packets: VecDeque::new(),
            local_address,
            waker: None,
        }
    }

    pub fn enqueue(&mut self, packet: Packet) {
        if self.packets.len() == self.capacity {
            // drop old packets if we're at capacity
            let _ = self.packets.pop_front();
        }

        self.packets.push_back(packet);

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
        let to_drop = self
            .packets
            .len()
            .saturating_add(msgs.len())
            .saturating_sub(self.capacity);

        // drop the oldest packets, if needed
        if to_drop > 0 {
            self.packets.drain(..to_drop);
        }

        for msg in msgs {
            let mut path = *msg.handle();

            // update the path with the latest address
            path.local_address = self.local_address;

            let ecn = msg.ecn();

            let msg_payload = msg.payload();
            let payload_len = msg_payload.len().min(self.mtu as _);
            let mut payload = vec![0u8; payload_len];
            payload.copy_from_slice(&msg_payload[..payload_len]);

            let packet = Packet { path, ecn, payload };

            self.packets.push_back(packet);
        }

        msgs.len()
    }

    pub fn recv(&mut self, cx: &mut Context, msgs: &mut [super::message::Message]) -> usize {
        let to_remove = self.packets.len().min(msgs.len());

        if to_remove == 0 {
            self.waker = Some(cx.waker().clone());
            return 0;
        }

        self.waker.take();

        for (packet, msg) in self.packets.drain(..to_remove).zip(msgs) {
            *msg.handle_mut() = packet.path;
            *msg.ecn_mut() = packet.ecn;
            let payload = msg.payload_mut();
            let to_copy = payload.len().min(packet.payload.len());
            payload[..to_copy].copy_from_slice(&packet.payload[..to_copy]);

            unsafe {
                msg.set_payload_len(to_copy);
            }
        }

        to_remove
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
