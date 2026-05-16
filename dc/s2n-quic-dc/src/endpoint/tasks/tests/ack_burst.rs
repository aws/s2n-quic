// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for the `ack_burst` task.
//!
//! The ack burst task sits on the recv dispatch worker. When the packet dispatch task
//! schedules an ACK (by pushing a recv::Context into the burst queue), this task encodes
//! the ACK ranges and emits a PendingAck submission to the send worker. These tests verify
//! the encoding/emission contract: contexts with pending ACKs produce submissions, contexts
//! without pending ACKs produce nothing, and already-flushed contexts are not double-submitted.

use super::helpers::{CollectingSender, RecvContextBuilder, TestReceiver};
use crate::{
    socket::channel::ReceiverExt as _,
    stream::endpoint::{msg, tasks},
    testing::ext::*,
    time::bach::Clock,
};
use s2n_quic_core::{time::Clock as _, varint::VarInt};

/// A context with ack_state=Scheduled and recorded ACK ranges produces a PendingAck submission.
#[test]
fn context_with_pending_acks_emits_submission() {
    crate::testing::sim(|| {
        let ctx = RecvContextBuilder::default().build();

        {
            let mut c = ctx.borrow_mut();
            let clock = Clock::default();
            let now = clock.get_time();
            c.ack_ranges.on_packet_received(VarInt::from_u8(1), now);
            c.ack_state.on_ack_eliciting().unwrap();
        }

        let (sender, collected) = CollectingSender::new();
        let input = TestReceiver::new([ctx]);
        let rx = tasks::ack_burst(input, sender, 0);
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            1.ms().sleep().await;
            let items = collected.borrow();
            assert_eq!(items.len(), 1);
            assert!(matches!(&*items[0], msg::Sender::PendingAck(_)));
        }
        .primary()
        .spawn();
    });
}

/// A context in Idle state (no ack-eliciting packets received) produces no output.
#[test]
fn context_with_no_pending_acks_emits_nothing() {
    crate::testing::sim(|| {
        let ctx = RecvContextBuilder::default().build();

        let (sender, collected) = CollectingSender::new();
        let input = TestReceiver::new([ctx]);
        let rx = tasks::ack_burst(input, sender, 0);
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            1.ms().sleep().await;
            assert!(collected.borrow().is_empty());
        }
        .primary()
        .spawn();
    });
}

/// Each context in the burst queue is processed independently — N scheduled contexts
/// produce N submissions.
#[test]
fn multiple_contexts_each_produce_submission() {
    crate::testing::sim(|| {
        let clock = Clock::default();
        let now = clock.get_time();

        let contexts: Vec<_> = (0..3)
            .map(|i| {
                let ctx = RecvContextBuilder::default()
                    .remote_sender_id(VarInt::new(i).unwrap())
                    .build();
                {
                    let mut c = ctx.borrow_mut();
                    c.ack_ranges.on_packet_received(VarInt::from_u8(1), now);
                    c.ack_state.on_ack_eliciting().unwrap();
                }
                ctx
            })
            .collect();

        let (sender, collected) = CollectingSender::new();
        let input = TestReceiver::new(contexts);
        let rx = tasks::ack_burst(input, sender, 0);
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            1.ms().sleep().await;
            assert_eq!(collected.borrow().len(), 3);
        }
        .primary()
        .spawn();
    });
}

/// A context already in Flushed state (ACK submission already in the send pipeline)
/// does not produce a second submission. The at-most-one-in-flight invariant is preserved.
#[test]
fn flushed_context_does_not_double_submit() {
    crate::testing::sim(|| {
        let ctx = RecvContextBuilder::default().build();

        {
            let mut c = ctx.borrow_mut();
            let clock = Clock::default();
            let now = clock.get_time();
            c.ack_ranges.on_packet_received(VarInt::from_u8(1), now);
            c.ack_state.on_ack_eliciting().unwrap();
            c.ack_state.on_flush().unwrap();
        }

        let (sender, collected) = CollectingSender::new();
        let input = TestReceiver::new([ctx]);
        let rx = tasks::ack_burst(input, sender, 0);
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            1.ms().sleep().await;
            assert!(collected.borrow().is_empty());
        }
        .primary()
        .spawn();
    });
}
