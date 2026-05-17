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

use super::helpers::{CollectingSender, RecvContextBuilder, TestReceiver};
use crate::{
    endpoint::{ack::state as ack_state, msg, recv, tasks},
    intrusive::Entry,
    socket::channel::ReceiverExt as _,
    testing::{ext::*, sim},
    time::bach::Clock,
};
use s2n_quic_core::{time::Clock as _, varint::VarInt};
use std::{cell::RefCell, rc::Rc};

struct Harness {
    collected: Rc<RefCell<Vec<Entry<msg::Sender>>>>,
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
    entries: impl IntoIterator<Item = Entry<msg::Sender>>,
) -> Harness {
    let (sender, collected) = CollectingSender::new();
    let input = TestReceiver::new(entries);
    let rx = tasks::ack_completion(input, cache, sender);
    async move { rx.drain_budgeted(Some(32)).await }
        .primary()
        .spawn();
    Harness { collected }
}

fn cache_with_context(
    ctx: Rc<RefCell<recv::Context>>,
    submission: &ack_state::Submission,
) -> Rc<RefCell<recv::Cache>> {
    let cache = Rc::new(RefCell::new(recv::Cache::new(
        std::time::Duration::from_secs(30),
        0,
    )));
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
        let entry = Entry::new(msg::Sender::PendingAck(submission));

        let harness = setup(cache, [entry]);

        async move {
            1.ms().sleep().await;
            assert!(harness.collected.borrow().is_empty());
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
        let entry = Entry::new(msg::Sender::PendingAck(submission));

        let harness = setup(cache, [entry]);

        async move {
            1.ms().sleep().await;
            let items = harness.collected.borrow();
            assert_eq!(items.len(), 1);
            assert!(matches!(&*items[0], msg::Sender::PendingAck(_)));
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
        let cache = Rc::new(RefCell::new(recv::Cache::new(
            std::time::Duration::from_secs(30),
            0,
        )));
        let entry = Entry::new(msg::Sender::PendingAck(submission));

        let harness = setup(cache, [entry]);

        async move {
            1.ms().sleep().await;
            assert!(harness.collected.borrow().is_empty());
        }
        .primary()
        .spawn();
    });
}
