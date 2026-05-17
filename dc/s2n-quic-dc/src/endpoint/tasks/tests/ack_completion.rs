// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for the `ack_completion` task.
//!
//! After the send worker assembles and transmits an ACK packet, the PendingAck entry is
//! returned to the recv dispatch worker via the ack_completion channel. This task looks up
//! the recv context and decides whether to re-submit (if new ack-eliciting packets arrived
//! while the ACK was in flight — "stale") or transition back to idle. These tests verify
//! the three outcomes: clean completion, stale re-submission, and graceful handling of
//! evicted contexts.

use super::helpers::{RecvContextBuilder, TestReceiver, TestReceiverExt as _};
use crate::{
    endpoint::{ack::state as ack_state, msg, recv, tasks},
    socket::channel::{intrusive::unsync, ReceiverExt as _},
    testing::{ext::*, sim},
    time::bach::Clock,
};
use s2n_quic_core::{time::Clock as _, varint::VarInt};
use std::{cell::RefCell, rc::Rc};

struct Harness {
    output_rx: crate::socket::channel::intrusive::unsync::Receiver<
        crate::intrusive::EntryAdapter<msg::Sender>,
    >,
}

/// Creates a recv context in Flushed state (ACK in-flight) and returns both the
/// context and the submission that was sent.
fn setup_flushed_context() -> (Rc<RefCell<recv::Context>>, ack_state::Submission) {
    let ctx = RecvContextBuilder::default().build();
    let submission = {
        let mut c = ctx.borrow_mut();
        let clock = Clock::default();
        let now = clock.get_time();
        c.ack_ranges.on_packet_received(VarInt::from_u8(1), now);
        c.ack_state.on_ack_eliciting().unwrap();
        c.encode_and_flush(0).expect("should produce submission")
    };
    (ctx, submission)
}

/// Spawns the ack_completion task with the given cache and completion entries.
fn setup(
    cache: Rc<RefCell<recv::Cache>>,
    entries: impl IntoIterator<Item = crate::intrusive::Entry<msg::Sender>>,
) -> Harness {
    let (sender, output_rx) = unsync::new::<msg::Sender>();
    let input = TestReceiver::new(entries);
    let rx = tasks::ack_completion(input, cache, sender);
    async move { rx.drain_budgeted(Some(32)).await }
        .primary()
        .spawn();
    Harness { output_rx }
}

fn cache_with_context(
    ctx: Rc<RefCell<recv::Context>>,
    submission: &ack_state::Submission,
) -> Rc<RefCell<recv::Cache>> {
    let cache = Rc::new(RefCell::new(recv::Cache::new(0)));
    let key = recv::Key {
        id: *submission.path_secret_entry.id(),
        remote_sender_id: submission.remote_sender_id,
    };
    cache.borrow_mut().senders.insert(key, ctx);
    cache
}

/// When no new packets arrived while the ACK was in flight (Flushed → Idle),
/// the task produces no re-submission. The recv context returns to idle.
#[test]
fn non_stale_completion_does_not_resubmit() {
    sim(|| {
        let (ctx, submission) = setup_flushed_context();
        let cache = cache_with_context(ctx, &submission);
        let entry = crate::intrusive::Entry::new(msg::Sender::PendingAck(submission));

        let Harness { mut output_rx } = setup(cache, [entry]);

        async move {
            assert!(
                output_rx.recv().await.is_none(),
                "non-stale completion should not re-submit"
            );
        }
        .primary()
        .spawn();
    });
}

/// When new ack-eliciting packets arrived while the ACK was in the send pipeline
/// (FlushedStale → Scheduled), the task re-encodes and re-submits a fresh PendingAck.
/// This ensures the peer eventually receives acknowledgment for all received packets.
#[test]
fn stale_completion_resubmits() {
    sim(|| {
        let (ctx, submission) = setup_flushed_context();

        // Simulate new packets arriving while the ACK was in flight
        {
            let mut c = ctx.borrow_mut();
            let clock = Clock::default();
            let now = clock.get_time();
            c.ack_ranges.on_packet_received(VarInt::from_u8(2), now);
            c.ack_state.on_ack_eliciting().unwrap();
        }

        let cache = cache_with_context(ctx, &submission);
        let entry = crate::intrusive::Entry::new(msg::Sender::PendingAck(submission));

        let Harness { mut output_rx } = setup(cache, [entry]);

        async move {
            let first = output_rx
                .recv()
                .await
                .expect("stale completion should re-submit pending ack");
            assert!(matches!(&*first, msg::Sender::PendingAck(_)));
            assert!(
                output_rx.recv().await.is_none(),
                "stale completion should only re-submit once"
            );
        }
        .primary()
        .spawn();
    });
}

/// When the recv context has been evicted from the cache (e.g. idle timeout expired),
/// the completion is silently dropped. No panic, no re-submission.
#[test]
fn unknown_context_silently_dropped() {
    sim(|| {
        let (_ctx, submission) = setup_flushed_context();

        // Empty cache — context won't be found
        let cache = Rc::new(RefCell::new(recv::Cache::new(0)));
        let entry = crate::intrusive::Entry::new(msg::Sender::PendingAck(submission));

        let Harness { mut output_rx } = setup(cache, [entry]);

        async move {
            assert!(
                output_rx.recv().await.is_none(),
                "unknown context should drop completion"
            );
        }
        .primary()
        .spawn();
    });
}
