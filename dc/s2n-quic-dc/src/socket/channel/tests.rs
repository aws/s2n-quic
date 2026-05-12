use super::*;
use crate::{intrusive_queue, testing::sim};
use core::{future::Future, pin::pin};

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
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Pending));
}

#[test]
fn cell_sender_drop_closes_receiver() {
    let (tx, mut rx) = cell::unsync::new::<u32>();
    drop(tx);
    let mut cx = noop_cx();
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(None)));
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

    // Empty — pending
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Pending));

    // Send via poll
    {
        let mut fut = pin!(tx.send(42));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }

    // Receive
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(42))));

    // Empty again
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Pending));

    // Send again
    {
        let mut fut = pin!(tx.send(99));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(99))));

    // Drop sender → closed
    drop(tx);
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(None)));
}

#[test]
fn cell_backpressure_poll() {
    let (mut tx, mut rx) = cell::unsync::new::<u32>();
    let mut cx = noop_cx();

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
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(1))));

    // Now send succeeds
    {
        let mut fut = pin!(tx.send(3));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(3))));
}

// ── slot tests ─────────────────────────────────────────────────────

#[test]
fn slot_poll_recv_empty_returns_pending() {
    let (_tx, mut rx) = cell::sync::new::<u32>();
    let mut cx = noop_cx();
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Pending));
}

#[test]
fn slot_sender_drop_closes_receiver() {
    let (tx, mut rx) = cell::sync::new::<u32>();
    drop(tx);
    let mut cx = noop_cx();
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(None)));
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

            tx.send(42).await.unwrap();
            let val = rx.recv().await;
            assert_eq!(val, Some(42));

            tx.send(99).await.unwrap();
            let val = rx.recv().await;
            assert_eq!(val, Some(99));

            drop(tx);
            let val = rx.recv().await;
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

    // Send one value via poll
    {
        let mut fut = pin!(tx.send(1));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }

    assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(1))));
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Pending));

    {
        let mut fut = pin!(tx.send(2));
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(2))));
}

#[test]
fn slot_concurrent_send_recv() {
    sim(|| {
        use crate::testing::ext::*;

        let (mut tx, mut rx) = cell::sync::new::<u32>();

        crate::testing::spawn(async move {
            for i in 0..10 {
                tx.send(i).await.unwrap();
            }
        });

        async move {
            for expected in 0..10 {
                let val = rx.recv().await.unwrap();
                assert_eq!(val, expected);
            }
            let val = rx.recv().await;
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
    assert!(matches!(priority.poll_recv(&mut cx), Poll::Pending));
}

#[test]
fn priority_all_closed_returns_none() {
    let (tx0, rx0) = cell::sync::new::<u32>();
    let (tx1, rx1) = cell::sync::new::<u32>();
    drop(tx0);
    drop(tx1);
    let mut priority = Priority::new(vec![rx0, rx1]);

    let mut cx = noop_cx();
    assert!(matches!(priority.poll_recv(&mut cx), Poll::Ready(None)));
}

#[test]
fn priority_high_wins_over_low() {
    sim(|| {
        use crate::testing::ext::*;

        async {
            let (mut tx0, rx0) = cell::sync::new::<u32>();
            let (mut tx1, rx1) = cell::sync::new::<u32>();
            let mut priority = Priority::new(vec![rx0, rx1]);

            tx0.send(10).await.unwrap();
            tx1.send(20).await.unwrap();

            let val = priority.recv().await;
            assert_eq!(val, Some(10));

            let val = priority.recv().await;
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

            tx1.send(99).await.unwrap();

            let val = priority.recv().await;
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

            drop(tx0);

            tx1.send(42).await.unwrap();
            let val = priority.recv().await;
            assert_eq!(val, Some(42));

            drop(tx1);
            let val = priority.recv().await;
            assert_eq!(val, None);
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
            let (mut tx, rx) = cell::sync::new::<intrusive_queue::Queue<u32>>();
            let mut flat = Flatten::new(rx);

            let mut queue = intrusive_queue::Queue::default();
            queue.push_back(intrusive_queue::Entry::new(10));
            queue.push_back(intrusive_queue::Entry::new(20));
            queue.push_back(intrusive_queue::Entry::new(30));

            assert!(tx.send(queue).await.is_ok());

            assert_eq!(*flat.recv().await.unwrap(), 10);
            assert_eq!(*flat.recv().await.unwrap(), 20);
            assert_eq!(*flat.recv().await.unwrap(), 30);

            let mut queue2 = intrusive_queue::Queue::default();
            queue2.push_back(intrusive_queue::Entry::new(40));
            assert!(tx.send(queue2).await.is_ok());

            assert_eq!(*flat.recv().await.unwrap(), 40);

            drop(tx);
            let val = flat.recv().await;
            assert!(val.is_none());
        }
        .primary()
        .spawn();
    });
}

// ── YieldAfter tests ────────────────────────────────────────────────

struct AssertSend<F>(F);
unsafe impl<F> Send for AssertSend<F> {}
impl<F: core::future::Future> core::future::Future for AssertSend<F> {
    type Output = F::Output;
    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        unsafe { self.map_unchecked_mut(|s| &mut s.0) }.poll(cx)
    }
}

#[test]
fn yield_after_forces_yield_at_threshold() {
    sim(|| {
        use crate::testing::ext::*;

        AssertSend(async {
            let (mut tx, rx) = cell::sync::new::<u32>();
            let mut yield_rx = super::YieldAfter::new(rx, 3);

            let mut cx = noop_cx();

            // First 3 should return Ready
            for i in 0..3 {
                tx.send(i).await.unwrap();
                assert!(matches!(yield_rx.poll_recv(&mut cx), Poll::Ready(Some(_))));
            }

            // 4th should force a yield (return Pending)
            tx.send(3).await.unwrap();
            assert!(matches!(yield_rx.poll_recv(&mut cx), Poll::Pending));

            // After the forced yield, should return Ready again
            assert!(matches!(yield_rx.poll_recv(&mut cx), Poll::Ready(Some(3))));
        })
        .primary()
        .spawn();
    });
}

#[test]
fn yield_after_resets_on_natural_pending() {
    sim(|| {
        use crate::testing::ext::*;

        AssertSend(async {
            let (mut tx, rx) = cell::sync::new::<u32>();
            let mut yield_rx = super::YieldAfter::new(rx, 5);
            let mut cx = noop_cx();

            // Send and receive 2 values
            tx.send(1).await.unwrap();
            assert!(matches!(yield_rx.poll_recv(&mut cx), Poll::Ready(Some(1))));

            tx.send(2).await.unwrap();
            assert!(matches!(yield_rx.poll_recv(&mut cx), Poll::Ready(Some(2))));

            // Now channel is empty, should return Pending naturally
            assert!(matches!(yield_rx.poll_recv(&mut cx), Poll::Pending));

            // Counter should have reset, so we can get 5 more Ready results
            for i in 3..=7 {
                tx.send(i).await.unwrap();
                assert!(matches!(yield_rx.poll_recv(&mut cx), Poll::Ready(Some(_))));
            }

            // Now should force a yield
            tx.send(8).await.unwrap();
            assert!(matches!(yield_rx.poll_recv(&mut cx), Poll::Pending));
        })
        .primary()
        .spawn();
    });
}

#[test]
fn flatten_empty_queue_skipped() {
    sim(|| {
        use crate::testing::ext::*;

        let (mut tx, rx) = cell::sync::new::<intrusive_queue::Queue<u32>>();

        // Sender task: send empty queue, then non-empty queue
        crate::testing::spawn(async move {
            let empty_queue = intrusive_queue::Queue::default();
            assert!(tx.send(empty_queue).await.is_ok());

            let mut queue = intrusive_queue::Queue::default();
            queue.push_back(intrusive_queue::Entry::new(42));
            assert!(tx.send(queue).await.is_ok());
        });

        // Receiver task: should skip the empty queue and return 42
        async move {
            let mut flat = Flatten::new(rx);
            assert_eq!(*flat.recv().await.unwrap(), 42);
        }
        .primary()
        .spawn();
    });
}

// ── Paced tests ────────────────────────────────────────────────────

#[test]
fn paced_limits_rate() {
    sim(|| {
        use crate::{clock::precision::Clock as _, testing::ext::*};

        async {
            let (mut tx, rx) = cell::sync::new::<u32>();
            let clock = crate::clock::bach::Clock::default();
            // 1 Gbps = 8 nanos per byte
            let rate = crate::socket::rate::Rate::new(1.0);
            let mut paced_rx = super::Paced::new(rx, clock.clone(), rate);

            let start = clock.now();

            // Send 100KB total - exceeds burst capacity of 64KB
            for _ in 0..100 {
                tx.send(1000).await.unwrap();
                let val = paced_rx.recv().await;
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
        use crate::{clock::precision::Clock as _, testing::ext::*};

        async {
            let (mut tx, rx) = cell::sync::new::<u32>();
            let clock = crate::clock::bach::Clock::default();
            let rate = crate::socket::rate::Rate::new(1.0);
            let mut paced_rx = super::Paced::new(rx, clock.clone(), rate);

            // Receive first packet but DON'T call on_consumed (simulating cancellation)
            tx.send(1000).await.unwrap();
            let val = paced_rx.recv().await;
            assert_eq!(val, Some(1000));
            // Intentionally not calling on_consumed

            // Next packet should not be paced since we didn't consume the first one
            tx.send(1000).await.unwrap();
            let start = clock.now();
            let val = paced_rx.recv().await;
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
        use crate::{clock::precision::Clock as _, testing::ext::*};

        async {
            let (mut tx, rx) = cell::sync::new::<usize>();
            let clock = crate::clock::bach::Clock::default();
            // 5 Gbps
            let rate = crate::socket::rate::Rate::new(5.0);
            let paced_rx = super::Paced::new(rx, clock.clone(), rate);
            let mut paced_rx = super::Reporter::new(paced_rx, clock.clone(), false);

            // Producer task: send 64KB packets continuously for 5 seconds worth of data
            // At 5Gbps, that's 5 * 5 / 8 = ~3GB = ~47,000 packets of 64KB
            crate::testing::spawn(async move {
                for _ in 0..47000 {
                    tx.send(65536).await.unwrap();
                }
            });

            // Consumer task: receive and discard
            let start = clock.now();
            let mut count = 0;
            let mut total_bytes = 0u64;
            while let Some(bytes) = paced_rx.recv().await {
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
            tracing::info!(
                "Received {} packets ({} GB) in {:.2}s = {:.2} Gbps",
                count,
                total_bytes / 1_000_000_000,
                elapsed_sec,
                gbps
            );

            // Should take around 5 seconds due to pacing
            assert!(
                elapsed_sec >= 4.5 && elapsed_sec <= 6.0,
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
        use crate::{clock::precision::Clock as _, testing::ext::*};

        async {
            let (mut tx, rx) = cell::sync::new::<u32>();
            let clock = crate::clock::bach::Clock::default();
            let rate = crate::socket::rate::Rate::new(1.0);
            let mut paced_rx = super::Paced::new(rx, clock.clone(), rate);

            let start = clock.now();

            // Send 64KB (burst capacity) - should go through without pacing
            for _ in 0..64 {
                tx.send(1000).await.unwrap();
                let val = paced_rx.recv().await;
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
    use crate::socket::channel::intrusive_queue::sync as iq_sync;

    #[test]
    fn batch_len_single_entry() {
        let e = crate::intrusive_queue::Entry::new(42u32);
        assert_eq!(e.batch_len(), 1);
    }

    #[test]
    fn batch_len_queue() {
        let mut q = crate::intrusive_queue::Queue::<u32>::new();
        assert_eq!(q.batch_len(), 0);
        q.push_back(crate::intrusive_queue::Entry::new(1u32));
        q.push_back(crate::intrusive_queue::Entry::new(2u32));
        q.push_back(crate::intrusive_queue::Entry::new(3u32));
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

        let mut tx: GaugedSender<_, crate::intrusive_queue::Entry<u32>> =
            GaugedSender::new(inner_tx, gauge.clone());

        assert_eq!(gauge_depth(&gauge), 0);
        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive_queue::Entry::new(10u32),
        )
        .unwrap();
        assert_eq!(gauge_depth(&gauge), 1);
        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive_queue::Entry::new(20u32),
        )
        .unwrap();
        assert_eq!(gauge_depth(&gauge), 2);
    }

    #[test]
    fn gauged_sender_unbounded_does_not_increment_on_closed_channel() {
        let (gauge, _reg) = make_gauge();
        let (inner_tx, rx) = iq_sync::new::<u32>();
        drop(rx);

        let mut tx: GaugedSender<_, crate::intrusive_queue::Entry<u32>> =
            GaugedSender::new(inner_tx, gauge.clone());

        let result = <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive_queue::Entry::new(42u32),
        );
        assert!(result.is_err(), "send should fail on closed channel");
        assert_eq!(gauge_depth(&gauge), 0, "depth must not increase on failure");
    }

    #[test]
    fn gauged_sender_batch_counts_queue_len() {
        // Sending a Queue<T> with 3 items should increment depth by 3
        let (gauge, _reg) = make_gauge();
        let (inner_tx, _rx) = iq_sync::new::<u32>();

        let mut tx: GaugedSender<_, crate::intrusive_queue::Queue<u32>> =
            GaugedSender::new(inner_tx, gauge.clone());

        assert_eq!(gauge_depth(&gauge), 0);

        let mut batch = crate::intrusive_queue::Queue::new();
        batch.push_back(crate::intrusive_queue::Entry::new(1u32));
        batch.push_back(crate::intrusive_queue::Entry::new(2u32));
        batch.push_back(crate::intrusive_queue::Entry::new(3u32));
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
            .send_entry(crate::intrusive_queue::Entry::new(1u32))
            .unwrap();
        inner_tx
            .send_entry(crate::intrusive_queue::Entry::new(2u32))
            .unwrap();

        let mut rx: GaugedReceiver<_, crate::intrusive_queue::Entry<u32>> =
            GaugedReceiver::new(inner_rx, gauge.clone());

        let mut cx = noop_cx();

        assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(_))));
        assert_eq!(gauge_depth(&gauge), 1);

        assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(_))));
        assert_eq!(gauge_depth(&gauge), 0);
    }

    #[test]
    fn gauged_sender_and_receiver_paired_track_depth() {
        let (gauge, _reg) = make_gauge();
        let (inner_tx, inner_rx) = iq_sync::new::<u32>();

        let mut tx: GaugedSender<_, crate::intrusive_queue::Entry<u32>> =
            GaugedSender::new(inner_tx, gauge.clone());
        let mut rx: GaugedReceiver<_, crate::intrusive_queue::Entry<u32>> =
            GaugedReceiver::new(inner_rx, gauge.clone());

        let mut cx = noop_cx();

        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive_queue::Entry::new(1u32),
        )
        .unwrap();
        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive_queue::Entry::new(2u32),
        )
        .unwrap();
        assert_eq!(gauge_depth(&gauge), 2);

        assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(_))));
        assert_eq!(gauge_depth(&gauge), 1);

        <GaugedSender<_, _> as UnboundedSender<_>>::send(
            &mut tx,
            crate::intrusive_queue::Entry::new(3u32),
        )
        .unwrap();
        assert_eq!(gauge_depth(&gauge), 2);

        assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(_))));
        assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(_))));
        assert_eq!(gauge_depth(&gauge), 0);
    }

    #[test]
    fn gauged_receiver_batch_decrements_by_batch_len() {
        // GaugedReceiver<Receiver<Queue<T>>, Queue<T>> decrements depth by queue.len()
        let (gauge, _reg) = make_gauge();
        let (inner_tx, inner_rx) = iq_sync::new::<u32>();

        // Pre-fill gauge depth to match the batch we will send
        gauge.enqueue(4);

        let mut rx: GaugedReceiver<_, crate::intrusive_queue::Queue<u32>> =
            GaugedReceiver::new(inner_rx, gauge.clone());

        let mut cx = noop_cx();

        let mut batch = crate::intrusive_queue::Queue::new();
        for i in 0..4u32 {
            batch.push_back(crate::intrusive_queue::Entry::new(i));
        }
        inner_tx.send_batch(batch).unwrap();

        // Receiving the batch decrements depth by 4 (the batch_len)
        assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(Some(_))));
        assert_eq!(gauge_depth(&gauge), 0);
    }
}
