use super::*;
use crate::{
    byte_vec::ByteVec,
    clock::testing as test_clock_mod,
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::ByteCost,
    stream3::frame::{Header, TransmissionStatus, DEFAULT_TTL},
};
use bytes::{Bytes, BytesMut};
use core::task::Poll;
use s2n_quic_core::varint::VarInt;
use std::{
    collections::VecDeque,
    future::Future,
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
    fn sticky_sender_idx(&self) -> Option<usize> {
        self.sticky_sender
    }

    fn set_sender_id(&mut self, _id: usize) {}
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
    fn poll_recv(&mut self, _cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        Poll::Ready(self.values.pop_front())
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.consumed += bytes;
    }
}

fn test_path_secret_entry() -> Arc<PathSecretEntry> {
    let peer = "127.0.0.1:4433"
        .parse()
        .expect("failed to parse hardcoded loopback address 127.0.0.1:4433");
    PathSecretEntry::fake_with_socket_senders(peer, None, 2)
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
        source_sender_id: VarInt::MAX,
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
        header: Header::FlowControl {
            queue_pair: crate::packet::datagram::QueuePair {
                source_queue_id: VarInt::from_u8(0),
                dest_queue_id: VarInt::from_u8(1),
            },
            stream_id: VarInt::from_u8(0),
        },
        source_sender_id,
        payload,
        path_secret_entry,
        completion: None,
        status: TransmissionStatus::Pending,
        ttl: DEFAULT_TTL,
        transmission_time: None,
    })
}

fn with_noop_context<R>(f: impl FnOnce(&mut task::Context<'_>) -> R) -> R {
    let waker = s2n_quic_core::task::waker::noop();
    let mut cx = task::Context::from_waker(&waker);
    f(&mut cx)
}

// ── PickTwo tests ─────────────────────────────────────────────────────────

fn try_send_pick_two(
    value: TestItem,
    senders: &mut Vec<TestSender>,
    rng: &mut crate::xorshift::Rng,
) -> Result<(), TestItem> {
    PickTwo::<TestItem, TestReceiver<TestItem>, TestSender>::try_send_pick_two(value, senders, rng)
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
    let pick_two = PickTwo::new(rx, senders, crate::xorshift::Rng::new());
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
        Header::FlowData {
            queue_pair: crate::packet::datagram::QueuePair {
                source_queue_id: VarInt::from_u8(0),
                dest_queue_id: VarInt::from_u8(1),
            },
            stream_id: VarInt::from_u8(0),
            offset: VarInt::ZERO,
            is_fin: false,
        },
    );
    let data_cost = data.byte_cost();
    batch.push_with_cost(data, data_cost);

    let reset = new_test_frame_with_header(
        path,
        0,
        Header::FlowReset {
            dest_queue_id: VarInt::from_u8(1),
            stream_id: VarInt::from_u8(0),
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
        costs[Priority::FlowControl.as_index()],
        MAX_FRAME_BATCH_PACKET_OVERHEAD + first_cost
    );
    assert_eq!(costs[Priority::FlowData.as_index()], data_cost);
    assert_eq!(costs[Priority::FlowReset.as_index()], reset_cost);
    assert_eq!(queues[Priority::FlowControl.as_index()].len(), 1);
    assert_eq!(queues[Priority::FlowData.as_index()].len(), 1);
    assert_eq!(queues[Priority::FlowReset.as_index()].len(), 1);
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

    let first = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(first)) = first else {
        panic!("expected first batch");
    };
    assert_eq!(first.len(), 2);
    assert!(Arc::ptr_eq(first.path_secret_entry(), &path_a));

    let second = with_noop_context(|cx| batcher.poll_recv(cx));
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

    let first = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(first)) = first else {
        panic!("expected first batch");
    };
    // First frame + overhead exceeds target, so only one frame per batch.
    assert_eq!(first.len(), 1);
    assert!(first.byte_cost() <= target_bytes);

    let second = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(second)) = second else {
        panic!("expected second batch");
    };
    assert_eq!(second.len(), 1);

    let third = with_noop_context(|cx| batcher.poll_recv(cx));
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

    let first = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(batch)) = first else {
        panic!("expected batch");
    };
    assert_eq!(batch.len(), 2);
    assert_eq!(batch.sender_id(), Some(2));
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

    let first = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(batch1)) = first else {
        panic!("expected first batch");
    };
    assert_eq!(batch1.len(), 1);
    assert_eq!(batch1.sender_id(), Some(1));

    let second = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(batch2)) = second else {
        panic!("expected second batch");
    };
    assert_eq!(batch2.len(), 1);
    assert_eq!(batch2.sender_id(), Some(2));
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

    let first = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(batch)) = first else {
        panic!("expected batch");
    };
    assert_eq!(batch.len(), 3);
    assert_eq!(batch.sender_id(), Some(3));
}

#[test]
fn ack_processor_drops_message_with_out_of_range_sender_idx() {
    const OUT_OF_RANGE_SENDER_ID: u64 = 42; // total_sender_ids is 1, so any value > 0 is invalid.

    let registry = crate::counter::Registry::default();
    let send_caches = vec![Rc::new(RefCell::new(send::Cache::new(&registry, 0)))];
    let sender_idx_to_local = vec![0];
    let (frame_tx, _frame_rx) = crate::stream3::frame::submission_channel(1);
    let (tx_wheel_tx, _tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
    let (pto_wheel_tx, _pto_wheel_rx) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
    let (idle_wheel_tx, _idle_wheel_rx) = unsync::new_with_adapter::<send::IdleWheelAdapter>();
    let path_secret_entry = test_path_secret_entry();

    let ack_rx = TestReceiver {
        values: VecDeque::from([Entry::new(msg::Sender::ReceivedAck {
            local_sender_id: VarInt::new(OUT_OF_RANGE_SENDER_ID).expect("valid varint"),
            path_secret_entry,
            payload: BytesMut::new(),
        })]),
        consumed: 0,
    };

    let mut processor = AckProcessor::new(
        ack_rx,
        send_caches,
        sender_idx_to_local,
        1,
        crate::clock::bach::Clock::default(),
        crate::xorshift::Rng::new(),
        frame_tx,
        crate::stream3::frame::PriorityInput::default(),
        crate::stream3::frame::PriorityInput::default(),
        tx_wheel_tx,
        pto_wheel_tx,
        idle_wheel_tx,
        crate::stream3::endpoint::counters::Send::new(&registry),
        registry.register_queue_gauge("q.tx_wheel"),
    );

    let first = with_noop_context(|cx| processor.poll_recv(cx));
    assert_eq!(first, Poll::Ready(Some(())));

    let second = with_noop_context(|cx| processor.poll_recv(cx));
    assert_eq!(second, Poll::Ready(None));
}
