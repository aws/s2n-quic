// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::precision,
    socket::{
        channel::{self, Flatten, Priority, Reporter},
        rate::Rate,
        send::{
            completion::Completion,
            transmission::{Entry, Transmission},
        },
    },
};
use std::future::Future;

// ── Socket Sender ──────────────────────────────────────────────────────────

/// Receives entries from a channel and sends them on a socket.
///
/// This function is now a simple wrapper around composable channel adapters:
/// - FilterAlive: checks if completion receiver is alive, returns Result<Entry, Entry>
/// - InspectErr: handles dead entries (TODO: push to cleanup channel)
/// - SocketSender: sends on socket, returns Result<Entry, (Error, Entry)>
/// - InspectErr: logs socket errors (TODO: push to cleanup channel)
/// - CompletionNotifier: notifies completions, returns Result<(), Entry>
/// - InspectErr: handles failed completions (TODO: push to cleanup channel)
///
/// Returns when the receiver signals that all senders are gone.
pub async fn socket_sender<S, Clk, Info, Meta, C, R>(socket: S, _clock: Clk, rx: R)
where
    S: super::Socket,
    Clk: precision::Clock,
    C: Completion<Info, Meta>,
    R: channel::Receiver<Entry<Info, Meta, C>>,
{
    use channel::ReceiverExt;

    // Capture local_addr before moving the socket
    let local_addr = socket.local_addr().unwrap();

    // Chain the adapters
    let rx = channel::FilterAlive::new(rx);
    let rx = channel::InspectErr::new(rx, |_entry| {
        // TODO push dropped entries into channel for dropping outside of the runtime
    });
    let rx = channel::SocketSender::new(rx, socket);
    let rx = channel::InspectErr::new(rx, |(err, _entry)| {
        tracing::warn!("socket send error: {err}");
        // TODO push dropped entries into channel for dropping outside of the runtime
    });
    let rx = channel::CompletionNotifier::new(rx);
    let rx = channel::InspectErr::new(rx, |_entry| {
        // TODO push dropped entries into channel for dropping outside of the runtime
    });

    rx.drain().await;

    tracing::info!(%local_addr, "shutting down UDP sender");
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Sends packets on a non-blocking socket with priority-based receivers.
///
/// Each receiver represents a priority level (index 0 = highest priority).
/// Receives queues of transmissions, flattens them to entries, merges via
/// Priority, adds rate pacing, and sends on the socket.
///
/// Callers should use `channel::pump` to forward timing wheels into intrusive queue channels:
/// ```ignore
/// let (tx, rx) = channel::intrusive_queue::sync::new();
/// spawn(channel::pump(wheel, tx));
/// receivers.push(rx);
/// ```
pub fn non_blocking<S, Clk, Info, Meta, C, R>(
    socket: S,
    receivers: Vec<R>,
    clock: Clk,
    rate: Rate,
) -> impl Future<Output = ()> + Send
where
    S: super::Socket,
    Clk: precision::Clock + Clone,
    C: Completion<Info, Meta>,
    R: channel::Receiver<crate::intrusive_queue::Queue<Transmission<Info, Meta, C>>>,
{
    // SAFETY: The future uses Rc-based cell channels internally, which are !Send.
    // However, the entire future is polled as a single unit — the Rc's are created
    // inside the async block and never escape it. No Rc crosses a thread boundary.
    AssertSend(non_blocking_inner(socket, receivers, clock, rate))
}

async fn non_blocking_inner<S, Clk, Info, Meta, C, R>(
    socket: S,
    receivers: Vec<R>,
    clock: Clk,
    rate: Rate,
) where
    S: super::Socket,
    Clk: precision::Clock + Clone,
    C: Completion<Info, Meta>,
    R: channel::Receiver<crate::intrusive_queue::Queue<Transmission<Info, Meta, C>>>,
{
    assert!(!receivers.is_empty());

    // Flatten each receiver's queues into individual entries
    let receivers: Vec<_> = receivers.into_iter().map(|rx| Flatten::new(rx)).collect();

    // Merge all via Priority (index 0 = highest priority)
    let receivers = Priority::new(receivers);
    let receivers = Reporter::new(receivers, clock.clone(), cfg!(debug_assertions));
    let receivers = channel::Paced::new(receivers, clock.clone(), rate);

    socket_sender(socket, clock, receivers).await;
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Wrapper that asserts a future is `Send` even if the compiler can't prove it.
///
/// SAFETY: The caller must ensure the future is only polled from a single thread
/// and that no !Send data escapes the future.
struct AssertSend<F>(F);

// SAFETY: The non_blocking_inner future contains Rc-based cell channels which are
// !Send. However, all Rc's are created within the async block, polled inline via
// futures_join (never spawned separately), and dropped when the future completes.
// No Rc ever crosses a thread boundary.
unsafe impl<F: Future> Send for AssertSend<F> {}

impl<F: Future> Future for AssertSend<F> {
    type Output = F::Output;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        // SAFETY: We're just delegating to the inner future. Pin projection is safe
        // because AssertSend is a transparent wrapper.
        unsafe { self.map_unchecked_mut(|s| &mut s.0) }.poll(cx)
    }
}
