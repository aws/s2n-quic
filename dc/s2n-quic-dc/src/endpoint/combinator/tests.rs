// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    byte_vec::ByteVec,
    endpoint::{
        frame::{Header, TransmissionStatus, DEFAULT_TTL},
        id::{Id, LocalSendSocketId, LocalSenderId},
    },
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{intrusive::unsync, ByteCost},
    time::testing as test_clock_mod,
};
use bytes::{Bytes, BytesMut};
use core::task::Poll;
use s2n_quic_core::varint::VarInt;
use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

fn test_clock() -> test_clock_mod::Clock {
    test_clock_mod::Clock::new(std::time::Duration::from_secs(1))
}

struct TestItem {
    path_secret_entry: Arc<PathSecretEntry>,
    byte_cost: u64,
    sticky_sender: Option<usize>,
    drop_counter: Arc<AtomicUsize>,
}

impl Drop for TestItem {
    fn drop(&mut self) {
        self.drop_counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl ByteCost for TestItem {
    fn byte_cost(&self) -> u64 {
        self.byte_cost
    }
}

impl PathSecretMapEntry for TestItem {
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        &self.path_secret_entry
    }
}

impl StickyRoute for TestItem {
    fn sticky_sender_idx(&self) -> Option<crate::endpoint::id::LocalSenderId> {
        self.sticky_sender
            .map(crate::endpoint::id::LocalSenderId::from_index)
    }

    fn set_sender_id(&mut self, _id: crate::endpoint::id::LocalSenderId) {}
}

struct TestSender {
    accept: bool,
    calls: usize,
}

impl UnboundedSender<TestItem> for TestSender {
    fn send(&mut self, value: TestItem) -> Result<(), TestItem> {
        self.calls += 1;
        if self.accept {
            drop(value);
            Ok(())
        } else {
            Err(value)
        }
    }
}

struct TestReceiver<T> {
    values: VecDeque<T>,
    consumed: u64,
}

impl<T> Receiver<T> for TestReceiver<T> {
    fn poll_recv(&mut self, _cx: &mut task::Context<'_>, _budget: &mut Budget) -> Poll<Option<T>> {
        Poll::Ready(self.values.pop_front())
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.consumed += bytes;
    }
}

struct BudgetAwareTestReceiver<T> {
    values: VecDeque<T>,
}

impl<T> Receiver<T> for BudgetAwareTestReceiver<T> {
    fn poll_recv(&mut self, _cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<T>> {
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

fn test_path_secret_entry() -> Arc<PathSecretEntry> {
    let peer: std::net::SocketAddr = "127.0.0.1:4433".parse().unwrap();
    PathSecretEntry::builder(peer)
        .socket_sender_count(2)
        .build()
}

fn new_test_item(
    path_secret_entry: Arc<PathSecretEntry>,
    drop_counter: Arc<AtomicUsize>,
) -> TestItem {
    TestItem {
        path_secret_entry,
        byte_cost: 123,
        sticky_sender: None,
        drop_counter,
    }
}

fn new_test_frame(path_secret_entry: Arc<PathSecretEntry>, payload_len: usize) -> Entry<Frame> {
    new_test_frame_with_sender_id(path_secret_entry, payload_len, VarInt::MAX)
}

fn new_test_frame_with_header(
    path_secret_entry: Arc<PathSecretEntry>,
    payload_len: usize,
    header: Header,
) -> Entry<Frame> {
    let mut payload = ByteVec::new();
    if payload_len > 0 {
        payload.push_back(Bytes::from(vec![0u8; payload_len]));
    }

    Entry::new(Frame {
        header,
        source_sender_id: crate::endpoint::id::LocalSenderId::new(VarInt::MAX),
        payload,
        path_secret_entry,
        completion: None,
        status: TransmissionStatus::Pending,
        ttl: DEFAULT_TTL,
        transmission_time: None,
    })
}

fn new_test_frame_with_sender_id(
    path_secret_entry: Arc<PathSecretEntry>,
    payload_len: usize,
    source_sender_id: VarInt,
) -> Entry<Frame> {
    let mut payload = ByteVec::new();
    if payload_len > 0 {
        payload.push_back(Bytes::from(vec![0u8; payload_len]));
    }

    Entry::new(Frame {
        header: Header::QueueControl {
            queue_pair: crate::packet::datagram::QueuePair {
                source_queue_id: VarInt::from_u8(0),
                dest_queue_id: VarInt::from_u8(1),
            },
            binding_id: VarInt::from_u8(0),
        },
        source_sender_id: crate::endpoint::id::LocalSenderId::new(source_sender_id),
        payload,
        path_secret_entry,
        completion: None,
        status: TransmissionStatus::Pending,
        ttl: DEFAULT_TTL,
        transmission_time: None,
    })
}

fn drive_completion_dispatcher(
    dispatcher: &mut CompletionDispatcher<TestReceiver<Entry<Frame>>>,
    budget_capacity: usize,
) -> Poll<Option<crate::flow::queue::AutoWake>> {
    with_noop_context(|cx| {
        let mut budget = Budget::new(budget_capacity);
        dispatcher.poll_recv(cx, &mut budget)
    })
}

fn with_noop_context<R>(f: impl FnOnce(&mut task::Context<'_>) -> R) -> R {
    let waker = s2n_quic_core::task::waker::noop();
    let mut cx = task::Context::from_waker(&waker);
    f(&mut cx)
}

// ── MappedSender tests ─────────────────────────────────────────────────────

#[derive(Debug)]
struct MappedItem {
    sender_id: LocalSenderId,
    value: usize,
}

impl HasId<LocalSenderId> for MappedItem {
    fn id(&self) -> LocalSenderId {
        self.sender_id
    }
}

struct MappedItemSender {
    sink: Rc<RefCell<Vec<usize>>>,
}

impl UnboundedSender<MappedItem> for MappedItemSender {
    fn send(&mut self, value: MappedItem) -> Result<(), MappedItem> {
        self.sink.borrow_mut().push(value.value);
        Ok(())
    }
}

#[test]
fn mapped_sender_routes_items_through_id_map() {
    let sink0 = Rc::new(RefCell::new(Vec::new()));
    let sink1 = Rc::new(RefCell::new(Vec::new()));

    let senders: crate::endpoint::id::IdMap<LocalSendSocketId, MappedItemSender> = [
        (
            LocalSendSocketId::new(0),
            MappedItemSender {
                sink: sink0.clone(),
            },
        ),
        (
            LocalSendSocketId::new(1),
            MappedItemSender {
                sink: sink1.clone(),
            },
        ),
    ]
    .into_iter()
    .collect();

    let mut sender_idx_to_local: crate::endpoint::id::IdMap<LocalSenderId, LocalSendSocketId> =
        crate::endpoint::id::IdMap::new(2, LocalSendSocketId::new(usize::MAX));
    sender_idx_to_local[LocalSenderId::from_index(0)] = LocalSendSocketId::new(1);
    sender_idx_to_local[LocalSenderId::from_index(1)] = LocalSendSocketId::new(0);

    let mut tx = MappedSender::new(senders, sender_idx_to_local);
    tx.send(MappedItem {
        sender_id: LocalSenderId::from_index(0),
        value: 10,
    })
    .unwrap();
    tx.send(MappedItem {
        sender_id: LocalSenderId::from_index(1),
        value: 20,
    })
    .unwrap();

    assert_eq!(&*sink0.borrow(), &[20]);
    assert_eq!(&*sink1.borrow(), &[10]);
}

// ── PickTwo tests ─────────────────────────────────────────────────────────

fn try_send_pick_two(
    value: TestItem,
    senders: &mut Vec<TestSender>,
    rng: &mut crate::xorshift::Rng,
) -> Result<(), TestItem> {
    use crate::time::precision::Clock as _;

    let registry = crate::counter::Registry::default();
    let pick_counters: Vec<_> = (0..senders.len())
        .map(|i| registry.register_nominal("pick_two.chosen", format_args!("send.{i}")))
        .collect();
    let rejected_counters: Vec<_> = (0..senders.len())
        .map(|i| {
            registry.register_nominal_summary(
                "pick_two.rejected",
                format_args!("send.{i}"),
                crate::counter::Unit::Microsecond,
            )
        })
        .collect();
    let score_delta =
        registry.register_summary("pick_two.score_delta", crate::counter::Unit::Microsecond);
    let pick_counters_map: crate::endpoint::id::IdMap<crate::endpoint::id::LocalSenderId, _> =
        pick_counters.into();
    let rejected_counters_map: crate::endpoint::id::IdMap<crate::endpoint::id::LocalSenderId, _> =
        rejected_counters.into();
    let mut senders_map: crate::endpoint::id::IdMap<crate::endpoint::id::LocalSenderId, _> =
        std::mem::take(senders).into();
    let mut socket_edts =
        crate::endpoint::edt::Local::new(senders_map.len(), crate::socket::rate::Rate::new(10.0));
    let clock = test_clock();
    let now = clock.now();
    let result = PickTwo::<
        TestItem,
        TestReceiver<TestItem>,
        TestSender,
        test_clock_mod::Clock,
    >::try_send_pick_two(
        value,
        &mut senders_map,
        &mut socket_edts,
        now,
        rng,
        &pick_counters_map,
        &rejected_counters_map,
        &score_delta,
    );
    *senders = senders_map.into_iter().map(|(_, v)| v).collect();
    result
}

#[test]
fn selected_sender_receives_item() {
    let item = new_test_item(test_path_secret_entry(), Arc::new(AtomicUsize::new(0)));
    let mut senders = vec![
        TestSender {
            accept: true,
            calls: 0,
        },
        TestSender {
            accept: true,
            calls: 0,
        },
    ];
    let result = try_send_pick_two(item, &mut senders, &mut crate::xorshift::Rng::new());
    assert!(result.is_ok());
    assert_eq!(senders[0].calls + senders[1].calls, 1);
}

#[test]
fn sender_error_returns_value() {
    let drop_counter = Arc::new(AtomicUsize::new(0));
    let item = new_test_item(test_path_secret_entry(), drop_counter.clone());
    let mut senders = vec![
        TestSender {
            accept: false,
            calls: 0,
        },
        TestSender {
            accept: false,
            calls: 0,
        },
    ];
    let result = try_send_pick_two(item, &mut senders, &mut crate::xorshift::Rng::new());
    assert!(result.is_err());
    assert_eq!(senders[0].calls + senders[1].calls, 1);
    assert_eq!(drop_counter.load(Ordering::Relaxed), 0);

    drop(result);
    assert_eq!(drop_counter.load(Ordering::Relaxed), 1);
}

#[test]
fn pick_two_drops_unsent_entry_on_shutdown() {
    let drop_counter = Arc::new(AtomicUsize::new(0));
    let rx = TestReceiver {
        values: [new_test_item(
            test_path_secret_entry(),
            drop_counter.clone(),
        )]
        .into(),
        consumed: 0,
    };
    let senders = vec![
        TestSender {
            accept: false,
            calls: 0,
        },
        TestSender {
            accept: false,
            calls: 0,
        },
    ];
    let registry = crate::counter::Registry::default();
    let pick_two = PickTwo::new(
        rx,
        senders.into(),
        test_clock(),
        crate::socket::rate::Rate::new(10.0),
        crate::xorshift::Rng::new(),
        &registry,
    );
    let mut fut = core::pin::pin!(crate::socket::channel::ReceiverExt::drain_budgeted(
        pick_two, None
    ));
    let result = with_noop_context(|cx| fut.as_mut().poll(cx));
    assert_eq!(result, Poll::Ready(()));
    assert_eq!(drop_counter.load(Ordering::Relaxed), 1);
}

#[test]
fn sticky_sender_bypasses_pick_two() {
    let mut item = new_test_item(test_path_secret_entry(), Arc::new(AtomicUsize::new(0)));
    item.sticky_sender = Some(1);
    let mut senders = vec![
        TestSender {
            accept: true,
            calls: 0,
        },
        TestSender {
            accept: true,
            calls: 0,
        },
    ];
    let result = try_send_pick_two(item, &mut senders, &mut crate::xorshift::Rng::new());
    assert!(result.is_ok());
    assert_eq!(senders[0].calls, 0);
    assert_eq!(senders[1].calls, 1);
}

#[test]
fn sticky_sender_error_returns_value() {
    let drop_counter = Arc::new(AtomicUsize::new(0));
    let mut item = new_test_item(test_path_secret_entry(), drop_counter.clone());
    item.sticky_sender = Some(0);
    let mut senders = vec![
        TestSender {
            accept: false,
            calls: 0,
        },
        TestSender {
            accept: true,
            calls: 0,
        },
    ];
    let result = try_send_pick_two(item, &mut senders, &mut crate::xorshift::Rng::new());
    assert!(result.is_err());
    assert_eq!(senders[0].calls, 1);
    assert_eq!(senders[1].calls, 0);

    drop(result);
    assert_eq!(drop_counter.load(Ordering::Relaxed), 1);
}

#[test]
fn completion_dispatcher_filters_non_failures_for_failure_only_subscriptions() {
    let path_secret_entry = test_path_secret_entry();
    let mut completion_rx = frame::failure_completion_channel();

    let mut frame = new_test_frame(path_secret_entry, 1).into_inner();
    frame.status = TransmissionStatus::Acknowledged;
    frame.completion = Some(completion_rx.sender());

    let rx = TestReceiver {
        values: [Entry::new(frame)].into(),
        consumed: 0,
    };
    let mut dispatcher = CompletionDispatcher::new(rx);
    let _ = drive_completion_dispatcher(&mut dispatcher, usize::MAX);

    with_noop_context(|cx| {
        let result = completion_rx.poll_swap(cx);
        assert!(
            matches!(result, Poll::Pending),
            "acknowledged frame should be filtered before notification"
        );
    });
}

#[test]
fn completion_dispatcher_notifies_failures_for_failure_only_subscriptions() {
    let path_secret_entry = test_path_secret_entry();
    let mut completion_rx = frame::failure_completion_channel();

    let mut frame = new_test_frame(path_secret_entry, 1).into_inner();
    frame.status = TransmissionStatus::Failed(frame::FailureReason::TransmissionError);
    frame.completion = Some(completion_rx.sender());

    let rx = TestReceiver {
        values: [Entry::new(frame)].into(),
        consumed: 0,
    };
    let mut dispatcher = CompletionDispatcher::new(rx);
    let _ = drive_completion_dispatcher(&mut dispatcher, usize::MAX);

    with_noop_context(|cx| match completion_rx.poll_swap(cx) {
        Poll::Ready(Some(queue)) => {
            assert_eq!(queue.len(), 1);
            assert!(matches!(
                queue.front().unwrap().status,
                TransmissionStatus::Failed(frame::FailureReason::TransmissionError)
            ));
        }
        other => panic!("expected failed completion notification, got {other:?}"),
    });
}

#[test]
fn completion_dispatcher_polls_past_filtered_frames_in_same_poll() {
    let path_secret_entry = test_path_secret_entry();
    let mut completion_rx = frame::failure_completion_channel();
    let sender = completion_rx.sender();

    let mut acknowledged = new_test_frame(path_secret_entry.clone(), 1).into_inner();
    acknowledged.status = TransmissionStatus::Acknowledged;
    acknowledged.completion = Some(sender.clone());

    let mut failed = new_test_frame(path_secret_entry, 1).into_inner();
    failed.status = TransmissionStatus::Failed(frame::FailureReason::TransmissionError);
    failed.completion = Some(sender);

    let rx = TestReceiver {
        values: [Entry::new(acknowledged), Entry::new(failed)].into(),
        consumed: 0,
    };
    let mut dispatcher = CompletionDispatcher::new(rx);

    let _ = drive_completion_dispatcher(&mut dispatcher, usize::MAX);

    with_noop_context(|cx| match completion_rx.poll_swap(cx) {
        Poll::Ready(Some(queue)) => {
            assert_eq!(queue.len(), 1);
            assert!(matches!(
                queue.front().unwrap().status,
                TransmissionStatus::Failed(frame::FailureReason::TransmissionError)
            ));
        }
        other => panic!("expected failed completion notification, got {other:?}"),
    });
}

#[test]
fn completion_dispatcher_returns_pending_when_budget_exhausted_while_filtering() {
    let path_secret_entry = test_path_secret_entry();
    let completion_rx = frame::failure_completion_channel();

    let mut frame = new_test_frame(path_secret_entry, 1).into_inner();
    frame.status = TransmissionStatus::Acknowledged;
    frame.completion = Some(completion_rx.sender());

    let rx = BudgetAwareTestReceiver {
        values: [Entry::new(frame)].into(),
    };
    let mut dispatcher = CompletionDispatcher::new(rx);

    with_noop_context(|cx| {
        let mut budget = Budget::new(1);
        let result = dispatcher.poll_recv(cx, &mut budget);
        assert!(matches!(result, Poll::Pending));
        assert!(budget.take_needs_wake());
    });
}

// ── BatchFramesByPathSecret tests ─────────────────────────────────────────

#[test]
fn frame_batch_tracks_byte_costs_per_priority() {
    let path = test_path_secret_entry();
    let first = new_test_frame(path.clone(), 16);
    let first_cost = first.byte_cost();
    let mut batch = FrameBatch::new(first);

    let data = new_test_frame_with_header(
        path.clone(),
        24,
        Header::QueueData {
            queue_pair: crate::packet::datagram::QueuePair {
                source_queue_id: VarInt::from_u8(0),
                dest_queue_id: VarInt::from_u8(1),
            },
            binding_id: VarInt::from_u8(0),
            offset: VarInt::ZERO,
            is_fin: false,
        },
    );
    let data_cost = data.byte_cost();
    batch.push_with_cost(data, data_cost);

    let reset = new_test_frame_with_header(
        path,
        0,
        Header::QueueReset {
            dest_queue_id: VarInt::from_u8(1),
            binding_id: VarInt::from_u8(0),
            reset_target: crate::packet::datagram::ResetTarget::Both,
            error_code: VarInt::from_u8(7),
        },
    );
    let reset_cost = reset.byte_cost();
    batch.push_with_cost(reset, reset_cost);

    assert_eq!(
        batch.byte_cost(),
        MAX_FRAME_BATCH_PACKET_OVERHEAD + first_cost + data_cost + reset_cost
    );

    let (queues, costs) = batch.into_queues();
    assert_eq!(
        costs[Priority::QueueControl.as_index()],
        MAX_FRAME_BATCH_PACKET_OVERHEAD + first_cost
    );
    assert_eq!(costs[Priority::QueueData.as_index()], data_cost);
    assert_eq!(costs[Priority::QueueReset.as_index()], reset_cost);
    assert_eq!(queues[Priority::QueueControl.as_index()].len(), 1);
    assert_eq!(queues[Priority::QueueData.as_index()].len(), 1);
    assert_eq!(queues[Priority::QueueReset.as_index()].len(), 1);
}

#[test]
fn batch_frames_groups_by_same_path_secret() {
    let path_a = test_path_secret_entry();
    let path_b = test_path_secret_entry();
    path_a.update_max_datagram_size(4_096);
    path_b.update_max_datagram_size(4_096);

    let rx = TestReceiver {
        values: VecDeque::from([
            new_test_frame(path_a.clone(), 16),
            new_test_frame(path_a.clone(), 16),
            new_test_frame(path_b.clone(), 16),
        ]),
        consumed: 0,
    };
    let mut batcher = BatchFramesByPathSecret::new(rx, &test_clock(), Rate::new(10.0));

    let first = with_noop_context(|cx| batcher.poll_recv(cx, &mut Budget::new(usize::MAX)));
    let Poll::Ready(Some(first)) = first else {
        panic!("expected first batch");
    };
    assert_eq!(first.len(), 2);
    assert!(Arc::ptr_eq(first.path_secret_entry(), &path_a));

    let second = with_noop_context(|cx| batcher.poll_recv(cx, &mut Budget::new(usize::MAX)));
    let Poll::Ready(Some(second)) = second else {
        panic!("expected second batch");
    };
    assert_eq!(second.len(), 1);
    assert!(Arc::ptr_eq(second.path_secret_entry(), &path_b));
}

#[test]
fn batch_frames_enforces_datagram_byte_budget() {
    let path = test_path_secret_entry();
    // target_bytes = u16::MAX - 3000 ≈ 62535. Use frames large enough to exceed it.
    let frame_size = 40_000;

    let rx = TestReceiver {
        values: VecDeque::from([
            new_test_frame(path.clone(), frame_size),
            new_test_frame(path.clone(), frame_size),
            new_test_frame(path.clone(), frame_size),
        ]),
        consumed: 0,
    };
    let mut batcher = BatchFramesByPathSecret::new(rx, &test_clock(), Rate::new(10.0));

    let target_bytes = u16::MAX as u64 - 3000;

    let first = with_noop_context(|cx| batcher.poll_recv(cx, &mut Budget::new(usize::MAX)));
    let Poll::Ready(Some(first)) = first else {
        panic!("expected first batch");
    };
    // First frame + overhead exceeds target, so only one frame per batch.
    assert_eq!(first.len(), 1);
    assert!(first.byte_cost() <= target_bytes);

    let second = with_noop_context(|cx| batcher.poll_recv(cx, &mut Budget::new(usize::MAX)));
    let Poll::Ready(Some(second)) = second else {
        panic!("expected second batch");
    };
    assert_eq!(second.len(), 1);

    let third = with_noop_context(|cx| batcher.poll_recv(cx, &mut Budget::new(usize::MAX)));
    let Poll::Ready(Some(third)) = third else {
        panic!("expected third batch");
    };
    assert_eq!(third.len(), 1);
}

#[test]
fn batch_frames_forwards_on_consumed() {
    let path = test_path_secret_entry();
    let rx = TestReceiver {
        values: VecDeque::from([new_test_frame(path, 0)]),
        consumed: 0,
    };
    let mut batcher = BatchFramesByPathSecret::new(rx, &test_clock(), Rate::new(10.0));

    batcher.on_consumed(321);
    assert_eq!(batcher.inner.consumed, 321);
}

#[test]
fn batch_frames_tracks_sticky_sender_from_first_frame() {
    let path = test_path_secret_entry();
    path.update_max_datagram_size(4_096);

    let rx = TestReceiver {
        values: VecDeque::from([
            new_test_frame_with_sender_id(path.clone(), 16, VarInt::from_u8(2)),
            new_test_frame(path.clone(), 16),
        ]),
        consumed: 0,
    };
    let mut batcher = BatchFramesByPathSecret::new(rx, &test_clock(), Rate::new(10.0));

    let first = with_noop_context(|cx| batcher.poll_recv(cx, &mut Budget::new(usize::MAX)));
    let Poll::Ready(Some(batch)) = first else {
        panic!("expected batch");
    };
    assert_eq!(batch.len(), 2);
    assert_eq!(
        batch.sender_id(),
        Some(crate::endpoint::id::LocalSenderId::from_index(2))
    );
}

#[test]
fn batch_frames_breaks_on_conflicting_sticky_senders() {
    let path = test_path_secret_entry();
    path.update_max_datagram_size(4_096);

    let rx = TestReceiver {
        values: VecDeque::from([
            new_test_frame_with_sender_id(path.clone(), 16, VarInt::from_u8(1)),
            new_test_frame_with_sender_id(path.clone(), 16, VarInt::from_u8(2)),
        ]),
        consumed: 0,
    };
    let mut batcher = BatchFramesByPathSecret::new(rx, &test_clock(), Rate::new(10.0));

    let first = with_noop_context(|cx| batcher.poll_recv(cx, &mut Budget::new(usize::MAX)));
    let Poll::Ready(Some(batch1)) = first else {
        panic!("expected first batch");
    };
    assert_eq!(batch1.len(), 1);
    assert_eq!(
        batch1.sender_id(),
        Some(crate::endpoint::id::LocalSenderId::from_index(1))
    );

    let second = with_noop_context(|cx| batcher.poll_recv(cx, &mut Budget::new(usize::MAX)));
    let Poll::Ready(Some(batch2)) = second else {
        panic!("expected second batch");
    };
    assert_eq!(batch2.len(), 1);
    assert_eq!(
        batch2.sender_id(),
        Some(crate::endpoint::id::LocalSenderId::from_index(2))
    );
}

#[test]
fn batch_frames_adopts_sticky_from_later_frame() {
    let path = test_path_secret_entry();
    path.update_max_datagram_size(4_096);

    let rx = TestReceiver {
        values: VecDeque::from([
            new_test_frame(path.clone(), 16),
            new_test_frame_with_sender_id(path.clone(), 16, VarInt::from_u8(3)),
            new_test_frame(path.clone(), 16),
        ]),
        consumed: 0,
    };
    let mut batcher = BatchFramesByPathSecret::new(rx, &test_clock(), Rate::new(10.0));

    let first = with_noop_context(|cx| batcher.poll_recv(cx, &mut Budget::new(usize::MAX)));
    let Poll::Ready(Some(batch)) = first else {
        panic!("expected batch");
    };
    assert_eq!(batch.len(), 3);
    assert_eq!(
        batch.sender_id(),
        Some(crate::endpoint::id::LocalSenderId::from_index(3))
    );
}

#[test]
fn ack_processor_drops_message_with_out_of_range_sender_idx() {
    const OUT_OF_RANGE_SENDER_ID: u64 = 42; // total_sender_ids is 1, so any value > 0 is invalid.

    let registry = crate::counter::Registry::default();
    let send_caches: crate::endpoint::id::IdMap<crate::endpoint::id::LocalSendSocketId, _> =
        vec![Rc::new(RefCell::new(send::Cache::new(
            &registry,
            crate::endpoint::id::LocalSenderId::from_index(0),
        )))]
        .into();
    let sender_idx_to_local = crate::endpoint::id::IdMap::<
        crate::endpoint::id::LocalSenderId,
        crate::endpoint::id::LocalSendSocketId,
    >::new(1, crate::endpoint::id::LocalSendSocketId::new(0));
    let (frame_tx, _frame_rx) = frame::submission_channel(1);
    let (tx_wheel_tx, _tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
    let (pto_wheel_tx, _pto_wheel_rx) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
    let (idle_wheel_tx, _idle_wheel_rx) = unsync::new_with_adapter::<send::IdleWheelAdapter>();
    let path_secret_entry = test_path_secret_entry();

    let ack_rx = TestReceiver {
        values: VecDeque::from([Entry::new(msg::Sender::ReceivedAck {
            local_sender_id: crate::endpoint::id::LocalSenderId::new(
                VarInt::new(OUT_OF_RANGE_SENDER_ID).expect("valid varint"),
            ),
            path_secret_entry,
            payload: BytesMut::new(),
            ack_delay: core::time::Duration::ZERO,
        })]),
        consumed: 0,
    };

    let processor = AckProcessor::new(
        ack_rx,
        send_caches,
        sender_idx_to_local,
        1,
        crate::time::bach::Clock::default(),
        crate::xorshift::Rng::new(),
        frame_tx,
        frame::PriorityInput::default(),
        frame::PriorityInput::default(),
        registry.register("!send.invalid_sender_idx"),
    );
    let rx = crate::socket::channel::Flatten::new(processor);
    let (immediate_tx, _immediate_rx) = unsync::new_with_adapter::<send::TxImmediateAdapter>();
    let mut router = crate::stream::endpoint::send::WheelRouter::new(
        rx,
        immediate_tx,
        tx_wheel_tx,
        pto_wheel_tx,
        idle_wheel_tx,
    );

    // The invalid sender_idx message is consumed (Flatten skips the None),
    // then the input is exhausted so the channel closes.
    let result = with_noop_context(|cx| router.poll_recv(cx, &mut Budget::new(usize::MAX)));
    assert_eq!(result, Poll::Ready(None));
}
