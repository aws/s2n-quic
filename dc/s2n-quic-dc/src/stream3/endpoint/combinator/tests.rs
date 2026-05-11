use super::*;
use crate::{
    byte_vec::ByteVec,
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::ByteCost,
    stream3::frame::{Header, TransmissionStatus, DEFAULT_TTL},
};
use bytes::Bytes;
use core::{mem::MaybeUninit, task::Poll};
use s2n_quic_core::varint::VarInt;
use std::{
    collections::VecDeque,
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

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
}

#[derive(Clone, Copy)]
enum SenderBehavior {
    Pending,
    ReadyOk,
    ReadyErr,
}

struct TestSender {
    behavior: SenderBehavior,
    calls: usize,
}

impl Sender<TestItem> for TestSender {
    fn poll_send(
        &mut self,
        _cx: &mut task::Context<'_>,
        value: &mut MaybeUninit<TestItem>,
    ) -> Poll<Result<(), ()>> {
        self.calls += 1;

        match self.behavior {
            SenderBehavior::Pending => Poll::Pending,
            SenderBehavior::ReadyOk => {
                // SAFETY: successful send consumes the value.
                unsafe { value.assume_init_drop() };
                Poll::Ready(Ok(()))
            }
            SenderBehavior::ReadyErr => Poll::Ready(Err(())),
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
    PathSecretEntry::fake(peer, None)
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
        header: Header::Control {
            dest_sender_id: VarInt::from_u8(1),
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

#[test]
fn selected_sender_is_polled_before_alternates() {
    let mut slot = MaybeUninit::new(new_test_item(
        test_path_secret_entry(),
        Arc::new(AtomicUsize::new(0)),
    ));
    let mut senders = vec![
        TestSender {
            behavior: SenderBehavior::ReadyOk,
            calls: 0,
        },
        TestSender {
            behavior: SenderBehavior::ReadyOk,
            calls: 0,
        },
    ];
    let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
    assert_eq!(result, Poll::Ready(true));
    assert_eq!(senders[0].calls, 1);
    assert_eq!(senders[1].calls, 0);
}

#[test]
fn falls_back_to_alternate_sender_when_selected_sender_is_pending() {
    let mut slot = MaybeUninit::new(new_test_item(
        test_path_secret_entry(),
        Arc::new(AtomicUsize::new(0)),
    ));
    let mut senders = vec![
        TestSender {
            behavior: SenderBehavior::Pending,
            calls: 0,
        },
        TestSender {
            behavior: SenderBehavior::ReadyOk,
            calls: 0,
        },
    ];
    let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
    assert_eq!(result, Poll::Ready(true));
    assert_eq!(senders[0].calls, 1);
    assert_eq!(senders[1].calls, 1);
}

#[test]
fn shuts_down_on_sender_error() {
    let mut slot = MaybeUninit::new(new_test_item(
        test_path_secret_entry(),
        Arc::new(AtomicUsize::new(0)),
    ));
    let mut senders = vec![
        TestSender {
            behavior: SenderBehavior::ReadyErr,
            calls: 0,
        },
        TestSender {
            behavior: SenderBehavior::ReadyOk,
            calls: 0,
        },
    ];
    let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
    assert_eq!(result, Poll::Ready(false));
    assert_eq!(senders[0].calls, 1);
    assert_eq!(senders[1].calls, 0);

    // SAFETY: `Err` keeps the value in slot and caller must drop it.
    unsafe { slot.assume_init_drop() };
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
    let senders = vec![TestSender {
        behavior: SenderBehavior::ReadyErr,
        calls: 0,
    }];
    let pick_two = PickTwo::new(rx, senders, |_| 0);
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
    let mut slot = MaybeUninit::new(item);
    let mut senders = vec![
        TestSender {
            behavior: SenderBehavior::ReadyOk,
            calls: 0,
        },
        TestSender {
            behavior: SenderBehavior::ReadyOk,
            calls: 0,
        },
    ];
    // random would pick sender 0, but sticky says sender 1
    let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
    assert_eq!(result, Poll::Ready(true));
    assert_eq!(senders[0].calls, 0);
    assert_eq!(senders[1].calls, 1);
}

#[test]
fn sticky_sender_does_not_fallback_on_pending() {
    let mut item = new_test_item(test_path_secret_entry(), Arc::new(AtomicUsize::new(0)));
    item.sticky_sender = Some(0);
    let mut slot = MaybeUninit::new(item);
    let mut senders = vec![
        TestSender {
            behavior: SenderBehavior::Pending,
            calls: 0,
        },
        TestSender {
            behavior: SenderBehavior::ReadyOk,
            calls: 0,
        },
    ];
    let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
    assert_eq!(result, Poll::Pending);
    assert_eq!(senders[0].calls, 1);
    assert_eq!(senders[1].calls, 0);

    // SAFETY: Pending keeps the value in slot.
    unsafe { slot.assume_init_drop() };
}

// ── BatchFramesByPathSecret tests ─────────────────────────────────────────

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
    let mut batcher = BatchFramesByPathSecret::new(rx);

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
    path.update_max_datagram_size(220);

    let rx = TestReceiver {
        values: VecDeque::from([
            new_test_frame(path.clone(), 70),
            new_test_frame(path.clone(), 70),
            new_test_frame(path.clone(), 70),
        ]),
        consumed: 0,
    };
    let mut batcher = BatchFramesByPathSecret::new(rx);

    let first = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(first)) = first else {
        panic!("expected first batch");
    };
    assert_eq!(first.len(), 1);
    assert!(first.byte_cost() <= 220);
    let frame_cost = first
        .queue()
        .peek_front()
        .expect("batch must contain the first frame")
        .byte_cost();
    assert!(first.byte_cost().saturating_add(frame_cost) > 220);

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
    let mut batcher = BatchFramesByPathSecret::new(rx);

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
    let mut batcher = BatchFramesByPathSecret::new(rx);

    let first = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(batch)) = first else {
        panic!("expected batch");
    };
    assert_eq!(batch.len(), 2);
    assert_eq!(batch.sticky_sender_idx(), Some(2));
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
    let mut batcher = BatchFramesByPathSecret::new(rx);

    let first = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(batch1)) = first else {
        panic!("expected first batch");
    };
    assert_eq!(batch1.len(), 1);
    assert_eq!(batch1.sticky_sender_idx(), Some(1));

    let second = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(batch2)) = second else {
        panic!("expected second batch");
    };
    assert_eq!(batch2.len(), 1);
    assert_eq!(batch2.sticky_sender_idx(), Some(2));
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
    let mut batcher = BatchFramesByPathSecret::new(rx);

    let first = with_noop_context(|cx| batcher.poll_recv(cx));
    let Poll::Ready(Some(batch)) = first else {
        panic!("expected batch");
    };
    assert_eq!(batch.len(), 3);
    assert_eq!(batch.sticky_sender_idx(), Some(3));
}
