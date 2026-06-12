// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for completion-side endpoint tasks.

use super::helpers::{entry_channel, test_entry, test_frame, TestReceiverExt as _};
use crate::{
    endpoint::{frame, tasks},
    intrusive::Entry,
    queue::AutoWake,
    socket::channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
    testing::{ext::*, sim},
};
use core::future::poll_fn;

struct CompletionHarness {
    frame_tx: unsync::Sender<crate::intrusive::EntryAdapter<frame::Frame>>,
    wake_rx: unsync::Receiver<crate::intrusive::EntryAdapter<AutoWake>>,
    send_credit_pool: crate::sync::Arc<crate::credit::Pool>,
}

#[derive(Default)]
struct CompletionHarnessBuilder;

impl CompletionHarnessBuilder {
    fn build(self) -> CompletionHarness {
        let (frame_tx, frame_rx) = unsync::new::<frame::Frame>();
        let (wake_tx, wake_rx) = entry_channel::<AutoWake>();
        let registry = crate::counter::Registry::default();
        let clock = crate::time::DefaultClock::default();
        let reader_metrics = std::sync::Arc::new(crate::stream::metrics::ReaderMetrics::new(
            &registry,
            "test.reader",
        ));
        let writer_metrics = std::sync::Arc::new(crate::stream::metrics::WriterMetrics::new(
            &registry,
            "test.writer",
        ));
        let send_credit_pool =
            crate::sync::Arc::new(crate::credit::Pool::new(crate::credit::Config::new(1_000_000)));
        let rx = tasks::completion_dispatcher(
            frame_rx,
            wake_tx,
            clock,
            reader_metrics,
            writer_metrics,
            send_credit_pool.clone(),
        );

        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        CompletionHarness {
            frame_tx,
            wake_rx,
            send_credit_pool,
        }
    }
}

/// A frame with completion wiring is forwarded end-to-end and wakes exactly once.
#[test]
fn completion_dispatcher_forwards_completed_frame_and_wakes() {
    sim(|| {
        let CompletionHarness {
            mut frame_tx,
            mut wake_rx,
            ..
        } = CompletionHarnessBuilder.build();

        async move {
            let pse = test_entry();
            let mut completion_rx = frame::completion_channel();
            let mut submitted = test_frame(&pse).into_inner();
            submitted.status = frame::TransmissionStatus::Acknowledged;
            submitted.completion = Some(completion_rx.sender());

            let expected_ttl = submitted.ttl;
            let expected_header = submitted.header;

            let _ = frame_tx.send(Entry::new(submitted));
            drop(frame_tx);

            let wake = wake_rx.recv().await;
            assert!(wake.is_some(), "completion dispatcher should emit one wake");
            assert!(
                wake_rx.recv().await.is_none(),
                "completion dispatcher should not emit extra wakes"
            );

            let completion = poll_fn(|cx| completion_rx.poll_swap(cx)).await;
            let completion = completion.expect("completion queue should receive submitted frame");
            assert_eq!(completion.len(), 1);
            let completed = completion.front().expect("queue is non-empty");
            assert_eq!(completed.ttl, expected_ttl);
            assert_eq!(completed.header, expected_header);
        }
        .primary()
        .spawn();
    });
}

/// Regression: a frame reaching the completion dispatcher while still holding `flow_credits`
/// (a pending frame drained on invalidation / idle-PeerDead, never admitted to the wire) must
/// release that borrowed credit back to the send pool. Otherwise — since `Frame` has no `Drop`
/// that releases credit — it leaks for the endpoint's lifetime, slowly draining the pool and
/// stalling writers under sustained peer churn.
#[test]
fn completion_dispatcher_releases_flow_credits_on_drained_frame() {
    sim(|| {
        let CompletionHarness {
            mut frame_tx,
            mut wake_rx,
            send_credit_pool,
        } = CompletionHarnessBuilder.build();

        // Borrow credit from the pool, as a writer would, then attach it to a frame that is
        // about to be drained (PeerDead) and routed to the completion dispatcher.
        const CAP: u64 = 1_000_000;
        let credit = {
            let waker = s2n_quic_core::task::waker::noop();
            let mut cx = core::task::Context::from_waker(&waker);
            let slot = Box::leak(Box::new(crate::credit::Slot::new(noop_drop_slot)));
            let slot_ptr = std::ptr::NonNull::from(&*slot);
            match unsafe {
                send_credit_pool.poll_acquire(
                    &mut cx,
                    slot_ptr,
                    CAP,
                    crate::credit::Priority::default(),
                )
            } {
                core::task::Poll::Ready(n) => n,
                core::task::Poll::Pending => panic!("fast-path acquire should succeed"),
            }
        };
        assert!(credit > 0);

        let pool = send_credit_pool.clone();
        async move {
            let pse = test_entry();
            let completion_rx = frame::completion_channel();
            let mut drained = test_frame(&pse).into_inner();
            // PeerDead-style failure status, completion wired, and still holding credit.
            drained.status = frame::TransmissionStatus::Failed(frame::FailureReason::PeerDead);
            drained.completion = Some(completion_rx.sender());
            drained.flow_credits = credit;

            let _ = frame_tx.send(Entry::new(drained));
            drop(frame_tx);

            // Drain the dispatcher (consume its wake) so it processes the frame.
            let _ = wake_rx.recv().await;
            while wake_rx.recv().await.is_some() {}

            assert_eq!(
                pool.debug_available() as u64 + pool.debug_returned(),
                CAP,
                "completion dispatcher leaked {} bytes of flow_credits on a drained frame",
                CAP - (pool.debug_available() as u64 + pool.debug_returned()),
            );
        }
        .primary()
        .spawn();
    });
}

unsafe fn noop_drop_slot(_ptr: std::ptr::NonNull<crate::credit::Slot>) {}

/// Frames without a completion sender do not emit any wake notifications.
#[test]
fn completion_dispatcher_ignores_frames_without_completion_sender() {
    sim(|| {
        let CompletionHarness {
            mut frame_tx,
            mut wake_rx,
            ..
        } = CompletionHarnessBuilder.build();

        async move {
            let pse = test_entry();
            let mut frame = test_frame(&pse).into_inner();
            frame.status = frame::TransmissionStatus::Acknowledged;

            let _ = frame_tx.send(Entry::new(frame));
            drop(frame_tx);

            assert!(
                wake_rx.recv().await.is_none(),
                "frames without completion sender should not emit wakes"
            );
        }
        .primary()
        .spawn();
    });
}

/// `cancelled_drain` yields one output item per input frame and then closes.
#[test]
fn cancelled_drain_consumes_all_frames_then_closes() {
    sim(|| {
        let (mut frame_tx, frame_rx) = unsync::new::<frame::Frame>();
        let (freed_batch_tx, _freed_batch_rx) = crate::queue::freed_batch_channel();
        let registry = crate::counter::Registry::default();
        let clock = crate::time::DefaultClock::default();
        let reader_metrics = std::sync::Arc::new(crate::stream::metrics::ReaderMetrics::new(
            &registry,
            "test.reader",
        ));
        let writer_metrics = std::sync::Arc::new(crate::stream::metrics::WriterMetrics::new(
            &registry,
            "test.writer",
        ));
        let send_credit_pool =
            crate::sync::Arc::new(crate::credit::Pool::new(crate::credit::Config::default()));
        let mut rx = tasks::cancelled_drain(
            frame_rx,
            freed_batch_tx,
            clock,
            reader_metrics,
            writer_metrics,
            send_credit_pool,
        );

        async move {
            let pse = test_entry();
            let _ = frame_tx.send(test_frame(&pse));
            let _ = frame_tx.send(test_frame(&pse));
            drop(frame_tx);
        }
        .spawn();

        async move {
            assert!(
                rx.recv().await.is_some(),
                "first cancelled frame should be consumed"
            );
            assert!(
                rx.recv().await.is_some(),
                "second cancelled frame should be consumed"
            );
            assert!(
                rx.recv().await.is_none(),
                "drain should close after input closes"
            );
        }
        .primary()
        .spawn();
    });
}
