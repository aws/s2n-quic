// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{intrusive, testing::sim};
use core::{cell::Cell, future::Future, pin::pin};
use std::rc::Rc;

trait SenderExt<T>: Sender<T> {
    async fn send(&mut self, value: T) -> Result<(), T> {
        let mut slot = core::mem::MaybeUninit::new(value);
        let mut taken = false;
        core::future::poll_fn(move |cx| {
            if taken {
                return Poll::Ready(Ok(()));
            }
            match self.poll_send(cx, &mut slot) {
                Poll::Ready(Ok(())) => {
                    taken = true;
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(())) => {
                    taken = true;
                    Poll::Ready(Err(unsafe { slot.assume_init_read() }))
                }
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }
}

impl<T, S: Sender<T>> SenderExt<T> for S {}

fn noop_cx() -> core::task::Context<'static> {
    let waker = s2n_quic_core::task::waker::noop();
    let waker = Box::leak(Box::new(waker));
    core::task::Context::from_waker(waker)
}

// ── cell tests (poll-based only, since cell is !Send) ──────────────

#[test]
fn cell_poll_recv_empty_returns_pending() {
    let (_tx, mut rx) = cell::unsync::new::<u32>();
    let mut cx = noop_cx();
    let mut budget = Budget::new(usize::MAX);
    assert!(matches!(rx.poll_recv(&mut cx, &mut budget), Poll::Pending));
}

#[test]
fn cell_sender_drop_closes_receiver() {
    let (tx, mut rx) = cell::unsync::new::<u32>();
    drop(tx);
    let mut cx = noop_cx();
    let mut budget = Budget::new(usize::MAX);
    assert!(matches!(
        rx.poll_recv(&mut cx, &mut budget),
        Poll::Ready(None)
    ));
}

#[test]
fn cell_receiver_drop_closes_sender() {
    // Sender's poll sees closed when receiver drops
    let (mut tx, rx) = cell::unsync::new::<u32>();
    drop(rx);
    // Use poll_fn to test send
    let mut cx = noop_cx();
    let mut fut = pin!(tx.send(42));
    assert_eq!(fut.as_mut().poll(&mut cx), Poll::Ready(Err(42)));
}

#[test]
fn cell_send_recv_poll_roundtrip() {
    let (mut tx, mut rx) = cell::unsync::new::<u32>();
    let mut cx = noop_cx();
    let mut budget = Budget::new(usize::MAX);

    // Empty — pending
    assert!(matches!(rx.poll_recv(&mut cx, &mut budget), Poll::Pending));

    // Send via poll
    {
        let mut fut = pin!(tx.send(42));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }

    // Receive
    assert!(matches!(
        rx.poll_recv(&mut cx, &mut budget),
        Poll::Ready(Some(42))
    ));

    // Empty again
    assert!(matches!(rx.poll_recv(&mut cx, &mut budget), Poll::Pending));

    // Send again
    {
        let mut fut = pin!(tx.send(99));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert!(matches!(
        rx.poll_recv(&mut cx, &mut budget),
        Poll::Ready(Some(99))
    ));

    // Drop sender → closed
    drop(tx);
    assert!(matches!(
        rx.poll_recv(&mut cx, &mut budget),
        Poll::Ready(None)
    ));
}

#[test]
fn cell_backpressure_poll() {
    let (mut tx, mut rx) = cell::unsync::new::<u32>();
    let mut cx = noop_cx();
    let mut budget = Budget::new(usize::MAX);

    // Send one value
    {
        let mut fut = pin!(tx.send(1));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }

    // Slot is full — send returns Pending
    {
        let mut fut = pin!(tx.send(2));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Pending));
    }

    // Drain the slot
    assert!(matches!(
        rx.poll_recv(&mut cx, &mut budget),
        Poll::Ready(Some(1))
    ));

    // Now send succeeds
    {
        let mut fut = pin!(tx.send(3));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert!(matches!(
        rx.poll_recv(&mut cx, &mut budget),
        Poll::Ready(Some(3))
    ));
}

// ── slot tests ─────────────────────────────────────────────────────

#[test]
fn slot_poll_recv_empty_returns_pending() {
    let (_tx, mut rx) = cell::sync::new::<u32>();
    let mut cx = noop_cx();
    let mut budget = Budget::new(usize::MAX);
    assert!(matches!(rx.poll_recv(&mut cx, &mut budget), Poll::Pending));
}

#[test]
fn slot_sender_drop_closes_receiver() {
    let (tx, mut rx) = cell::sync::new::<u32>();
    drop(tx);
    let mut cx = noop_cx();
    let mut budget = Budget::new(usize::MAX);
    assert!(matches!(
        rx.poll_recv(&mut cx, &mut budget),
        Poll::Ready(None)
    ));
}

#[test]
fn slot_receiver_drop_closes_sender() {
    sim(|| {
        use crate::testing::ext::*;

        async {
            let (mut tx, rx) = cell::sync::new::<u32>();
            drop(rx);
            let result = tx.send(42).await;
            assert_eq!(result, Err(42));
        }
        .primary()
        .spawn();
    });
}

#[test]
fn slot_send_recv_roundtrip() {
    sim(|| {
        use crate::testing::ext::*;

        async {
            let (mut tx, mut rx) = cell::sync::new::<u32>();
            let mut budget = Budget::new(usize::MAX);

            tx.send(42).await.unwrap();
            let val = rx.recv(&mut budget).await;
            assert_eq!(val, Some(42));

            tx.send(99).await.unwrap();
            let val = rx.recv(&mut budget).await;
            assert_eq!(val, Some(99));

            drop(tx);
            let val = rx.recv(&mut budget).await;
            assert_eq!(val, None);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn slot_backpressure() {
    let (mut tx, mut rx) = cell::sync::new::<u32>();
    let mut cx = noop_cx();
    let mut budget = Budget::new(usize::MAX);

    // Send one value via poll
    {
        let mut fut = pin!(tx.send(1));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }

    assert!(matches!(
        rx.poll_recv(&mut cx, &mut budget),
        Poll::Ready(Some(1))
    ));
    assert!(matches!(rx.poll_recv(&mut cx, &mut budget), Poll::Pending));

    {
        let mut fut = pin!(tx.send(2));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert!(matches!(
        rx.poll_recv(&mut cx, &mut budget),
        Poll::Ready(Some(2))
    ));
}

#[test]
fn slot_concurrent_send_recv() {
    sim(|| {
        use crate::testing::ext::*;

        let (mut tx, mut rx) = cell::sync::new::<u32>();

        async move {
            for i in 0..10 {
                tx.send(i).await.unwrap();
            }
        }
        .spawn();

        async move {
            let mut budget = Budget::new(usize::MAX);
            for expected in 0..10 {
                let val = rx.recv(&mut budget).await.unwrap();
                assert_eq!(val, expected);
            }
            let val = rx.recv(&mut budget).await;
            assert_eq!(val, None);
        }
        .primary()
        .spawn();
    });
}

// ── Priority tests (using slot since sim requires Send) ────────────

#[test]
fn priority_empty_returns_pending() {
    let (_tx0, rx0) = cell::sync::new::<u32>();
    let (_tx1, rx1) = cell::sync::new::<u32>();
    let mut priority = Priority::new(vec![rx0, rx1]);

    let mut cx = noop_cx();
    let mut budget = Budget::new(usize::MAX);
    assert!(matches!(
        priority.poll_recv(&mut cx, &mut budget),
        Poll::Pending
    ));
}

#[test]
fn priority_all_closed_returns_none() {
    let (tx0, rx0) = cell::sync::new::<u32>();
    let (tx1, rx1) = cell::sync::new::<u32>();
    drop(tx0);
    drop(tx1);
    let mut priority = Priority::new(vec![rx0, rx1]);

    let mut cx = noop_cx();
    let mut budget = Budget::new(usize::MAX);
    assert!(matches!(
        priority.poll_recv(&mut cx, &mut budget),
        Poll::Ready(None)
    ));
}

#[test]
fn priority_high_wins_over_low() {
    sim(|| {
        use crate::testing::ext::*;

        async {
            let (mut tx0, rx0) = cell::sync::new::<u32>();
            let (mut tx1, rx1) = cell::sync::new::<u32>();
            let mut priority = Priority::new(vec![rx0, rx1]);
            let mut budget = Budget::new(usize::MAX);

            tx0.send(10).await.unwrap();
            tx1.send(20).await.unwrap();

            let val = priority.recv(&mut budget).await;
            assert_eq!(val, Some(10));

            let val = priority.recv(&mut budget).await;
            assert_eq!(val, Some(20));
        }
        .primary()
        .spawn();
    });
}

#[test]
fn priority_low_priority_works_when_high_empty() {
    sim(|| {
        use crate::testing::ext::*;

        async {
            let (_tx0, rx0) = cell::sync::new::<u32>();
            let (mut tx1, rx1) = cell::sync::new::<u32>();
            let mut priority = Priority::new(vec![rx0, rx1]);
            let mut budget = Budget::new(usize::MAX);

            tx1.send(99).await.unwrap();

            let val = priority.recv(&mut budget).await;
            assert_eq!(val, Some(99));
        }
        .primary()
        .spawn();
    });
}

#[test]
fn priority_partial_close() {
    sim(|| {
        use crate::testing::ext::*;

        async {
            let (tx0, rx0) = cell::sync::new::<u32>();
            let (mut tx1, rx1) = cell::sync::new::<u32>();
            let mut priority = Priority::new(vec![rx0, rx1]);
            let mut budget = Budget::new(usize::MAX);

            drop(tx0);

            tx1.send(42).await.unwrap();
            let val = priority.recv(&mut budget).await;
            assert_eq!(val, Some(42));

            drop(tx1);
            let val = priority.recv(&mut budget).await;
            assert_eq!(val, None);
        }
        .primary()
        .spawn();
    });
}

struct CountingOnConsumed<R> {
    inner: R,
    consumed: Rc<Cell<u64>>,
}

impl<R> CountingOnConsumed<R> {
    fn new(inner: R, consumed: Rc<Cell<u64>>) -> Self {
        Self { inner, consumed }
    }
}

impl<T, R> Receiver<T> for CountingOnConsumed<R>
where
    R: Receiver<T>,
{
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<T>> {
        self.inner.poll_recv(cx, budget)
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.consumed.set(self.consumed.get() + bytes);
        self.inner.on_consumed(bytes);
    }
}

#[test]
fn priority_select_on_consumed_notifies_last_ready_receiver() {
    let _no_snap = crate::testing::without_snapshots();
    sim(|| {
        use crate::testing::ext::*;

        async {
            let (mut priority_tx, priority_rx) = cell::sync::new::<u32>();
            let (mut fallback_tx, fallback_rx) = cell::sync::new::<u32>();

            let priority_consumed = Rc::new(Cell::new(0));
            let fallback_consumed = Rc::new(Cell::new(0));

            let mut select = PrioritySelect::new(
                CountingOnConsumed::new(priority_rx, priority_consumed.clone()),
                CountingOnConsumed::new(fallback_rx, fallback_consumed.clone()),
            );
            let mut budget = Budget::new(usize::MAX);

            // Only fallback item queued: priority was empty so status is Empty.
            fallback_tx.send(1).await.unwrap();
            let (val, status) = select.recv(&mut budget).await.unwrap();
            assert_eq!(val, 1);
            assert_eq!(status, ImmediateQueueStatus::Empty, "priority was empty");
            select.on_consumed(5);
            assert_eq!(priority_consumed.get(), 0);
            assert_eq!(fallback_consumed.get(), 5);

            // Priority item queued alongside a fallback item: priority wins.
            // No second priority item queued, so status is Empty.
            priority_tx.send(2).await.unwrap();
            fallback_tx.send(3).await.unwrap();
            let (val, status) = select.recv(&mut budget).await.unwrap();
            assert_eq!(val, 2);
            assert_eq!(
                status,
                ImmediateQueueStatus::Empty,
                "only one priority item was queued"
            );
            select.on_consumed(7);
            assert_eq!(priority_consumed.get(), 7);
            assert_eq!(fallback_consumed.get(), 5);
        }
        .primary()
        .spawn();
    });
}

/// Verify the peek look-ahead: when two priority items are queued, the first
/// poll returns `HasMore` and the second returns `Empty`.
#[test]
fn priority_select_has_more_when_priority_queue_not_empty() {
    let _no_snap = crate::testing::without_snapshots();
    sim(|| {
        use crate::{socket::channel::intrusive::unsync as ch_unsync, testing::ext::*};

        async {
            // Use unsync channels so we can enqueue multiple items at once.
            let (mut imm_tx, imm_rx) = ch_unsync::new::<u32>();
            let (_ctx_tx, ctx_rx) = ch_unsync::new::<u32>();
            let mut select = PrioritySelect::new(imm_rx, ctx_rx);
            let mut budget = Budget::new(usize::MAX);

            // Enqueue two priority items; no fallback items.
            UnboundedSender::send(&mut imm_tx, intrusive::Entry::new(10)).expect("send 10");
            UnboundedSender::send(&mut imm_tx, intrusive::Entry::new(20)).expect("send 20");

            // First poll: one item queued behind → HasMore.
            let (entry, status) = select.recv(&mut budget).await.unwrap();
            assert_eq!(*entry, 10);
            assert_eq!(
                status,
                ImmediateQueueStatus::HasMore,
                "item 20 was still queued"
            );

            // Second poll: peeked item returned, nothing behind → Empty.
            let (entry, status) = select.recv(&mut budget).await.unwrap();
            assert_eq!(*entry, 20);
            assert_eq!(status, ImmediateQueueStatus::Empty, "queue was drained");
        }
        .primary()
        .spawn();
    });
}

// ── Flatten tests (using slot since sim requires Send) ─────────────

#[test]
fn flatten_drains_queue_then_fetches_next() {
    sim(|| {
        use crate::testing::ext::*;

        async {
            let (mut tx, rx) = cell::sync::new::<intrusive::Queue<u32>>();
            let mut flat = Flatten::new(rx);
            let mut budget = Budget::new(usize::MAX);

            let mut queue = intrusive::Queue::default();
            queue.push_back(intrusive::Entry::new(10));
            queue.push_back(intrusive::Entry::new(20));
            queue.push_back(intrusive::Entry::new(30));

            assert!(tx.send(queue).await.is_ok());

            assert_eq!(*flat.recv(&mut budget).await.unwrap(), 10);
            assert_eq!(*flat.recv(&mut budget).await.unwrap(), 20);
            assert_eq!(*flat.recv(&mut budget).await.unwrap(), 30);

            let mut queue2 = intrusive::Queue::default();
            queue2.push_back(intrusive::Entry::new(40));
            assert!(tx.send(queue2).await.is_ok());

            assert_eq!(*flat.recv(&mut budget).await.unwrap(), 40);

            drop(tx);
            let val = flat.recv(&mut budget).await;
            assert!(val.is_none());
        }
        .primary()
        .spawn();
    });
}

#[test]
fn flatten_empty_queue_skipped() {
    sim(|| {
        use crate::testing::ext::*;

        let (mut tx, rx) = cell::sync::new::<intrusive::Queue<u32>>();

        // Sender task: send empty queue, then non-empty queue
        async move {
            let empty_queue = intrusive::Queue::default();
            assert!(tx.send(empty_queue).await.is_ok());

            let mut queue = intrusive::Queue::default();
            queue.push_back(intrusive::Entry::new(42));
            assert!(tx.send(queue).await.is_ok());
        }
        .spawn();

        // Receiver task: should skip the empty queue and return 42
        async move {
            let mut flat = Flatten::new(rx);
            let mut budget = Budget::new(usize::MAX);
            assert_eq!(*flat.recv(&mut budget).await.unwrap(), 42);
        }
        .primary()
        .spawn();
    });
}

// ── Paced tests ────────────────────────────────────────────────────

#[test]
fn paced_limits_rate() {
    sim(|| {
        use crate::{testing::ext::*, time::precision::Clock as _};

        async {
            let (mut tx, rx) = cell::sync::new::<u32>();
            let clock = crate::time::bach::Clock::default();
            // 1 Gbps = 8 nanos per byte
            let rate = crate::socket::rate::Rate::new(1.0);
            let mut paced_rx = super::Paced::new(rx, clock.clone(), rate);
            let mut budget = Budget::new(usize::MAX);

            let start = clock.now();

            // Send 100KB total - exceeds burst capacity of 64KB
            for _ in 0..100 {
                tx.send(1000).await.unwrap();
                let val = paced_rx.recv(&mut budget).await;
                assert_eq!(val, Some(1000));
                paced_rx.on_consumed(1000);
            }

            let elapsed = clock.now().duration_since(start);
            // 100KB at 1 Gbps = 800,000 nanos = 0.8ms
            // With burst capacity of 64KB, the burst handles first 64KB fast,
            // then remaining 36KB paces out. Should still see significant elapsed time.
            assert!(elapsed.as_nanos() >= 100_000, "elapsed: {:?}", elapsed);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn paced_skips_cancelled_packets() {
    sim(|| {
        use crate::{testing::ext::*, time::precision::Clock as _};

        async {
            let (mut tx, rx) = cell::sync::new::<u32>();
            let clock = crate::time::bach::Clock::default();
            let rate = crate::socket::rate::Rate::new(1.0);
            let mut paced_rx = super::Paced::new(rx, clock.clone(), rate);
            let mut budget = Budget::new(usize::MAX);

            // Receive first packet but DON'T call on_consumed (simulating cancellation)
            tx.send(1000).await.unwrap();
            let val = paced_rx.recv(&mut budget).await;
            assert_eq!(val, Some(1000));
            // Intentionally not calling on_consumed

            // Next packet should not be paced since we didn't consume the first one
            tx.send(1000).await.unwrap();
            let start = clock.now();
            let val = paced_rx.recv(&mut budget).await;
            assert_eq!(val, Some(1000));
            let elapsed = clock.now().duration_since(start);

            // Should be immediate since we didn't consume tokens for the first packet
            assert!(elapsed.as_nanos() < 1000, "elapsed: {:?}", elapsed);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn paced_throughput_test() {
    sim(|| {
        use crate::{testing::ext::*, time::precision::Clock as _};

        async {
            let (mut tx, rx) = cell::sync::new::<usize>();
            let clock = crate::time::bach::Clock::default();
            // 5 Gbps
            let rate = crate::socket::rate::Rate::new(5.0);
            let paced_rx = super::Paced::new(rx, clock.clone(), rate);
            let mut paced_rx = super::Reporter::new(paced_rx, clock.clone(), false);
            let mut budget = Budget::new(usize::MAX);

            // Producer task: send 64KB packets continuously for 5 seconds worth of data
            // At 5Gbps, that's 5 * 5 / 8 = ~3GB = ~47,000 packets of 64KB
            async move {
                for _ in 0..47000 {
                    tx.send(65536).await.unwrap();
                }
            }
            .spawn();

            // Consumer task: receive and discard
            let start = clock.now();
            let mut count = 0;
            let mut total_bytes = 0u64;
            while let Some(bytes) = paced_rx.recv(&mut budget).await {
                total_bytes += bytes as u64;
                // Simulate consuming the packet
                paced_rx.on_consumed(bytes as u64);
                count += 1;
            }
            let elapsed = clock.now().duration_since(start);

            // 47000 * 64KB at 5Gbps = ~5 seconds
            assert_eq!(count, 47000);
            let elapsed_sec = elapsed.as_secs_f64();
            let gbps = (total_bytes as f64 * 8.0) / elapsed_sec / 1_000_000_000.0;
            info!(
                "Received {} packets ({} GB) in {:.2}s = {:.2} Gbps",
                count,
                total_bytes / 1_000_000_000,
                elapsed_sec,
                gbps
            );

            // Should take around 5 seconds due to pacing
            assert!(
                (4.5..=6.0).contains(&elapsed_sec),
                "elapsed: {:.2}s",
                elapsed_sec
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn paced_allows_burst() {
    sim(|| {
        use crate::{testing::ext::*, time::precision::Clock as _};

        async {
            let (mut tx, rx) = cell::sync::new::<u32>();
            let clock = crate::time::bach::Clock::default();
            let rate = crate::socket::rate::Rate::new(1.0);
            let mut paced_rx = super::Paced::new(rx, clock.clone(), rate);
            let mut budget = Budget::new(usize::MAX);

            let start = clock.now();

            // Send 64KB (burst capacity) - should go through without pacing
            for _ in 0..64 {
                tx.send(1000).await.unwrap();
                let val = paced_rx.recv(&mut budget).await;
                assert_eq!(val, Some(1000));
                paced_rx.on_consumed(1000);
            }

            let elapsed = clock.now().duration_since(start);

            // With burst capacity, first 64KB should go fast
            // Total time should be less than strict pacing would allow
            // 64KB at 1Gbps = 512,000 nanos, but burst means much of it goes faster
            assert!(elapsed.as_nanos() < 512_000, "elapsed: {:?}", elapsed);
        }
        .primary()
        .spawn();
    });
}

// ── GaugedSender / GaugedReceiver tests ───────────────────────────────────

mod gauged {
    use super::*;
    use crate::counter::Registry;

    fn make_gauge() -> (crate::counter::QueueGauge, Registry) {
        let reg = Registry::new();
        let gauge = reg.register_queue_gauge("test");
        (gauge, reg)
    }

    fn gauge_depth(gauge: &crate::counter::QueueGauge) -> i64 {
        gauge.depth.get()
    }

    // Alias the channel-specific intrusive_queue::sync module to avoid
    // shadowing by the crate-level `use crate::{intrusive_queue, ...}` import.
    use crate::socket::channel::intrusive::sync as iq_sync;

    #[test]
    fn batch_len_single_entry() {
        let e = crate::intrusive::Entry::new(42u32);
        assert_eq!(e.batch_len(), 1);
    }

    #[test]
    fn batch_len_queue() {
        let mut q = crate::intrusive::Queue::<u32>::new();
        assert_eq!(q.batch_len(), 0);
        q.push_back(crate::intrusive::Entry::new(1u32));
        q.push_back(crate::intrusive::Entry::new(2u32));
        q.push_back(crate::intrusive::Entry::new(3u32));
        assert_eq!(q.batch_len(), 3);
    }

    #[test]
    fn batch_len_vecdeque() {
        let mut v: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
        assert_eq!(v.batch_len(), 0);
        v.push_back(1);
        v.push_back(2);
        assert_eq!(v.batch_len(), 2);
    }

    #[test]
    fn gauged_sender_unbounded_increments_depth_on_send() {
        // intrusive_queue::sync::Sender<T> implements UnboundedSender<Entry<T>>
        let (gauge, _reg) = make_gauge();
        let (inner_tx, _rx) = iq_sync::new::<u32>();

        let mut tx: GaugedSender<_, crate::intrusive::Entry<u32>> =
            GaugedSender::new(inner_tx, gauge.clone());

        assert_eq!(gauge_depth(&gauge), 0);
        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive::Entry::new(10u32),
        )
        .unwrap();
        assert_eq!(gauge_depth(&gauge), 1);
        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive::Entry::new(20u32),
        )
        .unwrap();
        assert_eq!(gauge_depth(&gauge), 2);
    }

    #[test]
    fn gauged_sender_unbounded_does_not_increment_on_closed_channel() {
        let (gauge, _reg) = make_gauge();
        let (inner_tx, rx) = iq_sync::new::<u32>();
        drop(rx);

        let mut tx: GaugedSender<_, crate::intrusive::Entry<u32>> =
            GaugedSender::new(inner_tx, gauge.clone());

        let result = <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive::Entry::new(42u32),
        );
        assert!(result.is_err(), "send should fail on closed channel");
        assert_eq!(gauge_depth(&gauge), 0, "depth must not increase on failure");
    }

    #[test]
    fn gauged_sender_batch_counts_queue_len() {
        // Sending a Queue<T> with 3 items should increment depth by 3
        let (gauge, _reg) = make_gauge();
        let (inner_tx, _rx) = iq_sync::new::<u32>();

        let mut tx: GaugedSender<_, crate::intrusive::Queue<u32>> =
            GaugedSender::new(inner_tx, gauge.clone());

        assert_eq!(gauge_depth(&gauge), 0);

        let mut batch = crate::intrusive::Queue::new();
        batch.push_back(crate::intrusive::Entry::new(1u32));
        batch.push_back(crate::intrusive::Entry::new(2u32));
        batch.push_back(crate::intrusive::Entry::new(3u32));
        <GaugedSender<_, _> as UnboundedSender<_>>::send(&mut tx, batch).unwrap();

        assert_eq!(gauge_depth(&gauge), 3);
    }

    #[test]
    fn gauged_receiver_single_item_decrements_depth_by_one() {
        let (gauge, _reg) = make_gauge();
        let (inner_tx, inner_rx) = iq_sync::new::<u32>();

        // Pre-populate gauge depth to simulate earlier enqueues
        gauge.enqueue(2);
        inner_tx
            .send_entry(crate::intrusive::Entry::new(1u32))
            .unwrap();
        inner_tx
            .send_entry(crate::intrusive::Entry::new(2u32))
            .unwrap();

        let mut rx: GaugedReceiver<_, crate::intrusive::Entry<u32>> =
            GaugedReceiver::new(inner_rx, gauge.clone());

        let mut cx = noop_cx();
        let mut budget = Budget::new(usize::MAX);

        assert!(matches!(
            rx.poll_recv(&mut cx, &mut budget),
            Poll::Ready(Some(_))
        ));
        assert_eq!(gauge_depth(&gauge), 1);

        assert!(matches!(
            rx.poll_recv(&mut cx, &mut budget),
            Poll::Ready(Some(_))
        ));
        assert_eq!(gauge_depth(&gauge), 0);
    }

    #[test]
    fn gauged_sender_and_receiver_paired_track_depth() {
        let (gauge, _reg) = make_gauge();
        let (inner_tx, inner_rx) = iq_sync::new::<u32>();

        let mut tx: GaugedSender<_, crate::intrusive::Entry<u32>> =
            GaugedSender::new(inner_tx, gauge.clone());
        let mut rx: GaugedReceiver<_, crate::intrusive::Entry<u32>> =
            GaugedReceiver::new(inner_rx, gauge.clone());

        let mut cx = noop_cx();
        let mut budget = Budget::new(usize::MAX);

        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive::Entry::new(1u32),
        )
        .unwrap();
        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive::Entry::new(2u32),
        )
        .unwrap();
        assert_eq!(gauge_depth(&gauge), 2);

        assert!(matches!(
            rx.poll_recv(&mut cx, &mut budget),
            Poll::Ready(Some(_))
        ));
        assert_eq!(gauge_depth(&gauge), 1);

        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive::Entry::new(3u32),
        )
        .unwrap();
        assert_eq!(gauge_depth(&gauge), 2);

        assert!(matches!(
            rx.poll_recv(&mut cx, &mut budget),
            Poll::Ready(Some(_))
        ));
        assert!(matches!(
            rx.poll_recv(&mut cx, &mut budget),
            Poll::Ready(Some(_))
        ));
        assert_eq!(gauge_depth(&gauge), 0);
    }

    #[test]
    fn gauged_receiver_batch_decrements_by_batch_len() {
        // GaugedReceiver<Receiver<Queue<T>>, Queue<T>> decrements depth by queue.len()
        let (gauge, _reg) = make_gauge();
        let (inner_tx, inner_rx) = iq_sync::new::<u32>();

        // Pre-fill gauge depth to match the batch we will send
        gauge.enqueue(4);

        let mut rx: GaugedReceiver<_, crate::intrusive::Queue<u32>> =
            GaugedReceiver::new(inner_rx, gauge.clone());

        let mut cx = noop_cx();
        let mut budget = Budget::new(usize::MAX);

        let mut batch = crate::intrusive::Queue::new();
        for i in 0..4u32 {
            batch.push_back(crate::intrusive::Entry::new(i));
        }
        inner_tx.send_batch(batch).unwrap();

        // Receiving the batch decrements depth by 4 (the batch_len)
        assert!(matches!(
            rx.poll_recv(&mut cx, &mut budget),
            Poll::Ready(Some(_))
        ));
        assert_eq!(gauge_depth(&gauge), 0);
    }
}
