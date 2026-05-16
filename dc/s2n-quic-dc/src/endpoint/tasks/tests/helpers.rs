// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{Budget, Receiver, UnboundedSender},
    stream::endpoint::recv,
};
use core::task::{Poll, Waker};
use s2n_quic_core::varint::VarInt;
use std::{
    cell::RefCell,
    collections::VecDeque,
    net::SocketAddr,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

// ── Test Waker ───────────────────────────────────────────────────────────

pub struct WakeCount(Arc<WakeCounter>);

impl WakeCount {
    pub fn count(&self) -> usize {
        self.0 .0.load(Ordering::Relaxed)
    }
}

struct WakeCounter(AtomicUsize);

impl std::task::Wake for WakeCounter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn test_waker() -> (Waker, WakeCount) {
    let counter = Arc::new(WakeCounter(AtomicUsize::new(0)));
    let handle = WakeCount(counter.clone());
    let waker = Waker::from(counter);
    (waker, handle)
}

// ── Test Channels ────────────────────────────────────────────────────────

/// A pre-loaded receiver that yields items from a VecDeque, returning None when empty.
pub struct TestReceiver<T> {
    pub values: VecDeque<T>,
}

impl<T> TestReceiver<T> {
    pub fn new(values: impl IntoIterator<Item = T>) -> Self {
        Self {
            values: values.into_iter().collect(),
        }
    }
}

impl<T> Receiver<T> for TestReceiver<T> {
    fn poll_recv(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<T>> {
        if budget.is_exhausted() {
            budget.set_needs_wake();
            return Poll::Pending;
        }
        match self.values.pop_front() {
            Some(value) => {
                budget.consume();
                Poll::Ready(Some(value))
            }
            None => Poll::Ready(None),
        }
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}

/// A sender that collects all sent items into a shared Vec for assertion.
pub struct CollectingSender<T> {
    pub items: Rc<RefCell<Vec<T>>>,
}

impl<T> CollectingSender<T> {
    pub fn new() -> (Self, Rc<RefCell<Vec<T>>>) {
        let items = Rc::new(RefCell::new(Vec::new()));
        (
            Self {
                items: items.clone(),
            },
            items,
        )
    }
}

impl<T> UnboundedSender<T> for CollectingSender<T> {
    fn send(&mut self, value: T) -> Result<(), T> {
        self.items.borrow_mut().push(value);
        Ok(())
    }
}

// ── Recv Context Builder ─────────────────────────────────────────────────

pub struct RecvContextBuilder {
    peer: SocketAddr,
    remote_sender_id: VarInt,
    dest_sender_id: VarInt,
    idle_timeout: Duration,
}

impl Default for RecvContextBuilder {
    fn default() -> Self {
        Self {
            peer: "127.0.0.1:4433".parse().unwrap(),
            remote_sender_id: VarInt::from_u8(0),
            dest_sender_id: VarInt::from_u8(1),
            idle_timeout: Duration::from_secs(30),
        }
    }
}

impl RecvContextBuilder {
    #[expect(dead_code)]
    pub fn peer(mut self, peer: SocketAddr) -> Self {
        self.peer = peer;
        self
    }

    pub fn remote_sender_id(mut self, id: VarInt) -> Self {
        self.remote_sender_id = id;
        self
    }

    #[expect(dead_code)]
    pub fn dest_sender_id(mut self, id: VarInt) -> Self {
        self.dest_sender_id = id;
        self
    }

    pub fn build(self) -> Rc<RefCell<recv::Context>> {
        let entry =
            PathSecretEntry::fake_deterministic(self.peer, s2n_quic_core::endpoint::Type::Server);
        let opener = entry.secret().application_opener(VarInt::ZERO);
        let clock = crate::time::bach::Clock::default();
        Rc::new(RefCell::new(recv::Context::new(
            entry,
            self.remote_sender_id,
            self.dest_sender_id,
            opener,
            VarInt::ZERO,
            &clock,
            self.idle_timeout,
        )))
    }
}
