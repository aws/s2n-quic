// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint::{
        combinator::FrameBatch,
        frame::{self, Frame, Header},
    },
    intrusive::Entry,
    packet::datagram::QueuePair,
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{intrusive::unsync, Budget, EntryBoxSender, Receiver},
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

pub fn entry_channel<T>() -> (
    EntryBoxSender<T, unsync::Sender<crate::intrusive::EntryAdapter<T>>>,
    unsync::Receiver<crate::intrusive::EntryAdapter<T>>,
) {
    let (tx, rx) = unsync::new::<T>();
    (EntryBoxSender::new(tx), rx)
}

// ── ReceiverExt ──────────────────────────────────────────────────────────

pub trait TestReceiverExt<T>: Receiver<T> {
    /// Await the next item from the receiver, returning `None` if the channel is closed.
    async fn recv(&mut self) -> Option<T> {
        use core::future::poll_fn;
        let mut budget = Budget::new(1);
        poll_fn(|cx| {
            budget.reset();
            self.poll_recv(cx, &mut budget)
        })
        .await
    }
}

impl<T, R: Receiver<T>> TestReceiverExt<T> for R {}

// ── Recv Context Builder ─────────────────────────────────────────────────

pub struct RecvContextBuilder {
    peer: SocketAddr,
    remote_sender_id: VarInt,
    dest_sender_id: VarInt,
}

impl Default for RecvContextBuilder {
    fn default() -> Self {
        Self {
            peer: "127.0.0.1:4433".parse().unwrap(),
            remote_sender_id: VarInt::from_u8(0),
            dest_sender_id: VarInt::from_u8(1),
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
            crate::time::precision::Clock::now(&clock),
        )))
    }
}

// ── Frame/Batch Helpers ──────────────────────────────────────────────────

/// Creates a PathSecretEntry with peer data addrs set (required for send::Cache).
pub fn test_entry() -> Arc<PathSecretEntry> {
    let addr: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    let pse = PathSecretEntry::fake_deterministic(addr, s2n_quic_core::endpoint::Type::Client);
    pse.set_peer_data_addrs(&[addr]);
    pse
}

/// Creates a minimal FlowData frame for testing pipeline plumbing.
pub fn test_frame(pse: &Arc<PathSecretEntry>) -> Entry<Frame> {
    Entry::new(Frame {
        header: Header::FlowData {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            stream_id: VarInt::from_u8(1),
            offset: VarInt::ZERO,
            is_fin: false,
        },
        source_sender_id: VarInt::MAX,
        payload: Default::default(),
        path_secret_entry: pse.clone(),
        completion: None,
        status: frame::TransmissionStatus::Pending,
        ttl: 3,
        transmission_time: None,
    })
}

/// Creates a single-frame FrameBatch with sender_id=0.
pub fn test_batch(pse: &Arc<PathSecretEntry>) -> Entry<FrameBatch> {
    let mut batch = FrameBatch::single(test_frame(pse));
    batch.set_sender_id(0);
    Entry::new(batch)
}
