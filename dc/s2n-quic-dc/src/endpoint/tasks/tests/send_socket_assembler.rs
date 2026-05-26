// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for the `send_socket_assembler` pipeline function.
//!
//! The socket assembler pipeline wires together the per-socket `Assembler` combinator
//! (which seals frames into UDP datagrams), the `WheelRouter` (which re-schedules
//! the context into tx/PTO/idle wheels after assembly), pacing, and the socket sender.
//! These tests verify the routing behavior of every output channel under different
//! pending-data and frame-cancellation scenarios.
use super::helpers::{
    build_send_context, test_batch, test_batch_with_payload, test_entry_at, TestReceiverExt as _,
};
use crate::{
    endpoint::{combinator::AssemblerCounters, frame, id::Id, msg, send, tasks},
    socket::{
        channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
        pool::Pool,
        rate::Rate,
    },
    testing::{ext::*, sim},
    time::{bach::Clock, precision::Clock as _},
    tracing::*,
};
use bach::net::UdpSocket;
use core::time::Duration;
use s2n_quic_core::varint::VarInt;
use s2n_quic_platform::features::Gso;

// ── helpers ───────────────────────────────────────────────────────────────────

/// All channels required to wire up one `send_socket_assembler` pipeline.
///
/// Call [`assembler_channels`] to create the whole set at once, then fully
/// destructure the struct to distribute each field to the appropriate task:
///
/// - `ctx_tx` — feeder task (sends contexts into the pipeline)
/// - `ctx_rx`, `cancelled_tx`, `ack_completions_tx`, `asm_counters`,
///   `tx_wheel_tx`, `pto_wheel_tx`, `idle_wheel_tx` — pipeline task
/// - `tx_wheel_rx`, `pto_wheel_rx`, `idle_wheel_rx`, `cancelled_rx`,
///   `ack_completions_rx` — assertion task
struct AssemblerChannels {
    /// Send contexts into the pipeline (feeder task).
    ctx_tx: unsync::Sender<send::TxWheelAdapter>,
    /// Receive contexts out of the pipeline input (consumed by the assembler).
    ctx_rx: unsync::Receiver<send::TxWheelAdapter>,
    /// Immediate-tx input receiver for the assembler (priority path).
    immediate_rx: unsync::Receiver<send::TxImmediateAdapter>,
    /// Immediate-tx sender (routed by sender_idx).
    immediate_tx: send::ImmediateSender<unsync::Sender<send::TxImmediateAdapter>>,
    /// Cancelled-frame sink passed to the assembler.
    cancelled_tx: unsync::ListSender<crate::intrusive::EntryAdapter<frame::Frame>>,
    /// ACK-completion sink passed to the assembler.
    ack_completions_tx: unsync::ListSender<crate::intrusive::EntryAdapter<msg::Sender>>,
    /// Assembler metrics counters.
    asm_counters: AssemblerCounters,
    /// TX-wheel re-arm sender passed to the assembler.
    tx_wheel_tx: unsync::Sender<send::TxWheelAdapter>,
    /// PTO-wheel re-arm sender passed to the assembler.
    pto_wheel_tx: unsync::Sender<send::PtoWheelAdapter>,
    /// Idle-wheel re-arm sender passed to the assembler.
    idle_wheel_tx: unsync::Sender<send::IdleWheelAdapter>,
    /// Assert that the context was (or was not) re-armed on the TX wheel.
    tx_wheel_rx: unsync::Receiver<send::TxWheelAdapter>,
    /// Assert that the context was (or was not) re-armed on the PTO wheel.
    pto_wheel_rx: unsync::Receiver<send::PtoWheelAdapter>,
    /// Assert that the context was (or was not) re-armed on the idle wheel.
    idle_wheel_rx: unsync::Receiver<send::IdleWheelAdapter>,
    /// Assert on frames routed to the cancelled output.
    cancelled_rx: unsync::Receiver<crate::intrusive::EntryAdapter<frame::Frame>>,
    /// Assert on ACK-completion notifications.
    ack_completions_rx: unsync::Receiver<crate::intrusive::EntryAdapter<msg::Sender>>,
}

fn assembler_channels(registry: &crate::counter::Registry) -> AssemblerChannels {
    use crate::endpoint::id::{Id, IdMap, LocalSendSocketId, LocalSenderId};
    let (ctx_tx, ctx_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
    let (immediate_tx_raw, immediate_rx) = unsync::new_with_adapter::<send::TxImmediateAdapter>();
    let (tx_wheel_tx, tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
    let (pto_wheel_tx, pto_wheel_rx) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
    let (idle_wheel_tx, idle_wheel_rx) = unsync::new_with_adapter::<send::IdleWheelAdapter>();
    let (cancelled_tx, cancelled_rx) = unsync::new::<frame::Frame>();
    let (ack_completions_tx, ack_completions_rx) = unsync::new::<msg::Sender>();
    let asm_counters = AssemblerCounters::new(registry);

    let socket_immediate_txs: IdMap<LocalSendSocketId, _> =
        core::iter::once((LocalSendSocketId::new(0), immediate_tx_raw)).collect();
    let mut sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId> =
        IdMap::new(1, LocalSendSocketId::new(0));
    sender_idx_to_local[LocalSenderId::from_index(0)] = LocalSendSocketId::new(0);
    let immediate_tx = send::ImmediateSender::new(socket_immediate_txs, sender_idx_to_local);

    AssemblerChannels {
        ctx_tx,
        ctx_rx,
        immediate_rx,
        immediate_tx,
        cancelled_tx: cancelled_tx.into_list_sender(),
        ack_completions_tx: ack_completions_tx.into_list_sender(),
        asm_counters,
        tx_wheel_tx,
        pto_wheel_tx,
        idle_wheel_tx,
        tx_wheel_rx,
        pto_wheel_rx,
        idle_wheel_rx,
        cancelled_rx,
        ack_completions_rx,
    }
}

/// Binds an ephemeral UDP socket, runs `send_socket_assembler` with fixed test defaults
/// (source sender ID 0, port 0, `Gso::default()`, `Pool::new(u16::MAX)`, rate 100 Gbps),
/// and drains the pipeline to completion.
///
/// Use this as the body of a spawned assembler task.  Callers pass the pipeline-side
/// fields extracted from an [`AssemblerChannels`] destructuring; the assertion-side
/// receivers are kept in the calling scope for post-drain assertions.
async fn assembler_pipeline(
    immediate_rx: unsync::Receiver<send::TxImmediateAdapter>,
    ctx_rx: unsync::Receiver<send::TxWheelAdapter>,
    cancelled_tx: unsync::ListSender<crate::intrusive::EntryAdapter<frame::Frame>>,
    ack_completions_tx: unsync::ListSender<crate::intrusive::EntryAdapter<msg::Sender>>,
    asm_counters: AssemblerCounters,
    immediate_tx: send::ImmediateSender<unsync::Sender<send::TxImmediateAdapter>>,
    tx_wheel_tx: unsync::Sender<send::TxWheelAdapter>,
    pto_wheel_tx: unsync::Sender<send::PtoWheelAdapter>,
    idle_wheel_tx: unsync::Sender<send::IdleWheelAdapter>,
    clock: Clock,
) {
    let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    let send_counters = crate::endpoint::counters::Send::new(
        &crate::counter::Registry::default(),
        crate::endpoint::id::LocalSenderId::from_index(0),
    );
    let rx = tasks::send_socket_assembler(
        immediate_rx,
        ctx_rx,
        clock,
        crate::endpoint::id::LocalSenderId::new(VarInt::from_u8(0)),
        0,
        Gso::default(),
        Pool::new(u16::MAX),
        cancelled_tx,
        ack_completions_tx,
        asm_counters,
        send_counters,
        Rate::new(100.0),
        socket,
        immediate_tx,
        tx_wheel_tx,
        pto_wheel_tx,
        idle_wheel_tx,
    );
    rx.drain_budgeted(Some(32)).await;
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// A fully-assembled `Context` with one queued QueueData frame is fed through the
/// pipeline.
///
/// Output-channel assertions after the drain:
/// - an encrypted datagram arrives at the peer (positive — frame was sent)
/// - context re-armed on PTO wheel (inflight data present after the send)
/// - context re-armed on idle wheel (context is active)
/// - TX wheel NOT re-armed (no more pending data)
/// - `cancelled` empty (no frames were dropped)
/// - `ack_completions` empty (no ACK frame in this batch)
#[test]
fn sends_encrypted_packet_to_peer() {
    sim(|| {
        let registry = crate::counter::Registry::default();
        let clock = Clock::default();

        let AssemblerChannels {
            mut ctx_tx,
            ctx_rx,
            immediate_rx,
            immediate_tx,
            cancelled_tx,
            ack_completions_tx,
            asm_counters,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            mut tx_wheel_rx,
            mut pto_wheel_rx,
            mut idle_wheel_rx,
            mut cancelled_rx,
            mut ack_completions_rx,
        } = assembler_channels(&registry);

        // Bind the recv socket in the "server" group so its IP can be resolved by name.
        async {
            let recv_socket = UdpSocket::bind("0.0.0.0:4433").await.unwrap();
            let mut buf = vec![0u8; 1500];
            let (n, _peer) = recv_socket.recv_from(&mut buf).await.unwrap();
            debug!(n, "received encrypted datagram at server");
            assert!(n > 0, "assembler should have sent an encrypted packet");
        }
        .group("server")
        .primary()
        .spawn();

        // Run the assembler pipeline.
        let asm_clock = clock.clone();
        async move {
            assembler_pipeline(
                immediate_rx,
                ctx_rx,
                cancelled_tx,
                ack_completions_tx,
                asm_counters,
                immediate_tx,
                tx_wheel_tx,
                pto_wheel_tx,
                idle_wheel_tx,
                asm_clock,
            )
            .await;

            // drain_budgeted consumes the pipeline, dropping all internal senders,
            // so empty receivers now return None immediately.
            debug!("asserting wheel routing after assembly");
            assert!(
                pto_wheel_rx.recv().await.is_some(),
                "context with inflight data should be routed to PTO wheel"
            );
            assert!(
                idle_wheel_rx.recv().await.is_some(),
                "active context should always be routed to idle wheel"
            );
            assert!(
                tx_wheel_rx.recv().await.is_none(),
                "no pending data after send — TX wheel should not be re-armed"
            );
            assert!(
                cancelled_rx.recv().await.is_none(),
                "QueueData frame should not be cancelled"
            );
            assert!(
                ack_completions_rx.recv().await.is_none(),
                "no ACK completions for a plain QueueData frame"
            );
            debug!("all output assertions passed");
        }
        .spawn();

        // Resolve the server address via Bach DNS, build the context, and feed it.
        async move {
            let entry = test_entry_at("server:4433").await;
            let ctx = build_send_context(&entry, 0, &registry, &clock);
            let _ = ctx
                .borrow_mut()
                .push_batch(test_batch(&entry).into_inner(), &clock);
            ctx.borrow_mut().tx_wheel.target_time = Some(clock.now());
            let _ = ctx_tx.send(ctx);
            drop(ctx_tx);
        }
        .spawn();
    });
}

/// Two large frames (~1300 bytes of payload each) are pushed into a single context.
/// The first frame fills one MTU-sized segment; the second is pushed back by the
/// assembler and remains pending.  After the drain:
///
/// - TX wheel IS re-armed (pending second frame)
/// - PTO wheel IS re-armed (first frame is inflight)
/// - idle wheel IS re-armed (context is active)
/// - exactly one datagram arrives at the peer (only the first frame was sent)
/// - `cancelled` and `ack_completions` remain empty
#[test]
fn reassembles_context_to_tx_wheel_when_data_remains() {
    sim(|| {
        let registry = crate::counter::Registry::default();
        let clock = Clock::default();

        let AssemblerChannels {
            mut ctx_tx,
            ctx_rx,
            immediate_rx,
            immediate_tx,
            cancelled_tx,
            ack_completions_tx,
            asm_counters,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            mut tx_wheel_rx,
            mut pto_wheel_rx,
            mut idle_wheel_rx,
            mut cancelled_rx,
            mut ack_completions_rx,
        } = assembler_channels(&registry);

        // Recv side: exactly one datagram should arrive (only the first frame fits).
        // A second recv wrapped in a short timeout must time out to confirm no extra
        // datagrams were sent.
        async {
            let recv_socket = UdpSocket::bind("0.0.0.0:4433").await.unwrap();
            let mut buf = vec![0u8; 2000];
            let (n, _peer) = recv_socket.recv_from(&mut buf).await.unwrap();
            debug!(n, "received encrypted datagram at server");
            assert!(n > 0, "assembler should have sent the first large frame");

            // Assert no second datagram arrives.
            let result =
                bach::time::timeout(Duration::from_millis(1), recv_socket.recv_from(&mut buf))
                    .await;
            assert!(
                result.is_err(),
                "only one datagram should be sent — second recv must time out"
            );
        }
        .group("server")
        .primary()
        .spawn();

        let asm_clock = clock.clone();
        async move {
            assembler_pipeline(
                immediate_rx,
                ctx_rx,
                cancelled_tx,
                ack_completions_tx,
                asm_counters,
                immediate_tx,
                tx_wheel_tx,
                pto_wheel_tx,
                idle_wheel_tx,
                asm_clock,
            )
            .await;

            // The second frame remains pending → TX wheel must be re-armed.
            assert!(
                tx_wheel_rx.recv().await.is_some(),
                "remaining pending data should re-arm the TX wheel"
            );
            // First frame is in-flight → PTO must be scheduled.
            assert!(
                pto_wheel_rx.recv().await.is_some(),
                "inflight data should arm the PTO wheel"
            );
            // Context is still active → idle timeout must be scheduled.
            assert!(
                idle_wheel_rx.recv().await.is_some(),
                "active context should always be routed to idle wheel"
            );
            // Neither frame was cancelled and no ACK completions are expected.
            assert!(
                cancelled_rx.recv().await.is_none(),
                "no frames should be cancelled"
            );
            assert!(
                ack_completions_rx.recv().await.is_none(),
                "no ACK completions for QueueData frames"
            );
        }
        .spawn();

        async move {
            let entry = test_entry_at("server:4433").await;
            let ctx = build_send_context(&entry, 0, &registry, &clock);
            {
                let mut c = ctx.borrow_mut();
                // Push two large frames — each with a payload large enough that both
                // together exceed one MTU, so the assembler can only pack the first
                // into the single allowed segment.
                let _ = c.push_batch(test_batch_with_payload(&entry, 1300).into_inner(), &clock);
                let _ = c.push_batch(test_batch_with_payload(&entry, 1300).into_inner(), &clock);
                c.tx_wheel.target_time = Some(clock.now());
            }
            let _ = ctx_tx.send(ctx);
            drop(ctx_tx);
        }
        .spawn();
    });
}

/// A frame whose `CompletionReceiver` is explicitly cancelled (via `rx.cancel()`)
/// before being pushed into the context is routed to the `cancelled` output channel
/// because the assembler sees `!frame.should_transmit()`.
///
/// Output-channel assertions:
/// - `cancelled` receives exactly one frame (the cancelled one)
/// - no datagram reaches the peer: the server recv is wrapped in a short bach timeout
///   and must time out — nothing was encoded, so nothing was sent
/// - TX wheel NOT re-armed (no pending data after the cancelled frame is discarded)
/// - PTO wheel NOT re-armed (no inflight data)
/// - idle wheel IS re-armed (context is still active)
/// - `ack_completions` empty
#[test]
fn cancelled_frame_emitted_when_completion_is_cancelled() {
    sim(|| {
        let registry = crate::counter::Registry::default();
        let clock = Clock::default();

        let AssemblerChannels {
            mut ctx_tx,
            ctx_rx,
            immediate_rx,
            immediate_tx,
            cancelled_tx,
            ack_completions_tx,
            asm_counters,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            mut tx_wheel_rx,
            mut pto_wheel_rx,
            mut idle_wheel_rx,
            mut cancelled_rx,
            mut ack_completions_rx,
        } = assembler_channels(&registry);

        // Server: assert that no datagram arrives.  The recv is wrapped in a 1 ms
        // simulated-time timeout; since no packet is ever sent, the timeout must fire.
        async {
            let recv_socket = UdpSocket::bind("0.0.0.0:4433").await.unwrap();
            let mut buf = vec![0u8; 1500];
            let result =
                bach::time::timeout(Duration::from_millis(1), recv_socket.recv_from(&mut buf))
                    .await;
            assert!(
                result.is_err(),
                "no datagram should arrive for a cancelled frame"
            );
            debug!("server confirmed no packet arrived (timeout as expected)");
        }
        .group("server")
        .primary()
        .spawn();

        let asm_clock = clock.clone();
        async move {
            assembler_pipeline(
                immediate_rx,
                ctx_rx,
                cancelled_tx,
                ack_completions_tx,
                asm_counters,
                immediate_tx,
                tx_wheel_tx,
                pto_wheel_tx,
                idle_wheel_tx,
                asm_clock,
            )
            .await;

            // The cancelled frame must appear on the cancelled output.
            assert!(
                cancelled_rx.recv().await.is_some(),
                "assembler should route a cancelled frame to the cancelled channel"
            );
            // No data was encoded, so TX and PTO wheels must not be re-armed.
            assert!(
                tx_wheel_rx.recv().await.is_none(),
                "no pending data after cancellation — TX wheel must not be re-armed"
            );
            assert!(
                pto_wheel_rx.recv().await.is_none(),
                "no inflight data — PTO wheel must not be re-armed"
            );
            // The context is still live, so idle timeout is re-armed.
            assert!(
                idle_wheel_rx.recv().await.is_some(),
                "active context should be routed to idle wheel even when no data was sent"
            );
            assert!(
                ack_completions_rx.recv().await.is_none(),
                "no ACK completions when only a cancelled data frame was processed"
            );
            debug!("all output assertions passed");
        }
        .spawn();

        async move {
            let entry = test_entry_at("server:4433").await;
            // Create a completion receiver and cancel it immediately.
            // frame.should_transmit() will return false, triggering the cancelled path.
            let completion_rx = frame::completion_channel();
            let completion_sender = completion_rx.sender();
            completion_rx.cancel();

            let ctx = build_send_context(&entry, 0, &registry, &clock);
            {
                let mut c = ctx.borrow_mut();
                let mut batch = crate::endpoint::combinator::FrameBatch::single(
                    crate::intrusive::Entry::new(frame::Frame {
                        header: frame::Header::QueueData {
                            queue_pair: crate::packet::datagram::QueuePair {
                                source_queue_id: VarInt::from_u8(1),
                                dest_queue_id: VarInt::from_u8(2),
                            },
                            binding_id: VarInt::from_u8(1),
                            offset: VarInt::ZERO,
                            is_fin: false,
                        },
                        source_sender_id: crate::endpoint::id::LocalSenderId::new(VarInt::MAX),
                        payload: Default::default(),
                        path_secret_entry: entry.clone(),
                        completion: Some(completion_sender),
                        status: frame::TransmissionStatus::Pending,
                        ttl: 3,
                        transmission_time: None,
                    }),
                );
                batch.set_sender_id(crate::endpoint::id::LocalSenderId::from_index(0));
                let _ = c.push_batch(batch, &clock);
                c.tx_wheel.target_time = Some(clock.now());
            }
            let _ = ctx_tx.send(ctx);
            drop(ctx_tx);
        }
        .spawn();
    });
}

/// When the context input channel closes with no items, the pipeline terminates
/// without hanging or panicking, and no output channels receive any items.
#[test]
fn shuts_down_on_closed_input() {
    sim(|| {
        let registry = crate::counter::Registry::default();
        let clock = Clock::default();

        let AssemblerChannels {
            ctx_tx,
            ctx_rx,
            immediate_rx,
            immediate_tx,
            cancelled_tx,
            ack_completions_tx,
            asm_counters,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            ..
        } = assembler_channels(&registry);

        // Close the input before anything is sent.
        drop(ctx_tx);

        async move {
            assembler_pipeline(
                immediate_rx,
                ctx_rx,
                cancelled_tx,
                ack_completions_tx,
                asm_counters,
                immediate_tx,
                tx_wheel_tx,
                pto_wheel_tx,
                idle_wheel_tx,
                clock,
            )
            .await;
        }
        .primary()
        .spawn();
    });
}
