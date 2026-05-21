// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for completion-side endpoint tasks.

use super::helpers::{entry_channel, test_entry, test_frame, TestReceiverExt as _};
use crate::{
    endpoint::{frame, tasks},
    flow::queue::AutoWake,
    intrusive::Entry,
    socket::channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
    testing::{ext::*, sim},
};
use core::future::poll_fn;
use s2n_quic_core::varint::VarInt;

struct CompletionHarness {
    frame_tx: unsync::Sender<crate::intrusive::EntryAdapter<frame::Frame>>,
    wake_rx: unsync::Receiver<crate::intrusive::EntryAdapter<AutoWake>>,
}

#[derive(Default)]
struct CompletionHarnessBuilder;

impl CompletionHarnessBuilder {
    fn build(self) -> CompletionHarness {
        let (frame_tx, frame_rx) = unsync::new::<frame::Frame>();
        let (wake_tx, wake_rx) = entry_channel::<AutoWake>();
        let rx = tasks::completion_dispatcher(frame_rx, wake_tx);

        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        CompletionHarness { frame_tx, wake_rx }
    }
}

/// A frame with completion wiring is forwarded end-to-end and wakes exactly once.
#[test]
fn completion_dispatcher_forwards_completed_frame_and_wakes() {
    sim(|| {
        let CompletionHarness {
            mut frame_tx,
            mut wake_rx,
        } = CompletionHarnessBuilder.build();

        async move {
            let pse = test_entry();
            let mut completion_rx = frame::completion_channel();
            let mut submitted = test_frame(&pse).into_inner();
            submitted.status = frame::TransmissionStatus::Acknowledged;
            submitted.source_sender_id = crate::endpoint::id::LocalSenderId::new(VarInt::from_u8(7));
            submitted.completion = Some(completion_rx.sender());

            let expected_source_sender_id = submitted.source_sender_id;
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
            assert_eq!(completed.source_sender_id, expected_source_sender_id);
            assert_eq!(completed.ttl, expected_ttl);
            assert_eq!(completed.header, expected_header);
        }
        .primary()
        .spawn();
    });
}

/// Frames without a completion sender do not emit any wake notifications.
#[test]
fn completion_dispatcher_ignores_frames_without_completion_sender() {
    sim(|| {
        let CompletionHarness {
            mut frame_tx,
            mut wake_rx,
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
        let mut rx = tasks::cancelled_drain(frame_rx);

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
