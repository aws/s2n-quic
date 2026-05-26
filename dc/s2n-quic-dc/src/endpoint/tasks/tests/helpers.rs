// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint::{
        combinator::FrameBatch,
        frame::{self, Frame, Header},
        id::Id,
        send,
    },
    intrusive::Entry,
    packet::datagram::QueuePair,
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{intrusive::unsync, Budget, EntryBoxSender, Receiver},
    stream::endpoint::recv,
    time::bach::Clock,
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

/// Test sender that immediately wakes any `AutoWake` values it receives.
///
/// In production, wakers are offloaded to a drain task to avoid waking the application
/// inside busy-poll loops. In tests we just fire them inline.
pub struct WakeNowSender;

impl crate::socket::channel::UnboundedSender<crate::flow::queue::AutoWake> for WakeNowSender {
    fn send(
        &mut self,
        _value: crate::flow::queue::AutoWake,
    ) -> Result<(), crate::flow::queue::AutoWake> {
        // AutoWake fires on drop — just let it drop here
        Ok(())
    }
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
    remote_sender_id: crate::endpoint::id::RemoteSenderId,
    local_sender_id: crate::endpoint::id::LocalSenderId,
}

impl Default for RecvContextBuilder {
    fn default() -> Self {
        Self {
            peer: "127.0.0.1:4433".parse().unwrap(),
            remote_sender_id: crate::endpoint::id::RemoteSenderId::new(VarInt::from_u8(0)),
            local_sender_id: crate::endpoint::id::LocalSenderId::new(VarInt::from_u8(1)),
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
        self.remote_sender_id = crate::endpoint::id::RemoteSenderId::new(id);
        self
    }

    #[expect(dead_code)]
    pub fn local_sender_id(mut self, id: VarInt) -> Self {
        self.local_sender_id = crate::endpoint::id::LocalSenderId::new(id);
        self
    }

    pub fn build(self) -> Rc<RefCell<recv::Context>> {
        let entry = PathSecretEntry::builder(self.peer)
            .endpoint_type(s2n_quic_core::endpoint::Type::Server)
            .build();
        let opener = entry.secret().application_opener(VarInt::ZERO);
        let clock = crate::time::bach::Clock::default();
        Rc::new(RefCell::new(recv::Context::new(
            entry,
            self.remote_sender_id,
            self.local_sender_id,
            opener,
            VarInt::ZERO,
            crate::time::precision::Clock::now(&clock),
        )))
    }
}

/// Creates a `PathSecretEntry` routed to the address resolved from `addr`.
///
/// Both the path-secret addr and the peer-data addr are set to the resolved
/// address.  Accepts any Bach [`ToSocketAddrs`] — including group names like
/// `"server:4433"` — so tests can resolve simulated IPs registered by
/// `.group("server")` without hard-coding addresses.
///
/// [`ToSocketAddrs`]: bach::net::ToSocketAddrs
pub async fn test_entry_at(addr: impl bach::net::ToSocketAddrs) -> Arc<PathSecretEntry> {
    let addr = bach::net::lookup_host(addr)
        .await
        .expect("address resolution failed")
        .next()
        .expect("lookup_host returned empty iterator");
    let pse = PathSecretEntry::builder(addr).build();
    pse.set_peer_data_addrs(&[addr]);
    pse
}

pub fn test_entry() -> Arc<PathSecretEntry> {
    let addr: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    let pse = PathSecretEntry::builder(addr)
        .socket_sender_count(8)
        .build();
    pse.set_peer_data_addrs(&[addr]);
    pse
}

/// Creates a minimal QueueData frame for testing pipeline plumbing.
pub fn test_frame(pse: &Arc<PathSecretEntry>) -> Entry<Frame> {
    test_frame_with_payload(pse, 0)
}

/// Creates a QueueData frame whose application payload is `payload_size` zero bytes.
///
/// Use a payload large enough that two frames together exceed one MTU: the assembler
/// will only be able to pack the first into a single segment and will push the second
/// back, causing it to re-arm the TX wheel.
pub fn test_frame_with_payload(pse: &Arc<PathSecretEntry>, payload_size: usize) -> Entry<Frame> {
    Entry::new(Frame {
        header: Header::QueueData {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            binding_id: VarInt::from_u8(1),
            offset: VarInt::ZERO,
            is_fin: false,
        },
        source_sender_id: crate::endpoint::id::LocalSenderId::new(VarInt::MAX),
        payload: bytes::BytesMut::zeroed(payload_size).into(),
        path_secret_entry: pse.clone(),
        completion: None,
        status: frame::TransmissionStatus::Pending,
        ttl: 3,
        transmission_time: None,
    })
}

/// Creates a single-frame FrameBatch with sender_id=0.
pub fn test_batch(pse: &Arc<PathSecretEntry>) -> Entry<FrameBatch> {
    test_batch_with_payload(pse, 0)
}

/// Creates a single-frame FrameBatch carrying `payload_size` bytes of payload.
///
/// Use two such batches pushed into a context with a large enough payload to ensure
/// the assembler can only fit the first into one segment, leaving the second pending
/// and re-arming the TX wheel.
pub fn test_batch_with_payload(
    pse: &Arc<PathSecretEntry>,
    payload_size: usize,
) -> Entry<FrameBatch> {
    let mut batch = FrameBatch::single(test_frame_with_payload(pse, payload_size));
    batch.set_sender_id(crate::endpoint::id::LocalSenderId::from_index(0));
    Entry::new(batch)
}

/// Creates a `send::Context` for `entry` wrapped in `Rc<RefCell<_>>`.
///
/// All three queue-depth gauges are registered under `test.inflight`, `test.ack`,
/// and `test.pending`.  The context is immediately ready for use — callers push
/// frames and set `ctx.borrow_mut().tx_wheel.target_time` as needed.
pub fn build_send_context(
    entry: &Arc<PathSecretEntry>,
    sender_idx: usize,
    registry: &crate::counter::Registry,
    clock: &Clock,
) -> std::rc::Rc<std::cell::RefCell<send::Context>> {
    let ctx = send::Context::new(
        entry,
        registry.register_queue_gauge("test.inflight"),
        registry.register_queue_gauge("test.ack"),
        registry.register_queue_gauge("test.pending"),
        crate::endpoint::id::LocalSenderId::from_index(sender_idx),
        clock,
    )
    .expect("test context should be constructible");
    std::rc::Rc::new(std::cell::RefCell::new(ctx))
}
