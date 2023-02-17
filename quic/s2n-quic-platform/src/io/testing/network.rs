// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use s2n_quic_core::{
    inet::{datagram, ExplicitCongestionNotification, SocketAddress},
    io::{
        self,
        tx::{Entry as _, Queue as _},
    },
    path::{LocalAddress, Tuple},
};
use std::{
    collections::{HashMap, VecDeque},
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
        let mut lock = self.inner.lock().unwrap();
        lock.is_open = false;

        let state = &mut *lock;

        for entry in state.tx.values_mut().chain(state.rx.values_mut()) {
            if let Some(waker) = entry.waker.take() {
                waker.wake();
            }
        }
    }

    pub fn tx<F: FnOnce(&mut Queue)>(&self, handle: SocketAddress, f: F) {
        let mut lock = self.inner.lock().unwrap();
        if let Some(queue) = lock.tx.get_mut(&handle) {
            f(queue)
        }
    }

    pub fn rx<F: FnOnce(&mut Queue)>(&self, handle: SocketAddress, f: F) {
        let mut lock = self.inner.lock().unwrap();
        if let Some(queue) = lock.rx.get_mut(&handle) {
            f(queue)
        }
    }

    pub fn pending_transmission<F: FnMut(&Packet)>(&self, mut f: F) {
        let lock = self.inner.lock().unwrap();
        for queue in lock.tx.values() {
            for packet in &queue.packets {
                f(packet);
            }
        }
    }

    pub fn drain_pending_transmissions<F: FnMut(Packet) -> Result<(), ()>>(&self, mut f: F) {
        let mut lock = self.inner.lock().unwrap();

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

    pub(crate) fn readiness(&self, handle: SocketAddress) -> Readiness {
        Readiness {
            network: self,
            handle,
        }
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

    /// Register an address on the network
    pub fn register(&self, handle: SocketAddress) {
        let mut lock = self.inner.lock().unwrap();

        let queue = Queue::new(handle);

        lock.tx.insert(handle, queue.clone());
        lock.rx.insert(handle, queue);
    }
}

pub(crate) struct Readiness<'a> {
    network: &'a Buffers,
    handle: SocketAddress,
}

impl<'a> Future for Readiness<'a> {
    type Output = Result<(), ()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut lock = self.network.inner.lock().unwrap();

        if !lock.is_open {
            return Err(()).into();
        }

        let mut is_ready = false;

        let tx = lock.tx.get_mut(&self.handle).unwrap();
        if tx.is_blocked {
            // if we were blocked and now have capacity wake up the endpoint
            if tx.has_capacity() {
                tx.is_blocked = false;
                is_ready = true;
            } else {
                tx.waker = Some(cx.waker().clone());
            }
        }

        let rx = lock.rx.get_mut(&self.handle).unwrap();
        // wake up the endpoint if we have an rx message
        if io::rx::Queue::is_empty(rx) {
            rx.waker = Some(cx.waker().clone());
        } else {
            is_ready = true;
        }

        if is_ready {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

#[derive(Debug)]
struct State {
    is_open: bool,
    tx: HashMap<SocketAddress, Queue>,
    rx: HashMap<SocketAddress, Queue>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            is_open: true,
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
    pending: Packet,
    local_address: LocalAddress,
    is_blocked: bool,
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
            pending: Packet::new(mtu, local_address),
            local_address,
            is_blocked: false,
            waker: None,
        }
    }

    pub fn receive(&mut self, packet: Packet) {
        if self.packets.len() == self.capacity {
            // drop old packets if we're at capacity
            let _ = self.packets.pop_front();
        }

        self.packets.push_back(packet);

        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }

    pub fn take(&mut self, count: usize) -> impl Iterator<Item = Packet> + '_ {
        let count = self.packets.len().min(count);
        self.packets.drain(..count)
    }

    pub fn drain(&mut self) -> impl Iterator<Item = Packet> + '_ {
        self.packets.drain(..)
    }
}

impl io::tx::Queue for Queue {
    type Entry = Packet;
    type Handle = Tuple;

    const SUPPORTS_ECN: bool = true;

    fn push<M: io::tx::Message<Handle = Self::Handle>>(
        &mut self,
        message: M,
    ) -> Result<io::tx::Outcome, io::tx::Error> {
        if !self.has_capacity() {
            self.is_blocked = true;
            return Err(io::tx::Error::AtCapacity);
        }

        let len = self.pending.set(message)?;
        self.pending.payload.truncate(len);

        // create a packet for the next transmission
        let next = Packet::new(self.mtu, self.local_address);
        let packet = core::mem::replace(&mut self.pending, next);

        self.packets.push_back(packet);

        Ok(io::tx::Outcome { len, index: 0 })
    }

    fn as_slice_mut(&mut self) -> &mut [Self::Entry] {
        self.packets.make_contiguous()
    }

    fn capacity(&self) -> usize {
        self.capacity - self.packets.len()
    }

    fn len(&self) -> usize {
        self.packets.len()
    }
}

impl io::rx::Queue for Queue {
    type Entry = Packet;
    type Handle = Tuple;

    fn finish(&mut self, count: usize) {
        self.packets.drain(..count);
    }

    fn len(&self) -> usize {
        self.packets.len()
    }

    fn as_slice_mut(&mut self) -> &mut [Self::Entry] {
        self.packets.make_contiguous()
    }

    fn local_address(&self) -> LocalAddress {
        self.local_address
    }
}

#[derive(Clone, Debug)]
pub struct Packet {
    pub path: Tuple,
    pub ecn: ExplicitCongestionNotification,
    pub payload: Vec<u8>,
}

impl Packet {
    fn new(mtu: u16, local_address: LocalAddress) -> Self {
        Self {
            path: Tuple {
                local_address,
                remote_address: Default::default(),
            },
            ecn: Default::default(),
            payload: vec![0u8; mtu as usize],
        }
    }

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

impl io::tx::Entry for Packet {
    type Handle = Tuple;

    fn set<M>(&mut self, mut message: M) -> Result<usize, io::tx::Error>
    where
        M: io::tx::Message<Handle = Tuple>,
    {
        self.path.remote_address = message.path_handle().remote_address;
        self.ecn = message.ecn();
        message.write_payload(io::tx::PayloadBuffer::new(&mut self.payload), 0)
    }

    fn payload(&self) -> &[u8] {
        &self.payload
    }

    fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.payload
    }
}

impl io::rx::Entry for Packet {
    type Handle = Tuple;

    fn read(
        &mut self,
        _local_address: &LocalAddress,
    ) -> Option<(datagram::Header<Self::Handle>, &mut [u8])> {
        let header = datagram::Header {
            path: self.path,
            ecn: self.ecn,
        };
        let payload = &mut self.payload;
        Some((header, payload))
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

        assert_debug_snapshot!(addrs);
    }
}
