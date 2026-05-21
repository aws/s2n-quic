// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Regression test for stale TX wheel scheduling after ACK processing.
//!
//! When a send context is linked in the TX wheel (waiting for the wheel to pop it) and an
//! ACK arrives that removes the reason for scheduling (e.g., clears probe_state via
//! on_all_acked), the invariant "tx wheel scheduled without any sendable work" fires.
//! This is benign because the wheel will pop the context and the assembler will find nothing
//! to do (a no-op wakeup), but the debug invariant was too strict.

use super::helpers::{build_send_context, test_entry};
use crate::{
    endpoint::{
        counters,
        frame::{self, Frame, Header},
        id::Id,
        inflight::{Packet, TransmissionInfo},
        send,
    },
    intrusive::{Entry, Queue},
    packet::datagram::QueuePair,
    socket::channel::{intrusive::unsync, UnboundedSender as _},
    testing::sim,
    time::bach::Clock,
    xorshift::Rng,
};
use bytes::BytesMut;
use core::time::Duration;
use s2n_codec::EncoderValue as _;
use s2n_quic_core::{
    ack, frame as quic_frame, packet::number::PacketNumberSpace, time::Clock as _, varint::VarInt,
};

/// Encode a QUIC ACK frame acknowledging a single contiguous range [0, largest].
fn encode_ack_payload(largest: u64) -> BytesMut {
    let mut ranges = ack::Ranges::new(64);
    for pn in 0..=largest {
        let packet_number = PacketNumberSpace::Initial.new_packet_number(VarInt::new(pn).unwrap());
        ranges.insert_packet_number(packet_number).unwrap();
    }
    let frame = quic_frame::Ack {
        ack_delay: VarInt::ZERO,
        ack_ranges: &ranges,
        ecn_counts: None,
    };
    BytesMut::from(frame.encode_to_vec().as_slice())
}

/// Create a test frame suitable for inflight insertion (ack-eliciting).
fn inflight_frame(pse: &std::sync::Arc<crate::path::secret::map::Entry>) -> Entry<Frame> {
    Entry::new(Frame {
        header: Header::FlowData {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            stream_id: VarInt::from_u8(1),
            offset: VarInt::ZERO,
            is_fin: false,
        },
        source_sender_id: crate::endpoint::id::LocalSenderId::new(VarInt::MAX),
        payload: bytes::BytesMut::zeroed(100).into(),
        path_secret_entry: pse.clone(),
        completion: None,
        status: frame::TransmissionStatus::Pending,
        ttl: 3,
        transmission_time: None,
    })
}

/// When a context is scheduled in the TX wheel due to a pending probe, and an ACK arrives
/// that acknowledges all inflight packets (clearing probe_state via on_all_acked), the
/// invariant should not panic. The stale wheel entry is harmless — the assembler will
/// find nothing to do when it eventually pops.
#[test]
fn stale_tx_wheel_after_ack_clears_probe() {
    sim(|| {
        let clock = Clock::default();
        let registry = crate::counter::Registry::default();
        let entry = test_entry();
        let ctx_rc = build_send_context(&entry, 0, &registry, &clock);

        // Insert a packet into inflight (simulating a transmitted packet at PN 0)
        let now = clock.get_time();
        {
            let mut ctx = ctx_rc.borrow_mut();
            let rtt = ctx.rtt_estimator.clone();
            let cc_info = ctx.cca.on_packet_sent(now, 200, false, &rtt);
            let mut frames = Queue::new();
            frames.push_back(inflight_frame(&entry));
            let pn = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
            ctx.inflight.insert(
                pn,
                Packet::new(
                    frames,
                    TransmissionInfo {
                        cc_info,
                        time_sent: now,
                        sent_bytes: 200,
                    },
                ),
            );
            ctx.next_packet_number = VarInt::from_u8(1);

            // Simulate a PTO firing: transition probe_state to Requested
            let _ = ctx.pto.probe_state.request();
        }

        // Schedule the context in the TX wheel (making tx_wheel.is_scheduled() = true).
        // This simulates what the WheelRouter does when wheel_interest.transmission = true.
        let (mut tx_wheel_tx, _tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        {
            let mut ctx = ctx_rc.borrow_mut();
            ctx.tx_wheel.target_time = None; // immediate scheduling for probes
        }
        let _ = tx_wheel_tx.send(ctx_rc.clone());

        // Verify precondition: context is now scheduled
        assert!(
            ctx_rc.borrow().tx_wheel.is_scheduled(),
            "context must be linked in tx wheel"
        );

        // Now simulate an ACK arriving that acknowledges PN 0 (all inflight data).
        // This triggers on_ack_received(false) → on_all_acked() → probe_state = Idle.
        // The invariant fires because:
        //   - is_scheduled() = true (still linked in wheel)
        //   - has_pending_acks() = false
        //   - probe_state.is_requested() = false (cleared by on_all_acked)
        //   - has_pending_data() = false (no pending frames)
        let send_counters =
            counters::Send::new(&registry, crate::endpoint::id::LocalSenderId::from_index(0));
        let mut completed = Queue::new();
        let mut lost = Queue::new();
        let mut cancelled = Queue::new();
        let mut rng = Rng::new();
        let mut payload = encode_ack_payload(0);

        let mut deferred = Vec::new();
        let _ = ctx_rc.borrow_mut().process_ack_payload(
            &mut payload,
            Duration::ZERO,
            &send_counters,
            &mut completed,
            &mut lost,
            &mut cancelled,
            &clock,
            &mut rng,
            &mut deferred,
        );
    });
}
