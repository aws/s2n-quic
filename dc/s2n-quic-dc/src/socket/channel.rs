// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Single-entry channels for connecting wheel ticker tasks to a socket sender.
//!
//! # Implementation Guidelines
//!
//! ## Avoid Unbounded Loops in `poll_*` Methods
//!
//! **CRITICAL**: Never use unbounded `loop` or `while` constructs inside `poll_recv`,
//! `poll_send`, or any `poll_*` method. Loops can cause task starvation by monopolizing
//! the executor and preventing other tasks from making progress.
//!
//! ### Why This Matters
//!
//! When a `poll_*` method contains a loop that keeps returning `Ready`, it can:
//! - Starve other tasks on the same executor worker
//! - Cause pipeline stalls where one component loops while another waits
//! - Make the system unresponsive under load
//! - Create difficult-to-diagnose performance issues
//!
//! ### The Self-Wake Pattern
//!
//! Instead of looping, use the **self-wake pattern**: return `Pending` and wake yourself
//! to be polled again. This gives the executor a chance to poll other tasks.
//!
//! ```rust,ignore
//! // ❌ BAD: Unbounded loop
//! fn poll_recv(&mut self, cx: &mut Context) -> Poll<Option<T>> {
//!     loop {
//!         if let Some(item) = self.buffer.pop() {
//!             return Poll::Ready(Some(item));
//!         }
//!         match self.inner.poll_recv(cx) {
//!             Poll::Ready(Some(batch)) => {
//!                 self.buffer = batch;
//!                 continue; // ❌ Loops without yielding
//!             }
//!             Poll::Pending => return Poll::Pending,
//!             Poll::Ready(None) => return Poll::Ready(None),
//!         }
//!     }
//! }
//!
//! // ✅ GOOD: Self-wake instead of loop
//! fn poll_recv(&mut self, cx: &mut Context) -> Poll<Option<T>> {
//!     if let Some(item) = self.buffer.pop() {
//!         cx.waker().wake_by_ref(); // Wake to process more items
//!         return Poll::Ready(Some(item));
//!     }
//!     match self.inner.poll_recv(cx) {
//!         Poll::Ready(Some(batch)) => {
//!             self.buffer = batch;
//!             cx.waker().wake_by_ref(); // Wake to process batch
//!             Poll::Pending // ✅ Yield to executor
//!         }
//!         Poll::Pending => Poll::Pending,
//!         Poll::Ready(None) => Poll::Ready(None),
//!     }
//! }
//! ```
//!
//! ### Exception: Bounded Loops
//!
//! Small, **provably bounded** loops are acceptable when the bound is small (e.g., < 10
//! iterations) and based on a fixed-size collection:
//!
//! ```rust,ignore
//! // ✅ OK: Bounded by fixed array size
//! for (idx, rx) in self.receivers.iter_mut().enumerate() {
//!     if let Poll::Ready(Some(value)) = rx.poll_recv(cx) {
//!         return Poll::Ready(Some(value));
//!     }
//! }
//! ```
//!
//! ### Debugging Loop-Related Issues
//!
//! If you see:
//! - Tasks not making progress despite being woken
//! - Some pipeline stages receiving data while others don't
//! - Continuous polling without yielding
//!
//! Check for unbounded loops in `poll_*` methods and replace them with self-waking.

use crate::{
    packet::datagram::partial::{FailureReason, TransmissionInfo, TransmissionStatus},
    socket::{
        pool::descriptor,
        send::{completion::Completer as _, transmission},
    },
};
use core::{
    cell::RefCell,
    fmt,
    task::{self, Poll},
};
use s2n_quic_core::{assume, ready, varint::VarInt};
use std::{future::Future, io, marker::PhantomData, mem::MaybeUninit, rc::Rc, sync::Arc};

pub mod cell;
pub mod intrusive_queue;

#[cfg(test)]
mod tests;

/// A channel sender without backpressure.
///
/// Implementations never block - sends either succeed immediately or the channel is closed.
pub trait UnboundedSender<T> {
    /// Send a value on the channel without blocking.
    ///
    /// Returns `Ok(())` if the value was sent successfully.
    /// Returns `Err(value)` if the channel is closed, returning ownership of the value.
    fn send(&mut self, value: T) -> Result<(), T>;
}

/// A channel sender.
pub trait Sender<T> {
    /// Poll to send a value on the channel.
    ///
    /// On `Poll::Ready(Ok(()))`, the value was sent successfully and taken from `value`.
    /// On `Poll::Ready(Err(()))`, the channel is closed and the value remains in `value`.
    /// On `Poll::Pending`, the send is not complete yet - the caller should poll again with the same value.
    fn poll_send(
        &mut self,
        cx: &mut task::Context<'_>,
        value: &mut core::mem::MaybeUninit<T>,
    ) -> Poll<Result<(), ()>>;

    /// Sends the value on the channel. Returns `Err` if the channel is closed.
    async fn send(&mut self, value: T) -> Result<(), T> {
        let mut slot = core::mem::MaybeUninit::new(value);
        let mut taken = false;
        core::future::poll_fn(move |cx| {
            if taken {
                // Value was already taken, just return Ready
                return Poll::Ready(Ok(()));
            }

            match self.poll_send(cx, &mut slot) {
                Poll::Ready(Ok(())) => {
                    taken = true;
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(())) => {
                    taken = true;
                    // Channel closed, extract the value
                    Poll::Ready(Err(unsafe { slot.assume_init_read() }))
                }
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }
}

/// An async-capable channel receiver.
pub trait Receiver<T> {
    /// Poll for the next value. Registers the waker if nothing is available.
    ///
    /// Returns `Ready(Some(value))` when a value is available,
    /// `Pending` when empty but not closed,
    /// `Ready(None)` when the channel is closed.
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>>;

    /// Receives the next value. Returns `None` when the channel is closed.
    fn recv(&mut self) -> impl core::future::Future<Output = Option<T>> + '_ {
        core::future::poll_fn(|cx| self.poll_recv(cx))
    }

    /// Notify the receiver that the last item received was consumed.
    ///
    /// This is used by paced receivers to trigger token consumption after
    /// confirming the item was actually processed (not cancelled).
    fn on_consumed(&mut self, bytes: u64);
}

/// Extension methods for `Receiver<T>`.
/// Trait for types that have a byte cost (for bandwidth tracking and reporting)
pub trait ByteCost {
    /// Returns the byte cost of this item (e.g., packet size, total transmission bytes)
    fn byte_cost(&self) -> u64;
}

/// Trait for types that can be sent over a socket
pub trait Sendable: ByteCost {
    /// Send this item over the given socket
    fn send<S: crate::socket::send::Socket>(&mut self, socket: &S) -> std::io::Result<()>;
}

pub trait ReceiverExt<T>: Receiver<T> + Sized {
    /// Drains the receiver until it returns `None`.
    fn drain(self) -> impl core::future::Future<Output = ()>
    where
        Self: Receiver<()>,
    {
        self.drain_budgeted(None)
    }

    /// Drain the receiver, processing up to `budget` items per poll before yielding.
    /// `None` means process one item per poll.
    fn drain_budgeted(mut self, budget: Option<usize>) -> impl core::future::Future<Output = ()>
    where
        Self: Receiver<()>,
    {
        let budget = budget.unwrap_or(1);
        core::future::poll_fn(move |cx| {
            for _ in 0..budget {
                match self.poll_recv(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(None) => return Poll::Ready(()),
                    Poll::Ready(Some(())) => {}
                }
            }
            cx.waker().wake_by_ref();
            Poll::Pending
        })
    }

    /// Wraps the receiver with a debug adapter that logs received values.
    fn dbg(self, label: &'static str) -> Dbg<T, Self> {
        Dbg::new(self, label)
    }
}

impl<T, R> ReceiverExt<T> for R where R: Receiver<T> {}

// ── Priority merging receiver ──────────────────────────────────────────────

/// Merges multiple receivers, always polling the highest-priority (lowest-index)
/// channel first.
///
/// When `poll_recv` is called, receivers are checked in order from index 0
/// (highest priority) to the last. The first `Ready(Some(..))` value is returned.
/// If all receivers return `Pending`, `Pending` is returned. If all receivers
/// are closed (`Ready(None)`), `Ready(None)` is returned.
pub struct Priority<R> {
    receivers: Vec<R>,
    last_recv_idx: Option<usize>,
}

impl<R> Priority<R> {
    pub fn new(receivers: Vec<R>) -> Self {
        Self {
            receivers,
            last_recv_idx: None,
        }
    }
}

impl<T, R: Receiver<T>> Receiver<T> for Priority<R> {
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        let mut all_closed = true;
        for (idx, rx) in self.receivers.iter_mut().enumerate() {
            match rx.poll_recv(cx) {
                Poll::Ready(Some(value)) => {
                    self.last_recv_idx = Some(idx);
                    return Poll::Ready(Some(value));
                }
                Poll::Pending => {
                    all_closed = false;
                }
                Poll::Ready(None) => {
                    // This channel is closed, continue to lower priority
                }
            }
        }
        if all_closed {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        if let Some(idx) = self.last_recv_idx {
            self.receivers[idx].on_consumed(bytes);
        }
    }
}

// ── Flatten adapter ────────────────────────────────────────────────────────

/// Wraps a `Receiver<C>` where C is a container that can be converted to an iterator.
///
/// When `recv` is called, it first drains any buffered entries from the
/// current iterator. Once the iterator is exhausted, it pulls the next container
/// from the inner receiver and converts it to an iterator.
pub struct Flatten<I, R, C> {
    inner: R,
    iter: Option<I>,
    _phantom: PhantomData<C>,
}

impl<Item, I, R, C> Flatten<I, R, C>
where
    I: Iterator<Item = Item>,
    C: IntoIterator<IntoIter = I, Item = Item>,
    R: Receiver<C>,
{
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            iter: None,
            _phantom: PhantomData,
        }
    }
}

impl<Item, I, C, R> Receiver<Item> for Flatten<I, R, C>
where
    I: Iterator<Item = Item>,
    C: IntoIterator<IntoIter = I, Item = Item>,
    R: Receiver<C>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<Item>> {
        // Drain any buffered entries first
        if let Some(iter) = &mut self.iter {
            if let Some(item) = iter.next() {
                // Self-wake to drain more entries
                cx.waker().wake_by_ref();
                return Poll::Ready(Some(item));
            }
        }

        // Iterator exhausted, clear it and try to pull the next container
        self.iter = None;

        // Try to pull the next container from the inner receiver
        match self.inner.poll_recv(cx) {
            Poll::Ready(Some(container)) => {
                self.iter = Some(container.into_iter());
                // Self-wake to process the new iterator
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

/// Wraps a `Receiver<Queue<T>>` and implements `Receiver<Entry<T>>`.
///
/// Specialized version of `Flatten` for the common case of intrusive queues.
/// This avoids type inference issues with the generic `Flatten`.
pub struct FlattenQueue<T, R> {
    inner: R,
    queue: crate::intrusive_queue::Queue<T>,
}

impl<T, R> FlattenQueue<T, R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            queue: Default::default(),
        }
    }
}

impl<T, R> Receiver<crate::intrusive_queue::Entry<T>> for FlattenQueue<T, R>
where
    R: Receiver<crate::intrusive_queue::Queue<T>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<crate::intrusive_queue::Entry<T>>> {
        loop {
            // Drain any buffered entries first
            if let Some(entry) = self.queue.pop_front() {
                return Poll::Ready(Some(entry));
            }

            // Try to pull the next queue from the inner receiver
            match self.inner.poll_recv(cx) {
                Poll::Ready(Some(queue)) => {
                    if queue.is_empty() {
                        // Self-wake to process the new queue
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                    self.queue = queue;
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

/// Specialized version of `Flatten` for intrusive lists with adapters.
/// This is useful for flattening List<A> into A::Pointer entries.
pub struct FlattenList<A, R>
where
    A: crate::intrusive_queue::Adapter,
{
    inner: R,
    list: crate::intrusive_queue::List<A>,
}

impl<A, R> FlattenList<A, R>
where
    A: crate::intrusive_queue::Adapter,
{
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            list: crate::intrusive_queue::List::new(),
        }
    }
}

impl<A, R> Receiver<A::Pointer> for FlattenList<A, R>
where
    A: crate::intrusive_queue::Adapter,
    R: Receiver<crate::intrusive_queue::List<A>>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<A::Pointer>> {
        // Drain any buffered entries first
        if let Some(entry) = self.list.pop_front() {
            // Self-wake to drain more entries
            cx.waker().wake_by_ref();
            return Poll::Ready(Some(entry));
        }

        // Try to pull the next list from the inner receiver
        match self.inner.poll_recv(cx) {
            Poll::Ready(Some(list)) => {
                self.list = list;
                // Self-wake to process the new list
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

/// Calls an inspect function when the inner receiver returns a value.
///
/// Useful for side effects like calling `wheel.on_send()` after a flatten,
/// before the priority merge.
pub struct Inspect<T, R, F> {
    inner: R,
    inspect: F,
    _value: PhantomData<T>,
}

impl<T, R, F> Inspect<T, R, F> {
    pub fn new(inner: R, inspect: F) -> Self {
        Self {
            inner,
            inspect,
            _value: PhantomData,
        }
    }
}

impl<T, R, F> Receiver<T> for Inspect<T, R, F>
where
    R: Receiver<T>,
    F: Fn(&T),
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        match self.inner.poll_recv(cx) {
            Poll::Ready(Some(value)) => {
                (self.inspect)(&value);
                Poll::Ready(Some(value))
            }
            other => other,
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Map adapter ────────────────────────────────────────────────────────────

/// Maps items from one type to another using a transformation function.
pub struct Map<T, U, R, F> {
    inner: R,
    map: F,
    _phantom: PhantomData<(T, U)>,
}

impl<T, U, R, F> Map<T, U, R, F>
where
    R: Receiver<T>,
    F: FnMut(T) -> U,
{
    pub fn new(inner: R, map: F) -> Self {
        Self {
            inner,
            map,
            _phantom: PhantomData,
        }
    }
}

impl<T, U, R, F> Receiver<U> for Map<T, U, R, F>
where
    R: Receiver<T>,
    F: FnMut(T) -> U,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<U>> {
        match self.inner.poll_recv(cx) {
            Poll::Ready(Some(value)) => {
                let mapped = (self.map)(value);
                Poll::Ready(Some(mapped))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── FilterMap adapter ──────────────────────────────────────────────────────

/// Filters and maps items from the inner receiver.
///
/// Similar to Iterator::filter_map, applies a function that returns Option<U>.
/// Only returns Some values, filtering out None.
pub struct FilterMap<R, F, T, U> {
    inner: R,
    filter_map: F,
    _phantom: PhantomData<(T, U)>,
}

impl<R, F, T, U> FilterMap<R, F, T, U>
where
    R: Receiver<T>,
    F: FnMut(T) -> Option<U>,
{
    pub fn new(inner: R, filter_map: F) -> Self {
        Self {
            inner,
            filter_map,
            _phantom: PhantomData,
        }
    }
}

impl<R, F, T, U> Receiver<U> for FilterMap<R, F, T, U>
where
    R: Receiver<T>,
    F: FnMut(T) -> Option<U>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<U>> {
        // Try up to 10 items before yielding (bounded loop)
        for _ in 0..10 {
            match self.inner.poll_recv(cx) {
                Poll::Ready(Some(value)) => {
                    if let Some(mapped) = (self.filter_map)(value) {
                        return Poll::Ready(Some(mapped));
                    }
                    // Filtered out, continue to next item
                    continue;
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }

        // Hit iteration limit, wake up to continue
        cx.waker().wake_by_ref();
        Poll::Pending
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Paced adapter ──────────────────────────────────────────────────────────

/// Wraps a receiver with rate-based pacing using a token bucket.
///
/// Items are pulled from the inner receiver, but pacing is applied via
/// `on_consumed` — only when the consumer confirms an item was processed
/// are tokens consumed and a timer is armed. Subsequent `poll_recv` calls
/// will poll the timer until it fires.
///
/// This allows cancelled/skipped items to avoid rate limiting.
pub struct Paced<R, Clk, T>
where
    Clk: crate::clock::precision::Clock,
{
    inner: R,
    timer: Clk::Timer,
    rate: crate::socket::rate::Rate,
    bucket: crate::socket::rate::TokenBucket,
    buffered: Option<T>,
}

impl<R, Clk, T> Paced<R, Clk, T>
where
    Clk: crate::clock::precision::Clock,
{
    pub fn new(inner: R, clock: Clk, rate: crate::socket::rate::Rate) -> Self {
        use crate::clock::precision::Timer;

        let timer = clock.timer();
        let now = timer.now();
        let bucket = crate::socket::rate::TokenBucket::new(now, &rate);
        Self {
            inner,
            timer,
            rate,
            bucket,
            buffered: None,
        }
    }
}

impl<T, R, Clk> Receiver<T> for Paced<R, Clk, T>
where
    R: Receiver<T>,
    Clk: crate::clock::precision::Clock,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        use crate::clock::precision::Timer;

        // If we have a buffered item from a previous poll, check timer first
        if self.buffered.is_some() {
            ready!(self.timer.poll_ready(cx));
            // Timer is ready, return the buffered item
            return Poll::Ready(self.buffered.take());
        }

        // Try to get next item from inner receiver
        let item = ready!(self.inner.poll_recv(cx));

        // If we got an item, check if timer is ready
        if item.is_some() {
            match self.timer.poll_ready(cx) {
                Poll::Ready(()) => {
                    // Timer ready, can return the item
                    Poll::Ready(item)
                }
                Poll::Pending => {
                    // Timer not ready, buffer the item and return Pending
                    self.buffered = item;
                    Poll::Pending
                }
            }
        } else {
            // No item (channel closed), return None
            Poll::Ready(None)
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        use crate::clock::precision::Timer;

        // Notify inner receiver first
        self.inner.on_consumed(bytes);

        // Then apply rate limiting
        let now = self.timer.now();
        let cost_nanos = self.rate.nanos_for_bytes(bytes);
        let sleep_nanos = self.bucket.consume(now, cost_nanos);

        if sleep_nanos > 0 {
            let target = now + std::time::Duration::from_nanos(sleep_nanos);
            self.timer.update(target);
        }
    }
}

// ── YieldAfter adapter ─────────────────────────────────────────────────────

/// Wraps a receiver and forces a yield after a threshold of consecutive Ready returns.
///
/// This prevents a busy receiver from starving other tasks by ensuring the
/// executor gets a chance to poll other futures periodically.
pub struct YieldAfter<R> {
    inner: R,
    ready_count: u32,
    threshold: u32,
}

impl<R> YieldAfter<R> {
    pub fn new(inner: R, threshold: u32) -> Self {
        Self {
            inner,
            ready_count: 0,
            threshold,
        }
    }
}

impl<T, R> Receiver<T> for YieldAfter<R>
where
    R: Receiver<T>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        // If we've hit the threshold, force a yield
        if self.ready_count >= self.threshold {
            self.ready_count = 0;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        match self.inner.poll_recv(cx) {
            Poll::Ready(value) => {
                self.ready_count += 1;
                Poll::Ready(value)
            }
            Poll::Pending => {
                // Reset count on Pending since we yielded
                self.ready_count = 0;
                Poll::Pending
            }
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Pump ───────────────────────────────────────────────────────────────────

/// Continuously pumps values from a receiver to a sender.
///
/// This ensures the receiver makes progress independently while buffering its
/// output into a channel. Useful for converting a pull-based receiver into a
/// push-based channel.
///
/// Returns when either the receiver or sender closes.
/// Transfers items from a receiver to a sender, yielding after each item.
pub async fn pump<T, R, S>(rx: R, tx: S)
where
    R: Receiver<T>,
    S: Sender<T>,
{
    pump_budgeted(rx, tx, None).await;
}

/// Transfers items from a receiver to a sender, yielding after `budget` items
/// are transferred in a single poll. `None` means transfer one item per poll.
pub async fn pump_budgeted<T, R, S>(rx: R, tx: S, budget: Option<usize>)
where
    R: Receiver<T>,
    S: Sender<T>,
{
    BudgetedPump {
        rx,
        tx,
        value: None,
        budget: budget.unwrap_or(1),
    }
    .await;

    struct BudgetedPump<T, R, S> {
        rx: R,
        tx: S,
        value: Option<core::mem::MaybeUninit<T>>,
        budget: usize,
    }

    impl<T, R, S> Future for BudgetedPump<T, R, S>
    where
        R: Receiver<T>,
        S: Sender<T>,
    {
        type Output = ();

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<Self::Output> {
            let this = unsafe { self.get_unchecked_mut() };

            for _ in 0..this.budget {
                if this.value.is_none() {
                    match this.rx.poll_recv(cx) {
                        Poll::Ready(Some(value)) => {
                            this.value = Some(MaybeUninit::new(value));
                        }
                        Poll::Ready(None) => {
                            return Poll::Ready(());
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }

                if let Some(v) = this.value.as_mut() {
                    match this.tx.poll_send(cx, v) {
                        Poll::Ready(Ok(())) => {
                            this.value = None;
                        }
                        Poll::Ready(Err(())) => return Poll::Ready(()),
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
            }

            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

// ── RoundRobin ─────────────────────────────────────────────────────────────

/// Routes entries from a receiver to multiple senders in round-robin fashion.
///
/// Receives items from `rx` and sends to the first available sender, starting
/// from the last used index to distribute load evenly.
pub async fn round_robin<T, R, S>(mut rx: R, mut senders: Vec<S>)
where
    T: ByteCost,
    R: Receiver<T>,
    S: Sender<T>,
{
    use core::future::poll_fn;

    let mut next_idx = 0;

    loop {
        // Receive next item
        let Some(entry) = rx.recv().await else {
            break;
        };

        let bytes = entry.byte_cost();
        let mut slot = core::mem::MaybeUninit::new(entry);

        // Try to send to the next available sender
        let start_idx = next_idx;
        let sent = poll_fn(|cx| {
            loop {
                if senders.is_empty() {
                    // All senders closed
                    return Poll::Ready(false);
                }

                match senders[next_idx].poll_send(cx, &mut slot) {
                    Poll::Ready(Ok(())) => {
                        // Successfully sent
                        next_idx = (next_idx + 1) % senders.len();
                        return Poll::Ready(true);
                    }
                    Poll::Ready(Err(())) => {
                        // This sender is closed, remove it
                        senders.swap_remove(next_idx);
                        if next_idx >= senders.len() && !senders.is_empty() {
                            next_idx = 0;
                        }
                        // Try next sender immediately
                        continue;
                    }
                    Poll::Pending => {
                        // This sender is full, try the next one
                        next_idx = (next_idx + 1) % senders.len();
                        if next_idx == start_idx {
                            // Tried all senders, all are full
                            return Poll::Pending;
                        }
                        // Try next sender immediately
                        continue;
                    }
                }
            }
        })
        .await;

        if !sent {
            // All senders closed
            break;
        }

        // Notify that we consumed the item
        rx.on_consumed(bytes);
    }
}

// ── FilterAlive adapter ────────────────────────────────────────────────────

/// Wraps a receiver and filters out entries with dead completion receivers.
///
/// Returns `Ok(entry)` if the completion receiver is alive, `Err(entry)` if dead.
/// This allows downstream combinators to choose how to handle dead entries.
pub struct FilterAlive<R, Info, Meta, C> {
    inner: R,
    _phantom: PhantomData<(Info, Meta, C)>,
}

impl<R, Info, Meta, C> FilterAlive<R, Info, Meta, C> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            _phantom: PhantomData,
        }
    }
}

impl<R, Info, Meta, C>
    Receiver<Result<transmission::Entry<Info, Meta, C>, transmission::Entry<Info, Meta, C>>>
    for FilterAlive<R, Info, Meta, C>
where
    R: Receiver<transmission::Entry<Info, Meta, C>>,
    C: crate::socket::send::completion::Completion<Info, Meta>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<Result<transmission::Entry<Info, Meta, C>, transmission::Entry<Info, Meta, C>>>>
    {
        let Some(entry) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        if !entry.completion.is_alive() {
            return Poll::Ready(Some(Err(entry)));
        }

        Poll::Ready(Some(Ok(entry)))
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── SocketSender adapter ───────────────────────────────────────────────────

/// Wraps a receiver and sends items on a socket using the Sendable trait.
///
/// Receives sendable items and transmits them on the socket. Returns `Ok(item)` on
/// success or `Err((error, item))` on failure, allowing the caller to decide how to
/// handle errors while ensuring the item is always available for cleanup.
pub struct SocketSender<R, S> {
    inner: R,
    socket: S,
}

impl<R, S> SocketSender<R, S> {
    pub fn new(inner: R, socket: S) -> Self {
        Self { inner, socket }
    }
}

impl<R, S, T> Receiver<Result<T, (io::Error, T)>> for SocketSender<R, S>
where
    R: Receiver<T>,
    S: crate::socket::send::Socket,
    T: Sendable,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<Result<T, (io::Error, T)>>> {
        let Some(mut item) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        let result = match item.send(&self.socket) {
            Ok(()) => Ok(item),
            Err(err) => Err((err, item)),
        };

        Poll::Ready(Some(result))
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── InspectErr adapter ─────────────────────────────────────────────────────

/// Wraps a receiver of `Result<T, E>` and inspects errors.
///
/// Calls a callback on errors, then only yields successful values.
/// Failed values are dropped after the callback is invoked.
pub struct InspectErr<R, F, T, E> {
    inner: R,
    on_error: F,
    _phantom: PhantomData<(T, E)>,
}

impl<R, F, T, E> InspectErr<R, F, T, E>
where
    R: Receiver<Result<T, E>>,
    F: FnMut(E),
{
    pub fn new(inner: R, on_error: F) -> Self {
        Self {
            inner,
            on_error,
            _phantom: PhantomData,
        }
    }
}

impl<R, F, T, E> Receiver<T> for InspectErr<R, F, T, E>
where
    R: Receiver<Result<T, E>>,
    F: FnMut(E),
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        let Some(result) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        match result {
            Ok(value) => Poll::Ready(Some(value)),
            Err(err) => {
                (self.on_error)(err);
                // Drop error, self-wake to get next
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── UnwrapOk adapter ───────────────────────────────────────────────────────

/// Wraps a receiver of `Result<T, E>` and unwraps Ok values.
///
/// Only yields successful values. Failed values are silently dropped.
pub struct UnwrapOk<R, T, E> {
    inner: R,
    _phantom: PhantomData<(T, E)>,
}

impl<R, T, E> UnwrapOk<R, T, E> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            _phantom: PhantomData,
        }
    }
}

impl<R, T, E> Receiver<T> for UnwrapOk<R, T, E>
where
    R: Receiver<Result<T, E>>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        let Some(result) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        match result {
            Ok(value) => Poll::Ready(Some(value)),
            Err(_) => {
                // Drop error, self-wake to get next
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── CompletionNotifier adapter ─────────────────────────────────────────────

/// Wraps a receiver and notifies completions after receiving entries.
///
/// Checks if the completion receiver is still alive before yielding entries.
/// If alive, upgrades the completion and notifies it immediately after receiving.
pub struct CompletionNotifier<R, Info, Meta, C> {
    inner: R,
    _phantom: PhantomData<(Info, Meta, C)>,
}

impl<R, Info, Meta, C> CompletionNotifier<R, Info, Meta, C> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            _phantom: PhantomData,
        }
    }
}

impl<R, Info, Meta, C> Receiver<Result<(), transmission::Entry<Info, Meta, C>>>
    for CompletionNotifier<R, Info, Meta, C>
where
    R: Receiver<transmission::Entry<Info, Meta, C>>,
    C: crate::socket::send::completion::Completion<Info, Meta>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<Result<(), transmission::Entry<Info, Meta, C>>>> {
        let Some(entry) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        let total_len = entry.total_len as u64;

        // Try to upgrade the completion
        let Some(completion) = entry.completion.upgrade() else {
            // The sender went away; skip this entry
            return Poll::Ready(Some(Err(entry)));
        };

        // Complete the entry
        completion.complete(entry);

        // Notify that we consumed the packet
        self.inner.on_consumed(total_len);

        // Return unit since we've consumed the entry
        return Poll::Ready(Some(Ok(())));
    }

    fn on_consumed(&mut self, _bytes: u64) {
        // Already handled in poll_recv
    }
}

// ── CompletionBatcher adapter ──────────────────────────────────────────────

/// Batches PartialDatagram entries by completion sender for efficient notifications.
///
/// Receives individual PartialDatagram entries and buffers them into queues grouped
/// by completion sender pointer. When the completion sender changes or we're out of
/// items to process, the buffered queue is sent to the completion channel as a batch.
///
/// This reduces synchronization overhead by batching notifications to the same
/// completion channel rather than sending them one-by-one.
pub struct CompletionBatcher<R> {
    inner: R,
    /// Current batch being built
    current_batch: crate::intrusive_queue::Queue<crate::packet::datagram::partial::PartialDatagram>,
    /// Completion sender for the current batch (taken from first entry)
    current_sender: Option<crate::packet::datagram::partial::CompletionSender>,
    /// Total bytes in current batch (for on_consumed reporting)
    current_batch_bytes: u64,
}

impl<R> CompletionBatcher<R>
where
    R: Receiver<crate::intrusive_queue::Entry<crate::packet::datagram::partial::PartialDatagram>>,
{
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            current_batch: crate::intrusive_queue::Queue::new(),
            current_sender: None,
            current_batch_bytes: 0,
        }
    }

    /// Get the byte count for a PartialDatagram
    #[inline]
    fn entry_bytes(entry: &crate::packet::datagram::partial::PartialDatagram) -> u64 {
        // Use the sent_bytes from transmission_info if available, otherwise estimate
        if let Some(ref tx_info) = entry.transmission_info {
            tx_info.sent_bytes as u64
        } else {
            // Fall back to estimate (this shouldn't happen for ACKed packets)
            entry.estimate_encoded_len(16) as u64
        }
    }

    /// Flush the current batch to its completion sender
    fn flush_batch(&mut self) {
        if self.current_batch.is_empty() {
            return;
        }

        let batch = core::mem::take(&mut self.current_batch);
        let bytes = self.current_batch_bytes;
        let sender = self.current_sender.take();
        self.current_batch_bytes = 0;

        // Send the batch to the completion channel if we have a sender
        if let Some(sender) = sender {
            let _ = sender.send_batch(batch);
        }

        // Report bytes consumed to upstream
        self.inner.on_consumed(bytes);
    }
}

impl<R> Receiver<()> for CompletionBatcher<R>
where
    R: Receiver<crate::intrusive_queue::Entry<crate::packet::datagram::partial::PartialDatagram>>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<()>> {
        for _ in 0..10 {
            match self.inner.poll_recv(cx) {
                Poll::Ready(Some(mut entry)) => {
                    let bytes = Self::entry_bytes(&entry);

                    // Take the completion sender from the entry
                    let sender = entry.completion.take();

                    match (self.current_sender.as_ref(), sender) {
                        // Same sender - add to current batch
                        (Some(current), Some(new)) if current.queue_id() == new.queue_id() => {
                            self.current_batch_bytes += bytes;
                            self.current_batch.push_back(entry);
                            // Don't change the sender, since it matches the previous one
                        }
                        // Different sender or first entry - flush and start new batch
                        (_, Some(new_sender)) => {
                            // Flush the old batch if there is one
                            self.flush_batch();

                            // Start new batch with this sender
                            self.current_sender = Some(new_sender);
                            self.current_batch_bytes = bytes;
                            self.current_batch.push_back(entry);
                        }
                        // No completion sender - just drop it
                        (_, None) => {
                            // If we have a batch in progress, flush it first
                            self.flush_batch();
                            // Drop this entry (no completion notification needed)
                            // But still report the bytes consumed
                            self.inner.on_consumed(bytes);
                        }
                    }

                    // Continue processing - don't return yet
                    continue;
                }
                Poll::Ready(None) => {
                    // Input stream ended - flush any remaining batch
                    self.flush_batch();
                    return Poll::Ready(None);
                }
                Poll::Pending => {
                    // No more entries available right now
                    // Flush the current batch if we have one
                    self.flush_batch();
                    return Poll::Pending;
                }
            }
        }

        self.flush_batch();
        cx.waker().wake_by_ref();
        Poll::Pending
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Retransmission Batcher ─────────────────────────────────────────────────

/// Batches lost packets for retransmission using GSO.
///
/// Receives individual PartialDatagram entries (from loss detection) and
/// batches them by peer address into Batch entries. Uses `batch::Builder::try_push`
/// to determine when a batch is full - when `try_push` fails, flushes the
/// current batch and starts a new one.
pub struct RetransmissionBatcher<R> {
    inner: R,
    /// Current batch builder (None if no batch in progress)
    current_builder: Option<crate::datagram::batch::Builder>,
}

impl<R> RetransmissionBatcher<R>
where
    R: Receiver<crate::intrusive_queue::Entry<crate::packet::datagram::partial::PartialDatagram>>,
{
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            current_builder: None,
        }
    }

    /// Flush the current batch if there is one
    fn flush_batch(
        &mut self,
    ) -> Option<crate::intrusive_queue::Entry<crate::datagram::batch::Batch>> {
        self.current_builder
            .take()
            .map(|builder| crate::intrusive_queue::Entry::new(builder.finish()))
    }
}

impl<R> Receiver<crate::intrusive_queue::Entry<crate::datagram::batch::Batch>>
    for RetransmissionBatcher<R>
where
    R: Receiver<crate::intrusive_queue::Entry<crate::packet::datagram::partial::PartialDatagram>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<crate::intrusive_queue::Entry<crate::datagram::batch::Batch>>> {
        // Process up to 10 lost packets per poll
        for _ in 0..10 {
            match self.inner.poll_recv(cx) {
                Poll::Ready(Some(entry)) => {
                    // Get the peer address from the datagram
                    let peer_addr = entry.remote_address();

                    // Try to add to current builder
                    if let Some(ref mut builder) = self.current_builder {
                        // Try to push into current builder
                        match builder.try_push(entry) {
                            Ok(()) => {
                                // Successfully added to batch, continue processing
                                // Note: try_push automatically sets sticky sender_id if needed
                                continue;
                            }
                            Err(rejected_entry) => {
                                // Batch is full or incompatible - flush it and create a new one
                                let full_batch = self.flush_batch().expect("batch should exist");

                                // Start new builder with the rejected datagram
                                let mut new_builder =
                                    crate::datagram::batch::Builder::new(None, peer_addr);
                                let _ = new_builder.try_push(rejected_entry); // Should always succeed
                                                                              // Note: try_push automatically sets sticky sender_id if needed

                                self.current_builder = Some(new_builder);

                                // Return the full batch
                                return Poll::Ready(Some(full_batch));
                            }
                        }
                    } else {
                        // No current builder - start a new one
                        let mut new_builder = crate::datagram::batch::Builder::new(None, peer_addr);
                        let _ = new_builder.try_push(entry);
                        // Note: try_push automatically sets sticky sender_id if needed

                        self.current_builder = Some(new_builder);
                        // Continue to process more packets
                        continue;
                    }
                }
                Poll::Ready(None) => {
                    // Input stream ended - flush any remaining batch
                    if let Some(batch) = self.flush_batch() {
                        return Poll::Ready(Some(batch));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => {
                    // No more entries available right now
                    // Flush the current batch if we have one
                    if let Some(batch) = self.flush_batch() {
                        return Poll::Ready(Some(batch));
                    }
                    return Poll::Pending;
                }
            }
        }

        // Processed max iterations - flush current batch and wake up for more
        if let Some(batch) = self.flush_batch() {
            cx.waker().wake_by_ref();
            return Poll::Ready(Some(batch));
        }

        cx.waker().wake_by_ref();
        Poll::Pending
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── SocketReceiver adapter ─────────────────────────────────────────────────

/// Receives packets from a socket and yields Segments (queue of filled descriptors).
///
/// Allocates unfilled descriptors from a pool, fills them from the socket,
/// and yields the resulting Segments queue or errors. Errors are returned to
/// the caller for handling.
pub struct SocketReceiver<S> {
    socket: S,
    alloc: crate::socket::pool::Pool,
    pending: Option<descriptor::Unfilled>,
}

impl<S> SocketReceiver<S> {
    pub fn new(socket: S, alloc: crate::socket::pool::Pool) -> Self {
        Self {
            socket,
            alloc,
            pending: None,
        }
    }
}

impl<S> Receiver<io::Result<descriptor::Segments>> for SocketReceiver<S>
where
    S: crate::socket::recv::Socket,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<io::Result<descriptor::Segments>>> {
        use std::io;

        let unfilled = self.pending.take().or_else(|| self.alloc.alloc());

        let Some(unfilled) = unfilled else {
            // Allocator exhausted
            tracing::warn!("packet allocator exhausted on recv path");
            cx.waker().wake_by_ref();
            return Poll::Pending;
        };

        let res = unfilled.fill_with(|addr, cmsg, buffer| {
            match self.socket.poll_recv(cx, addr, cmsg, &mut [buffer]) {
                Poll::Pending => Err(io::ErrorKind::WouldBlock.into()),
                Poll::Ready(Ok(len)) => Ok(len),
                Poll::Ready(Err(err)) => Err(err),
            }
        });

        match res {
            Ok(segments) => Poll::Ready(Some(Ok(segments))),
            Err((desc, err)) => {
                // Put the unfilled segment back for retry
                self.pending = Some(desc);

                let kind = err.kind();

                // If we got blocked, yield the future
                if kind == io::ErrorKind::WouldBlock {
                    return Poll::Pending;
                }

                // Return the error to the caller
                Poll::Ready(Some(Err(err)))
            }
        }
    }

    fn on_consumed(&mut self, _bytes: u64) {
        // Not used for recv side
    }
}

// ── FlattenSegments adapter ────────────────────────────────────────────────

/// Wraps a `Receiver<Segments>` and implements `Receiver<Filled>`.
///
/// When `recv` is called, it first drains any buffered segments from the
/// current iterator. Once the iterator is exhausted, it pulls the next Segments
/// from the inner receiver and converts it to an iterator.
pub struct FlattenSegments<R> {
    inner: R,
    iter: descriptor::SegmentsIter,
}

impl<R> FlattenSegments<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            iter: descriptor::SegmentsIter::empty(),
        }
    }
}

impl<R> Receiver<descriptor::Filled> for FlattenSegments<R>
where
    R: Receiver<descriptor::Segments>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<descriptor::Filled>> {
        // Drain any buffered segments first
        if let Some(segment) = self.iter.next() {
            // Self-wake to drain more segments
            cx.waker().wake_by_ref();
            return Poll::Ready(Some(segment));
        }

        // Try to pull the next Segments from the inner receiver
        match ready!(self.inner.poll_recv(cx)) {
            Some(segments) => {
                self.iter = segments.into_iter();
                // Self-wake to process the new segments
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            None => Poll::Ready(None),
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── RouterAdapter ──────────────────────────────────────────────────────────

/// Wraps a receiver and routes segments to a Router.
///
/// Receives filled descriptors and dispatches them to the router's `on_segment` method.
/// Returns `()` for each segment processed, allowing it to be drained via `ReceiverExt::drain`.
pub struct RouterAdapter<R, Router> {
    inner: R,
    router: Router,
}

impl<R, Router> RouterAdapter<R, Router> {
    pub fn new(inner: R, router: Router) -> Self {
        Self { inner, router }
    }
}

impl<R, Router> Receiver<()> for RouterAdapter<R, Router>
where
    R: Receiver<descriptor::Filled>,
    Router: crate::socket::recv::router::Router,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<()>> {
        // Check if router is still open
        if !self.router.is_open() {
            return Poll::Ready(None);
        }

        // Get next segment
        let segment = ready!(self.inner.poll_recv(cx));

        match segment {
            Some(segment) => {
                let bytes = segment.len() as u64;
                self.router.on_segment(segment);
                // Notify upstream that we consumed this segment
                self.inner.on_consumed(bytes);
                Poll::Ready(Some(()))
            }
            None => Poll::Ready(None),
        }
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}

// ── Reporter adapter ───────────────────────────────────────────────────────

/// Wraps a receiver and periodically logs throughput based on `on_consumed` feedback.
///
/// Uses a precision `Clock` to get wall-clock time for rate calculation.
pub struct Reporter<R, Clk> {
    inner: R,
    clock: Clk,
    last_emit: crate::clock::precision::Timestamp,
    next_emit: crate::clock::precision::Timestamp,
    sent: u64,
    enabled: bool,
}

impl<R, Clk> Reporter<R, Clk>
where
    Clk: crate::clock::precision::Clock,
{
    pub fn new(inner: R, clock: Clk, enabled: bool) -> Self {
        let now = clock.now();
        Self {
            inner,
            clock,
            last_emit: now,
            next_emit: now + std::time::Duration::from_secs(1),
            sent: 0,
            enabled,
        }
    }

    fn on_send(&mut self, len: u64) {
        if !self.enabled {
            return;
        }

        self.sent += len;

        let now = self.clock.now();
        if now < self.next_emit {
            return;
        }
        if self.sent > 0 {
            let elapsed_nanos = now.nanos_since(self.last_emit) as f64;
            let elapsed = elapsed_nanos / 1_000_000_000.0;
            let mut rate = self.sent as f64 * 8.0 / elapsed;
            let prefixes = [("G", 1e9), ("M", 1e6), ("K", 1e3)];
            let mut prefix = "";
            for (pref, divisor) in prefixes {
                if rate > divisor {
                    rate /= divisor;
                    prefix = pref;
                    break;
                }
            }
            tracing::info!("{now}: {rate:.2} {prefix}bps");
        }
        self.last_emit = now;
        self.next_emit = now + std::time::Duration::from_secs(1);
        self.sent = 0;
    }
}

impl<T, R, Clk> Receiver<T> for Reporter<R, Clk>
where
    R: Receiver<T>,
    Clk: crate::clock::precision::Clock,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        self.inner.poll_recv(cx)
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.on_send(bytes);
        self.inner.on_consumed(bytes);
    }
}

// ── Path Context ───────────────────────────────────────────────────────────

/// Path context containing all state needed for encoding and tracking packets
///
/// Resolved from the PathSecretEntry and includes:
/// - Sealer for encrypting packets
/// - Credentials for this path
/// - CCA state for congestion control
/// - PacketNumberMap for tracking sent packets
/// - Next packet number counter (per-path)
/// PTO (Probe Timeout) state for tail loss recovery
pub struct Pto {
    /// PTO backoff exponent (starts at INITIAL_PTO_BACKOFF)
    pub backoff: u32,
    /// Target time for the next PTO probe (None = not scheduled)
    pub target_time: Option<crate::clock::precision::Timestamp>,
    /// Time when the last ack-eliciting packet was sent
    pub last_sent_time: Option<crate::clock::precision::Timestamp>,
    /// Set to true when the PTO timer needs to be recalculated and updated
    pub needs_update: bool,
    /// Intrusive list links for PTO wheel
    pub links: crate::intrusive_queue::Links,
}

impl Default for Pto {
    fn default() -> Self {
        Self {
            backoff: s2n_quic_core::path::INITIAL_PTO_BACKOFF,
            target_time: None,
            last_sent_time: None,
            needs_update: false,
            links: crate::intrusive_queue::Links::new(),
        }
    }
}

impl Pto {
    /// Returns true if we have inflight packets that need PTO protection
    pub fn has_inflight_packets(
        packet_map: &s2n_quic_core::packet::number::Map<
            crate::intrusive_queue::Entry<crate::packet::datagram::partial::PartialDatagram>,
        >,
    ) -> bool {
        !packet_map.is_empty()
    }

    /// Called when a new ack-eliciting packet is sent
    pub fn on_packet_sent(&mut self, now: crate::clock::precision::Timestamp) {
        // Track the last send time for PTO calculation
        self.last_sent_time = Some(now);
        // Mark that we need to update the PTO timer (new tail packet)
        self.needs_update = true;
    }

    /// Called when we receive an ACK
    pub fn on_ack_received(&mut self, has_remaining_inflight: bool) {
        // Reset backoff on forward progress
        self.backoff = s2n_quic_core::path::INITIAL_PTO_BACKOFF;

        if has_remaining_inflight {
            // We still have packets in flight, mark for update
            self.needs_update = true;
        } else {
            // No more packets in flight, cancel PTO
            self.target_time = None;
            self.needs_update = false;
        }
    }

    /// Called when the PTO timer fires. Returns true if we should send a probe.
    ///
    /// The caller must recompute target_time via update_target() before reinserting.
    pub fn on_timeout(&mut self, has_inflight: bool) -> bool {
        // Clear the target time - caller will recompute before reinserting
        self.target_time = None;

        // If we need an update, just reschedule without sending probe
        if self.needs_update {
            self.needs_update = false;
            return false;
        }

        // This is an actual PTO timeout - send probe and increase backoff
        self.backoff = self.backoff.saturating_mul(2).min(16); // Cap at 16
        true
    }

    /// Calculate and set the target time for the next PTO
    ///
    /// Call this right before inserting into the wheel.
    /// Uses last_sent_time as the base, falling back to clock.now() if no packets sent yet.
    pub fn update_target<Clk: crate::clock::precision::Clock + ?Sized>(
        &mut self,
        clock: &Clk,
        rtt_estimator: &s2n_quic_core::recovery::RttEstimator,
    ) {
        use s2n_quic_core::packet::number::PacketNumberSpace;
        let mut pto_period = rtt_estimator.pto_period(self.backoff, PacketNumberSpace::Initial);

        // Minimum 2ms to avoid premature triggers due to timestamp rounding
        pto_period = pto_period.max(core::time::Duration::from_millis(2));

        // Base the timeout on when the last packet was sent
        // Only read clock if we don't have a last_sent_time
        let base_time = self.last_sent_time.unwrap_or_else(|| clock.now());
        self.target_time = Some(base_time + pto_period);
    }
}

/// Adapter for using `Rc<RefCell<PathContext<S>>>` in the PTO timing wheel
///
/// This adapter allows the wheel to work directly with Rc pointers, avoiding
/// any additional allocations. The links are stored in `PathContext.pto.links`.
pub struct PtoAdapter<S>(core::marker::PhantomData<S>);

impl<S> crate::intrusive_queue::Adapter for PtoAdapter<S> {
    type Value = std::cell::RefCell<PathContext<S>>;
    type Target = std::cell::RefCell<PathContext<S>>;
    type Pointer = std::rc::Rc<std::cell::RefCell<PathContext<S>>>;

    unsafe fn links(value: *mut Self::Value) -> *mut crate::intrusive_queue::Links {
        core::ptr::addr_of_mut!((*value).borrow_mut().pto.links)
    }

    unsafe fn target(value: *mut Self::Value) -> *mut Self::Target {
        value
    }

    fn as_ptr(ptr: &Self::Pointer) -> *const Self::Value {
        &**ptr
    }

    fn into_raw(ptr: Self::Pointer) -> *mut Self::Value {
        std::rc::Rc::into_raw(ptr) as *mut Self::Value
    }

    unsafe fn from_raw(ptr: *mut Self::Value) -> Self::Pointer {
        std::rc::Rc::from_raw(ptr)
    }
}

impl<S> crate::clock::wheel::WheelAdapter for PtoAdapter<S> {
    unsafe fn target_time(value: *const Self::Value) -> Option<crate::clock::precision::Timestamp> {
        (*value).borrow().pto.target_time
    }

    unsafe fn set_target_time(value: *mut Self::Value, time: crate::clock::precision::Timestamp) {
        (*value).borrow_mut().pto.target_time = Some(time);
    }
}

#[repr(C)]
pub struct PathContext<Sealer> {
    /// The path secret entry
    pub path_secret_entry: Arc<crate::path::secret::map::Entry>,
    /// Sealer for encrypting packets
    pub sealer: Sealer,
    /// Credentials for this path
    pub credentials: crate::credentials::Credentials,
    /// Next packet number to assign (per-path counter)
    pub next_packet_number: VarInt,
    /// Next attempt ID for FlowInit/FlowInitValidate packets (per-sender counter)
    ///
    /// This is a monotonically increasing identifier used for server-side deduplication
    /// of flow initialization attempts. Scoped to (credentials, source_sender_id).
    pub flow_attempt_id_counter: VarInt,
    /// Congestion controller for this path
    pub cca: crate::congestion::Controller,
    /// RTT estimator for this path
    pub rtt_estimator: s2n_quic_core::recovery::RttEstimator,
    /// Packet number map for tracking sent packets
    pub packet_number_map: s2n_quic_core::packet::number::Map<
        crate::intrusive_queue::Entry<crate::packet::datagram::partial::PartialDatagram>,
    >,
    /// PTO state for tail loss recovery
    pub pto: Pto,
    /// Number of pending batches in the pipeline for this path
    ///
    /// Incremented when a batch starts being processed, decremented when finished.
    /// Used to determine if CCA should signal "has_more_app_data".
    pub pending_batches: u32,
}

/// Trait for resolving PathContext from PathSecretEntry
pub trait PathContextResolver {
    type Sealer: crate::crypto::seal::Application;

    /// Resolve path context for the given PathSecretEntry
    fn resolve(
        &self,
        entry: &Arc<crate::path::secret::map::Entry>,
    ) -> Option<Rc<RefCell<PathContext<Self::Sealer>>>>;
}

// ── Path Resolution ────────────────────────────────────────────────────────

/// Resolves PathContext for each batch based on its PathSecretEntry.
///
/// Returns PathBatch items. Batches that fail to resolve are sent to the error
/// channel with packet status marked as failed.
pub struct PathResolver<R, Resolver, ErrorSender> {
    inner: R,
    resolver: Resolver,
    error_sender: ErrorSender,
}

impl<R, Resolver, ErrorSender> PathResolver<R, Resolver, ErrorSender> {
    pub fn new(inner: R, resolver: Resolver, error_sender: ErrorSender) -> Self {
        Self {
            inner,
            resolver,
            error_sender,
        }
    }
}

impl<R, Resolver, ErrorSender>
    Receiver<
        crate::intrusive_queue::Entry<
            crate::datagram::batch::Batch<Rc<RefCell<PathContext<Resolver::Sealer>>>>,
        >,
    > for PathResolver<R, Resolver, ErrorSender>
where
    R: Receiver<crate::intrusive_queue::Entry<crate::datagram::batch::Batch>>,
    Resolver: PathContextResolver,
    ErrorSender: UnboundedSender<crate::intrusive_queue::Entry<crate::datagram::batch::Batch>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<
        Option<
            crate::intrusive_queue::Entry<
                crate::datagram::batch::Batch<Rc<RefCell<PathContext<Resolver::Sealer>>>>,
            >,
        >,
    > {
        let Some(mut batch) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        let Some(front) = batch.datagrams.front() else {
            // Empty batch - just skip it
            debug_assert!(false, "empty batch submitted");
            cx.waker().wake_by_ref();
            return Poll::Pending;
        };

        // Resolve path context
        let Some(context) = self.resolver.resolve(&front.path_secret_entry) else {
            // Reset all of the batch datagrams as failed
            for dgram in batch.datagrams.iter_mut() {
                dgram.status = TransmissionStatus::Failed(FailureReason::UnknownPathSecret);
            }

            let _ = self.error_sender.send(batch);

            cx.waker().wake_by_ref();
            return Poll::Pending;
        };

        // Increment pending_batches counter
        context.borrow_mut().pending_batches += 1;

        // Attach the context to the batch, making it !Send
        let batch = batch.with_context(context);

        Poll::Ready(Some(batch))
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Packet Encoding ────────────────────────────────────────────────────────

/// Encodes batches of PartialDatagrams into wire-format packets.
///
/// Takes batches with assigned packet numbers, uses the sealer from PathContext,
/// allocates descriptors from the pool, and encodes each transmission. The encoded
/// bytes are stored in Batch.encoded.
pub struct Encoder<R> {
    inner: R,
    pool: crate::socket::pool::Pool,
    source_control_port: u16,
    source_sender_id: VarInt,
}

impl<R> Encoder<R> {
    pub fn new(
        inner: R,
        pool: crate::socket::pool::Pool,
        source_control_port: u16,
        source_sender_id: VarInt,
    ) -> Self {
        Self {
            inner,
            pool,
            source_control_port,
            source_sender_id,
        }
    }
}

impl<R, Sealer>
    Receiver<
        crate::intrusive_queue::Entry<
            crate::datagram::batch::Batch<Rc<RefCell<PathContext<Sealer>>>>,
        >,
    > for Encoder<R>
where
    R: Receiver<
        crate::intrusive_queue::Entry<
            crate::datagram::batch::Batch<Rc<RefCell<PathContext<Sealer>>>>,
        >,
    >,
    Sealer: crate::crypto::seal::Application,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<
        Option<
            crate::intrusive_queue::Entry<
                crate::datagram::batch::Batch<Rc<RefCell<PathContext<Sealer>>>>,
            >,
        >,
    > {
        let Some(mut batch) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        let Some(unfilled) = self.pool.alloc() else {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        };

        batch.encode(unfilled, self.source_control_port, self.source_sender_id);

        Poll::Ready(Some(batch))
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Packet Registration ────────────────────────────────────────────────────

/// Registers packets in the packet number map and notifies the CCA.
///
/// Takes PathBatch items after encoding, decomposes the batch into individual
/// datagrams, registers each in the packet number map, notifies the CCA of
/// the sent packet, and returns just the Batch (consuming the PathContext).
pub struct PacketRegistrar<R, Clk, Sealer> {
    inner: R,
    clock: Clk,
    _phantom: PhantomData<Sealer>,
}

impl<R, Clk, Sealer> PacketRegistrar<R, Clk, Sealer> {
    pub fn new(inner: R, clock: Clk) -> Self {
        Self {
            inner,
            clock,
            _phantom: PhantomData,
        }
    }
}

impl<R, Clk, Sealer>
    Receiver<
        crate::intrusive_queue::Entry<
            crate::datagram::batch::Batch<Rc<RefCell<PathContext<Sealer>>>>,
        >,
    > for PacketRegistrar<R, Clk, Sealer>
where
    R: Receiver<
        crate::intrusive_queue::Entry<
            crate::datagram::batch::Batch<Rc<RefCell<PathContext<Sealer>>>>,
        >,
    >,
    Clk: crate::clock::precision::Clock,
    Sealer: crate::crypto::seal::Application,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<
        Option<
            crate::intrusive_queue::Entry<
                crate::datagram::batch::Batch<Rc<RefCell<PathContext<Sealer>>>>,
            >,
        >,
    > {
        let Some(mut batch_entry) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        // Deref Entry to Batch to enable disjoint borrows
        let batch = &mut *batch_entry;

        // TODO: Move CCA notification to BEFORE transmission, not after
        // The CCA should be consulted before sending to:
        // 1. Check CWND - don't send if we'd exceed the congestion window
        // 2. Get next_transmission_time for pacing
        // 3. Schedule packets into a timing wheel based on pacing delays
        // 4. Implement backpressure to avoid scheduling too far into the future
        //
        // Current implementation notifies CCA after encoding but before actual socket send,
        // which doesn't respect CWND limits or proper pacing delays.

        // Get current time for CCA and PTO
        let now = self.clock.now();

        // Store a reference to context (can now borrow disjoint fields)
        let context = &batch.context;

        // Check if there's more app data based on pending batch count
        let has_more_app_data = context.borrow().pending_batches > 1;

        // Get starting packet number from batch metadata
        let Some(starting_pn) = batch.meta.starting_packet_number else {
            unsafe {
                assume!(false, "starting packet number required for this phase");
            }
        };

        // Get segment sizes without consuming the encoded field (we need it for sending)
        let Some(segments) = batch.encoded.as_ref() else {
            unsafe {
                assume!(false, "encoded field required for this phase");
            }
        };
        let segment_sizes = segments.sizes();

        {
            // Borrow context to register packets (disjoint from batch.datagrams)
            let mut ctx = context.borrow_mut();
            let ctx = &mut *ctx;
            let mut packet_number = starting_pn;

            // Iterate over segment sizes and datagrams together
            for (packet_size, mut datagram_entry) in
                segment_sizes.into_iter().zip(batch.datagrams.drain())
            {
                // TODO store control packets once we start ACKing them
                if matches!(
                    datagram_entry.packet_type,
                    crate::packet::datagram::partial::PacketType::Control { .. }
                ) {
                    continue;
                }

                // Notify CCA about the sent packet
                let cc_info = ctx.cca.on_packet_sent(
                    now.into(),
                    packet_size,
                    has_more_app_data,
                    &ctx.rtt_estimator,
                );

                // Store packet_info on the PartialDatagram
                let transmission_info = TransmissionInfo {
                    cc_info,
                    time_sent: now.into(),
                    sent_bytes: packet_size,
                };
                datagram_entry.transmission_info = Some(transmission_info);

                // For control packets, clear the payload to save memory
                // Control packets (like ACKs) are ephemeral and don't need retransmission
                if let crate::packet::datagram::partial::PacketType::Control {
                    routing_info: _,
                    control_data,
                } = &mut datagram_entry.packet_type
                {
                    control_data.clear();
                }

                // Convert packet number to PacketNumber type and register in map
                let pn = s2n_quic_core::packet::number::PacketNumberSpace::Initial
                    .new_packet_number(packet_number);
                ctx.packet_number_map.insert(pn, datagram_entry);

                packet_number += VarInt::from_u8(1);
            }

            // Mark PTO for update after sending packets (new tail)
            ctx.pto.on_packet_sent(now);

            // Decrement pending_batches counter
            ctx.pending_batches -= 1;
        }

        Poll::Ready(Some(batch_entry))
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── PTO Wheel Injector ─────────────────────────────────────────────────────

/// Adapter that injects PathContext into PTO wheel after packet registration
pub struct PtoWheelInjector<R, Sealer, PtoTx> {
    inner: R,
    pto_wheel_tx: PtoTx,
    _phantom: PhantomData<Sealer>,
}

impl<R, Sealer, PtoTx> PtoWheelInjector<R, Sealer, PtoTx> {
    pub fn new(inner: R, pto_wheel_tx: PtoTx) -> Self {
        Self {
            inner,
            pto_wheel_tx,
            _phantom: PhantomData,
        }
    }
}

impl<R, Sealer, PtoTx> Receiver<crate::intrusive_queue::Entry<crate::datagram::batch::Batch>>
    for PtoWheelInjector<R, Sealer, PtoTx>
where
    R: Receiver<
        crate::intrusive_queue::Entry<
            crate::datagram::batch::Batch<Rc<RefCell<PathContext<Sealer>>>>,
        >,
    >,
    Sealer: crate::crypto::seal::Application,
    PtoTx: UnboundedSender<std::rc::Rc<std::cell::RefCell<PathContext<Sealer>>>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<crate::intrusive_queue::Entry<crate::datagram::batch::Batch>>> {
        let Some(batch_with_context) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        // Split the batch and context
        let (batch, context) = batch_with_context.into_parts();

        // Check if context needs to be inserted into PTO wheel
        // Only insert if:
        // 1. PTO needs update (packets were sent), AND
        // 2. Context is not already linked in the wheel
        {
            let ctx = context.borrow();
            if ctx.pto.needs_update && !ctx.pto.links.is_linked() {
                drop(ctx);
                let _ = self.pto_wheel_tx.send(context.clone());
            }
        }

        Poll::Ready(Some(batch))
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Timing adapter ─────────────────────────────────────────────────────────

/// Wraps a receiver and measures time spent in poll_recv
pub struct Timing<T, R> {
    inner: R,
    label: &'static str,
    _phantom: PhantomData<T>,
}

impl<T, R> Timing<T, R> {
    pub fn new(inner: R, label: &'static str) -> Self {
        Self {
            inner,
            label,
            _phantom: PhantomData,
        }
    }
}

impl<T, R> Receiver<T> for Timing<T, R>
where
    R: Receiver<T>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        // let start = std::time::Instant::now();
        let result = self.inner.poll_recv(cx);
        // let elapsed = start.elapsed();

        // if elapsed.as_millis() > 1 {
        // tracing::warn!(label = self.label, ?elapsed, "slow poll_recv detected");
        // }

        result
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Dbg adapter ────────────────────────────────────────────────────────────

/// Wraps a receiver and logs debug information about received values.
///
/// Prints the type name and label when values are received, useful for
/// debugging channel pipelines.
pub struct Dbg<T, R> {
    inner: R,
    label: &'static str,
    _phantom: PhantomData<T>,
}

impl<T, R> Dbg<T, R> {
    pub fn new(inner: R, label: &'static str) -> Self {
        Self {
            inner,
            label,
            _phantom: PhantomData,
        }
    }
}

impl<T, R> Receiver<T> for Dbg<T, R>
where
    R: Receiver<T>,
    T: fmt::Debug,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<T>> {
        match self.inner.poll_recv(cx) {
            Poll::Ready(Some(value)) => {
                tracing::trace!(
                    label = self.label,
                    // value = ?value,
                    "recv Ready(Some)",
                );
                Poll::Ready(Some(value))
            }
            Poll::Ready(None) => {
                tracing::trace!(label = self.label, "recv Ready(None)",);
                Poll::Ready(None)
            }
            Poll::Pending => {
                tracing::trace!(label = self.label, "recv Pending",);
                Poll::Pending
            }
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        tracing::trace!(label = self.label, bytes, "on_consumed",);
        self.inner.on_consumed(bytes);
    }
}
