// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Channel traits and combinators for the receiver pipeline.
//!
//! # Budget-Based Yielding
//!
//! Every `poll_recv` call takes a `&mut Budget`. The budget tracks how many items
//! the pipeline may process in one poll cycle. Combinators MUST NOT call
//! `cx.waker().wake_by_ref()` — only the top-level `drain_budgeted` does that.
//!
//! When budget is exhausted:
//! - Leaf receivers return `Poll::Pending` after calling `budget.set_needs_wake()`
//! - Flatten-style adapters stop yielding buffered items
//! - Filter/batch adapters stop pulling from inner
//!
//! The `drain_budgeted` future resets budget each poll, loops until Pending, then
//! checks `budget.take_needs_wake()` to issue a single self-wake if more work exists.
use crate::{socket::pool::descriptor, time::precision, tracing::*};
use core::task::{self, Poll};
use s2n_quic_core::ready;
use std::{future::Future, io, marker::PhantomData, mem::MaybeUninit, time::Instant};

pub mod cell;
pub mod intrusive;

#[cfg(test)]
mod tests;

// ── Budget ────────────────────────────────────────────────────────────────

/// Tracks remaining work budget for a single poll cycle.
///
/// Threaded through the receiver pipeline from the top-level drain. Replaces
/// per-combinator self-wakes with a centralized "needs_wake" signal.
pub struct Budget {
    capacity: usize,
    remaining: usize,
    generation: usize,
    needs_wake: bool,
}

impl Budget {
    #[inline]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            remaining: capacity,
            generation: 0,
            needs_wake: false,
        }
    }

    /// Consume one unit of budget. Returns `true` if budget was available.
    #[inline]
    pub fn consume(&mut self) -> bool {
        if self.remaining > 0 {
            self.remaining -= 1;
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn is_exhausted(&self) -> bool {
        self.remaining == 0
    }

    /// Signal that there is more work available. The top-level drain will
    /// issue a self-wake when it sees this flag.
    #[inline]
    pub fn set_needs_wake(&mut self) {
        self.needs_wake = true;
    }

    #[inline]
    pub fn take_needs_wake(&mut self) -> bool {
        core::mem::replace(&mut self.needs_wake, false)
    }

    /// Reset budget for the next poll cycle. Increments generation.
    #[inline]
    pub fn reset(&mut self) {
        self.remaining = self.capacity;
        self.needs_wake = false;
        self.generation = self.generation.wrapping_add(1);
    }

    #[inline]
    pub fn consumed(&self) -> usize {
        self.capacity - self.remaining
    }

    #[inline]
    pub fn generation(&self) -> usize {
        self.generation
    }
}

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

// ── EntryBoxSender ─────────────────────────────────────────────────────────

/// Adapts an `UnboundedSender<Entry<T>>` into an `UnboundedSender<T>` by boxing
/// values into [`crate::intrusive_queue::Entry`] on each send.
///
/// This is the canonical way to bridge between a "plain value" sender interface and a
/// channel that requires intrusive-queue entries without duplicating boxing logic at
/// every call site.
pub struct EntryBoxSender<T, S> {
    inner: S,
    // `fn() -> T` phantom avoids propagating Send/Sync from T onto the struct — the struct
    // does not own any T values, so auto-trait derivation should come from S alone.
    _phantom: PhantomData<fn() -> T>,
}

impl<T, S> Clone for EntryBoxSender<T, S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T, S> EntryBoxSender<T, S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            _phantom: PhantomData,
        }
    }
}

impl<T, S> UnboundedSender<T> for EntryBoxSender<T, S>
where
    S: UnboundedSender<crate::intrusive::Entry<T>>,
{
    #[inline]
    fn send(&mut self, value: T) -> Result<(), T> {
        self.inner
            .send(crate::intrusive::Entry::new(value))
            .map_err(|e| e.into_inner())
    }
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
}

/// An async-capable channel receiver.
pub trait Receiver<T> {
    /// Poll for the next value with budget tracking.
    ///
    /// Returns `Ready(Some(value))` when a value is available,
    /// `Pending` when empty (or budget exhausted) but not closed,
    /// `Ready(None)` when the channel is closed.
    ///
    /// Implementations MUST NOT call `cx.waker().wake_by_ref()`. When budget
    /// is exhausted and more work is available, call `budget.set_needs_wake()`
    /// and return `Pending`. The top-level drain handles the single self-wake.
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<T>>;

    /// Receives the next value. Returns `None` when the channel is closed.
    fn recv<'a>(
        &'a mut self,
        budget: &'a mut Budget,
    ) -> impl core::future::Future<Output = Option<T>> + 'a {
        core::future::poll_fn(move |cx| self.poll_recv(cx, budget))
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

    /// Drain the receiver, processing up to `capacity` items per poll before yielding.
    /// `None` means process one item per poll.
    fn drain_budgeted(mut self, capacity: Option<usize>) -> impl core::future::Future<Output = ()>
    where
        Self: Receiver<()>,
    {
        let cap = capacity.unwrap_or(1);
        let mut budget = Budget::new(cap);
        core::future::poll_fn(move |cx| {
            budget.reset();
            loop {
                match self.poll_recv(cx, &mut budget) {
                    Poll::Pending => break,
                    Poll::Ready(None) => return Poll::Ready(()),
                    Poll::Ready(Some(())) => {
                        if budget.is_exhausted() {
                            budget.set_needs_wake();
                            break;
                        }
                        continue;
                    }
                }
            }
            if budget.take_needs_wake() {
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        })
    }

    /// Like [`drain_budgeted`](Self::drain_budgeted), but records per-poll metrics:
    /// items consumed, wall-clock duration, and the latency until the next poll.
    fn drain_budgeted_metered(
        mut self,
        capacity: Option<usize>,
        task_counter: crate::counter::Task,
    ) -> impl core::future::Future<Output = ()>
    where
        Self: Receiver<()>,
    {
        let cap = capacity.unwrap_or(1);
        let mut budget = Budget::new(cap);
        let mut prev_poll_end = None::<Instant>;
        core::future::poll_fn(move |cx| {
            budget.reset();
            let guard = task_counter.time.start();
            let mut output = Poll::Pending;
            loop {
                match self.poll_recv(cx, &mut budget) {
                    Poll::Pending => break,
                    Poll::Ready(None) => {
                        task_counter.drained.record_value(budget.consumed() as u64);
                        output = Poll::Ready(());
                        break;
                    }
                    Poll::Ready(Some(())) => {
                        if budget.is_exhausted() {
                            budget.set_needs_wake();
                            break;
                        }
                        continue;
                    }
                }
            }
            if output.is_pending() {
                task_counter.drained.record_value(budget.consumed() as u64);
                if budget.take_needs_wake() {
                    cx.waker().wake_by_ref();
                }
            }
            if let Some(guard) = guard {
                let now = guard.record();
                if let Some(prev_end) = prev_poll_end {
                    task_counter
                        .next_poll_latency
                        .record(now.duration_since(prev_end));
                }
                prev_poll_end = Some(now);
            }
            output
        })
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
    current_idx: usize,
    last_generation: usize,
}

impl<R> Priority<R> {
    pub fn new(receivers: Vec<R>) -> Self {
        Self {
            receivers,
            last_recv_idx: None,
            current_idx: 0,
            last_generation: usize::MAX,
        }
    }
}

impl<T, R: Receiver<T>> Receiver<T> for Priority<R> {
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<T>> {
        let len = self.receivers.len();
        if len == 0 {
            return Poll::Ready(None);
        }

        // Reset to highest priority at the start of each new generation
        if budget.generation() != self.last_generation {
            self.current_idx = 0;
            self.last_generation = budget.generation();
        }

        let mut all_closed = true;
        for i in 0..len {
            let idx = (self.current_idx + i) % len;
            match self.receivers[idx].poll_recv(cx, budget) {
                Poll::Ready(Some(value)) => {
                    self.last_recv_idx = Some(idx);
                    self.current_idx = (idx + 1) % len;
                    return Poll::Ready(Some(value));
                }
                Poll::Pending => {
                    all_closed = false;
                }
                Poll::Ready(None) => {}
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
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<Item>> {
        loop {
            // Drain any buffered entries first
            if let Some(iter) = &mut self.iter {
                if budget.is_exhausted() {
                    budget.set_needs_wake();
                    return Poll::Pending;
                }
                if let Some(item) = iter.next() {
                    // Subsequent items from buffer consume budget
                    budget.consume();
                    return Poll::Ready(Some(item));
                }
                // Iterator exhausted
                self.iter = None;
            }

            // Pull next container from inner (inner consumes budget for the acquisition)
            match self.inner.poll_recv(cx, budget) {
                Poll::Ready(Some(container)) => {
                    let mut iter = container.into_iter();
                    // First item is free — budget was consumed by inner
                    if let Some(item) = iter.next() {
                        self.iter = Some(iter);
                        return Poll::Ready(Some(item));
                    }
                    // Empty container, loop back
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

/// Wraps a `Receiver<Queue<T>>` and implements `Receiver<Entry<T>>`.
///
/// Specialized version of `Flatten` for the common case of intrusive queues.
/// This avoids type inference issues with the generic `Flatten`.
pub struct FlattenQueue<T, R> {
    inner: R,
    queue: crate::intrusive::Queue<T>,
}

impl<T, R> FlattenQueue<T, R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            queue: Default::default(),
        }
    }
}

impl<T, R> Receiver<crate::intrusive::Entry<T>> for FlattenQueue<T, R>
where
    R: Receiver<crate::intrusive::Queue<T>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<crate::intrusive::Entry<T>>> {
        loop {
            // Drain any buffered entries — subsequent items consume budget
            if !self.queue.is_empty() {
                if budget.is_exhausted() {
                    budget.set_needs_wake();
                    return Poll::Pending;
                }
                if let Some(entry) = self.queue.pop_front() {
                    budget.consume();
                    return Poll::Ready(Some(entry));
                }
            }

            // Pull next queue from inner (inner consumes budget for the swap)
            match self.inner.poll_recv(cx, budget) {
                Poll::Ready(Some(queue)) => {
                    self.queue = queue;
                    // First item is free — budget was consumed by inner
                    if let Some(entry) = self.queue.pop_front() {
                        return Poll::Ready(Some(entry));
                    }
                    // Empty queue, loop back
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
    A: crate::intrusive::Adapter,
{
    inner: R,
    list: crate::intrusive::List<A>,
}

impl<A, R> FlattenList<A, R>
where
    A: crate::intrusive::Adapter,
{
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            list: crate::intrusive::List::new(),
        }
    }
}

impl<A, R> Receiver<A::Pointer> for FlattenList<A, R>
where
    A: crate::intrusive::Adapter,
    R: Receiver<crate::intrusive::List<A>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<A::Pointer>> {
        loop {
            // Drain buffered entries — subsequent items consume budget
            if !self.list.is_empty() {
                if budget.is_exhausted() {
                    budget.set_needs_wake();
                    return Poll::Pending;
                }
                if let Some(entry) = self.list.pop_front() {
                    budget.consume();
                    return Poll::Ready(Some(entry));
                }
            }

            // Pull next list from inner (inner consumes budget for the swap)
            match self.inner.poll_recv(cx, budget) {
                Poll::Ready(Some(list)) => {
                    self.list = list;
                    // First item is free — budget was consumed by inner
                    if let Some(entry) = self.list.pop_front() {
                        return Poll::Ready(Some(entry));
                    }
                    // Empty list, loop back
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
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<T>> {
        match self.inner.poll_recv(cx, budget) {
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
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<U>> {
        match self.inner.poll_recv(cx, budget) {
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
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<U>> {
        loop {
            match self.inner.poll_recv(cx, budget) {
                Poll::Ready(Some(value)) => {
                    if let Some(mapped) = (self.filter_map)(value) {
                        return Poll::Ready(Some(mapped));
                    }
                    if budget.is_exhausted() {
                        budget.set_needs_wake();
                        return Poll::Pending;
                    }
                    continue;
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
    Clk: precision::Clock,
{
    inner: R,
    timer: Clk::Timer,
    rate: crate::socket::rate::Rate,
    bucket: crate::socket::rate::TokenBucket,
    buffered: Option<T>,
}

impl<R, Clk, T> Paced<R, Clk, T>
where
    Clk: precision::Clock,
{
    pub fn new(inner: R, clock: Clk, rate: crate::socket::rate::Rate) -> Self {
        use crate::time::precision::Timer;

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
    Clk: precision::Clock,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<T>> {
        use crate::time::precision::Timer;

        // If we have a buffered item from a previous poll, check timer first
        if self.buffered.is_some() {
            ready!(self.timer.poll_ready(cx));
            // Timer is ready, return the buffered item
            return Poll::Ready(self.buffered.take());
        }

        // Try to get next item from inner receiver
        let item = ready!(self.inner.poll_recv(cx, budget));

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
        use crate::time::precision::Timer;

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

/// Transfers items from a receiver to a sender, yielding after `capacity` items
/// are transferred in a single poll. `None` means transfer one item per poll.
pub async fn pump_budgeted<T, R, S>(rx: R, tx: S, capacity: Option<usize>)
where
    R: Receiver<T>,
    S: Sender<T>,
{
    BudgetedPump {
        rx,
        tx,
        value: None,
        budget: Budget::new(capacity.unwrap_or(1)),
    }
    .await;

    struct BudgetedPump<T, R, S> {
        rx: R,
        tx: S,
        value: Option<core::mem::MaybeUninit<T>>,
        budget: Budget,
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
            this.budget.reset();

            loop {
                if this.value.is_none() {
                    match this.rx.poll_recv(cx, &mut this.budget) {
                        Poll::Ready(Some(value)) => {
                            this.value = Some(MaybeUninit::new(value));
                        }
                        Poll::Ready(None) => {
                            return Poll::Ready(());
                        }
                        Poll::Pending => break,
                    }
                }

                if let Some(v) = this.value.as_mut() {
                    match this.tx.poll_send(cx, v) {
                        Poll::Ready(Ok(())) => {
                            this.value = None;
                            if this.budget.is_exhausted() {
                                this.budget.set_needs_wake();
                                break;
                            }
                        }
                        Poll::Ready(Err(())) => return Poll::Ready(()),
                        Poll::Pending => break,
                    }
                }
            }

            if this.budget.take_needs_wake() {
                cx.waker().wake_by_ref();
            }
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
    let mut budget = Budget::new(usize::MAX);

    loop {
        // Receive next item
        let Some(entry) = rx.recv(&mut budget).await else {
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
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<Result<T, (io::Error, T)>>> {
        let Some(mut item) = ready!(self.inner.poll_recv(cx, budget)) else {
            return Poll::Ready(None);
        };

        let result = match item.send(&self.socket) {
            Ok(()) => {
                self.inner.on_consumed(item.byte_cost());
                Ok(item)
            }
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
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<T>> {
        loop {
            let Some(result) = ready!(self.inner.poll_recv(cx, budget)) else {
                return Poll::Ready(None);
            };

            match result {
                Ok(value) => return Poll::Ready(Some(value)),
                Err(err) => {
                    (self.on_error)(err);
                    if budget.is_exhausted() {
                        budget.set_needs_wake();
                        return Poll::Pending;
                    }
                    continue;
                }
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
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<T>> {
        loop {
            let Some(result) = ready!(self.inner.poll_recv(cx, budget)) else {
                return Poll::Ready(None);
            };

            match result {
                Ok(value) => return Poll::Ready(Some(value)),
                Err(_) => {
                    if budget.is_exhausted() {
                        budget.set_needs_wake();
                        return Poll::Pending;
                    }
                    continue;
                }
            }
        }
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
    /// Shared local pool of recycled descriptors (Rc, single-threaded, no locking)
    local_pool: std::rc::Rc<core::cell::RefCell<crate::intrusive::List<descriptor::RecycleAdapter>>>,
    /// Weak sender for fresh allocations — stored in each new descriptor's Header
    recycle_weak: descriptor::WeakRecycleSender,
}

impl<S> SocketReceiver<S> {
    pub fn new(
        socket: S,
        alloc: crate::socket::pool::Pool,
        local_pool: std::rc::Rc<core::cell::RefCell<crate::intrusive::List<descriptor::RecycleAdapter>>>,
        recycle_weak: descriptor::WeakRecycleSender,
    ) -> Self {
        Self {
            socket,
            alloc,
            pending: None,
            local_pool,
            recycle_weak,
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
        budget: &mut Budget,
    ) -> Poll<Option<io::Result<descriptor::Segments>>> {
        use std::io;

        let unfilled = self.pending.take().or_else(|| {
            // Try recycled descriptors first (LIFO for cache locality)
            let mut list = self.local_pool.borrow_mut();
            if let Some(recycled) = list.pop_back() {
                return Some(descriptor::Unfilled::from_recycled(recycled.into_descriptor()));
            }
            drop(list);
            // Fall back to fresh allocation
            self.alloc.alloc_with_recycler(&self.recycle_weak)
        });

        let Some(unfilled) = unfilled else {
            // Allocator exhausted
            warn!("packet allocator exhausted on recv path");
            budget.set_needs_wake();
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
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<descriptor::Filled>> {
        loop {
            // Drain any buffered segments first
            if let Some(segment) = self.iter.next() {
                if budget.is_exhausted() {
                    budget.set_needs_wake();
                    return Poll::Pending;
                }
                // Subsequent items from buffer consume budget
                budget.consume();
                return Poll::Ready(Some(segment));
            }

            // Pull next Segments from inner (inner consumes budget for the acquisition)
            match self.inner.poll_recv(cx, budget) {
                Poll::Ready(Some(segments)) => {
                    self.iter = segments.into_iter();
                    // First item is free — budget was consumed by inner
                    if let Some(segment) = self.iter.next() {
                        return Poll::Ready(Some(segment));
                    }
                    // Empty segments, loop back
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
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<()>> {
        // Check if router is still open
        if !self.router.is_open() {
            return Poll::Ready(None);
        }

        // Get next segment
        let segment = ready!(self.inner.poll_recv(cx, budget));

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
    last_emit: precision::Timestamp,
    next_emit: precision::Timestamp,
    sent: u64,
    enabled: bool,
}

impl<R, Clk> Reporter<R, Clk>
where
    Clk: precision::Clock,
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
            info!("{now}: {rate:.2} {prefix}bps");
        }
        self.last_emit = now;
        self.next_emit = now + std::time::Duration::from_secs(1);
        self.sent = 0;
    }
}

impl<T, R, Clk> Receiver<T> for Reporter<R, Clk>
where
    R: Receiver<T>,
    Clk: precision::Clock,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<T>> {
        self.inner.poll_recv(cx, budget)
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.on_send(bytes);
        self.inner.on_consumed(bytes);
    }
}

// ── BatchLen trait ─────────────────────────────────────────────────────────

/// Returns the number of logical items a channel message represents.
///
/// Implement this trait on types used as channel messages so that
/// [`GaugedSender`] and [`GaugedReceiver`] can correctly update the
/// queue-depth metric for both single-item and batch sends/receives.
///
/// # Examples
///
/// * A single entry (e.g. `Entry<T>`) should return `1`.
/// * A batch container (e.g. `Queue<T>` or `List<A>`) should return the
///   number of entries it contains.
pub trait BatchLen {
    fn batch_len(&self) -> u64;
}

/// Single intrusive-queue entries always count as one item.
impl<T> BatchLen for crate::intrusive::Entry<T> {
    #[inline]
    fn batch_len(&self) -> u64 {
        1
    }
}

/// An intrusive `List<A>` (including the `Queue<T>` type alias) counts as the
/// number of entries it holds.
impl<A: crate::intrusive::Adapter> BatchLen for crate::intrusive::List<A> {
    #[inline]
    fn batch_len(&self) -> u64 {
        self.len() as u64
    }
}

/// A `VecDeque<T>` counts as the number of elements it holds.
impl<T> BatchLen for std::collections::VecDeque<T> {
    #[inline]
    fn batch_len(&self) -> u64 {
        self.len() as u64
    }
}

// ── GaugedSender ───────────────────────────────────────────────────────────

/// Wraps a sender and records enqueue metrics for every successful send.
///
/// Associates a [`crate::counter::QueueGauge`] with the sender. Each time
/// a value is successfully enqueued the gauge is incremented by the item
/// count reported by [`BatchLen::batch_len`], so both single-item and batch
/// sends produce accurate queue-depth readings.
///
/// Works with any type implementing [`UnboundedSender`] or [`Sender`].
pub struct GaugedSender<S, T> {
    inner: S,
    gauge: crate::counter::QueueGauge,
    _phantom: PhantomData<T>,
}

impl<S: Clone, T> Clone for GaugedSender<S, T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            gauge: self.gauge.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<S, T> GaugedSender<S, T> {
    pub fn new(inner: S, gauge: crate::counter::QueueGauge) -> Self {
        Self {
            inner,
            gauge,
            _phantom: PhantomData,
        }
    }
}

impl<T, S> UnboundedSender<T> for GaugedSender<S, T>
where
    T: BatchLen,
    S: UnboundedSender<T>,
{
    #[inline]
    fn send(&mut self, value: T) -> Result<(), T> {
        let count = value.batch_len();
        match self.inner.send(value) {
            Ok(()) => {
                self.gauge.enqueue(count);
                Ok(())
            }
            Err(v) => Err(v),
        }
    }
}

impl<T, S> Sender<T> for GaugedSender<S, T>
where
    T: BatchLen,
    S: Sender<T>,
{
    #[inline]
    fn poll_send(
        &mut self,
        cx: &mut task::Context<'_>,
        value: &mut core::mem::MaybeUninit<T>,
    ) -> Poll<Result<(), ()>> {
        // SAFETY: The `Sender` trait contract requires callers to pass an initialized
        // `MaybeUninit<T>` to `poll_send`.  We read it here only to compute the batch
        // size before forwarding to the inner sender, which consumes/moves the value.
        let count = unsafe { value.assume_init_ref() }.batch_len();
        match self.inner.poll_send(cx, value) {
            Poll::Ready(Ok(())) => {
                self.gauge.enqueue(count);
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

// ── GaugedReceiver ─────────────────────────────────────────────────────────

/// Wraps a receiver and records dequeue metrics for every value received.
///
/// Associates a [`crate::counter::QueueGauge`] with the receiver.  Each time
/// `poll_recv` returns `Ready(Some(value))` the gauge is decremented by the
/// item count reported by [`BatchLen::batch_len`], so both single-item and
/// batch receives produce accurate queue-depth readings.
///
/// Pair with a [`GaugedSender`] that shares the same `QueueGauge` to get
/// a live view of channel build-up from both ends.
pub struct GaugedReceiver<R, T> {
    inner: R,
    gauge: crate::counter::QueueGauge,
    _phantom: PhantomData<T>,
}

impl<R, T> GaugedReceiver<R, T> {
    pub fn new(inner: R, gauge: crate::counter::QueueGauge) -> Self {
        Self {
            inner,
            gauge,
            _phantom: PhantomData,
        }
    }
}

impl<T, R> Receiver<T> for GaugedReceiver<R, T>
where
    T: BatchLen,
    R: Receiver<T>,
{
    #[inline]
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<T>> {
        match self.inner.poll_recv(cx, budget) {
            Poll::Ready(Some(value)) => {
                self.gauge.dequeue_n(value.batch_len());
                Poll::Ready(Some(value))
            }
            other => other,
        }
    }

    #[inline]
    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── PrioritySelect ───────────────────────────────────────────────────────

/// Indicates how many high-priority items remain after the current one was
/// consumed. Returned alongside each item by [`PrioritySelect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImmediateQueueStatus {
    /// At least one more item is queued in the priority receiver.
    HasMore,
    /// The priority receiver is empty (or closed) — this was the last item.
    Empty,
    /// The caller's budget was exhausted before we could peek; treat
    /// conservatively as if there are more items waiting.
    BudgetExhausted,
}

/// Polls a high-priority receiver first; falls back to a low-priority receiver
/// only when the priority receiver is pending. Closes immediately when the
/// priority receiver closes (returns `Ready(None)`).
///
/// Each item is yielded as `(T, ImmediateQueueStatus)` where the status
/// indicates whether the priority queue had more items remaining after this one
/// was consumed. This lets the consumer decide whether to defer low-urgency
/// work (e.g. paced data frames) to avoid blocking any high-priority
/// transmissions that are queued behind the current item.
///
/// The look-ahead is implemented by attempting an extra `poll_recv` on the
/// priority receiver immediately after consuming an item. If another item is
/// available it is stored in `peeked_priority` and returned on the next call,
/// so no item is ever dropped.
#[derive(Clone, Copy)]
enum PrioritySelectBranch {
    Priority,
    Fallback,
}

pub struct PrioritySelect<T, A, B> {
    priority: A,
    fallback: B,
    /// Pre-fetched item from the priority receiver (the look-ahead result).
    peeked_priority: Option<T>,
    last_recv_branch: Option<PrioritySelectBranch>,
    _phantom: PhantomData<T>,
}

impl<T, A, B> PrioritySelect<T, A, B> {
    pub fn new(priority: A, fallback: B) -> Self {
        Self {
            priority,
            fallback,
            peeked_priority: None,
            last_recv_branch: None,
            _phantom: PhantomData,
        }
    }
}

impl<T, A, B> Receiver<(T, ImmediateQueueStatus)> for PrioritySelect<T, A, B>
where
    A: Receiver<T>,
    B: Receiver<T>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<(T, ImmediateQueueStatus)>> {
        // If we pre-fetched a priority item on the previous call, return it now.
        if let Some(value) = self.peeked_priority.take() {
            if budget.is_exhausted() {
                // Can't process it yet; put it back and park.
                self.peeked_priority = Some(value);
                budget.set_needs_wake();
                return Poll::Pending;
            }
            budget.consume();
            self.last_recv_branch = Some(PrioritySelectBranch::Priority);
            let status = self.try_peek_priority(cx, budget);
            return Poll::Ready(Some((value, status)));
        }

        // Normal poll: try the priority receiver first.
        match self.priority.poll_recv(cx, budget) {
            Poll::Ready(Some(value)) => {
                self.last_recv_branch = Some(PrioritySelectBranch::Priority);
                let status = self.try_peek_priority(cx, budget);
                return Poll::Ready(Some((value, status)));
            }
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => {}
        }

        // Priority empty; fall back to the low-priority receiver.
        match self.fallback.poll_recv(cx, budget) {
            Poll::Ready(Some(value)) => {
                self.last_recv_branch = Some(PrioritySelectBranch::Fallback);
                // Priority was confirmed empty above; no need to peek again.
                Poll::Ready(Some((value, ImmediateQueueStatus::Empty)))
            }
            other => other.map(|x| x.map(|v| (v, ImmediateQueueStatus::Empty))),
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        match self.last_recv_branch {
            Some(PrioritySelectBranch::Priority) => self.priority.on_consumed(bytes),
            Some(PrioritySelectBranch::Fallback) => self.fallback.on_consumed(bytes),
            None => {}
        }
    }
}

impl<T, A, B> PrioritySelect<T, A, B>
where
    A: Receiver<T>,
{
    /// Attempt a non-blocking look-ahead on the priority receiver using the
    /// caller's existing budget.
    ///
    /// If the budget is already exhausted we cannot peek without bypassing
    /// fairness accounting, so we conservatively return
    /// [`ImmediateQueueStatus::BudgetExhausted`] — the caller should treat this
    /// as if there are more items waiting.
    ///
    /// Otherwise one unit of budget is consumed for the peek. If an item is
    /// found it is stored in `peeked_priority` for return on the next
    /// `poll_recv` call and [`ImmediateQueueStatus::HasMore`] is returned.
    /// If no item is available, [`ImmediateQueueStatus::Empty`] is returned.
    fn try_peek_priority(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> ImmediateQueueStatus {
        if budget.is_exhausted() {
            return ImmediateQueueStatus::BudgetExhausted;
        }
        match self.priority.poll_recv(cx, budget) {
            Poll::Ready(Some(next)) => {
                self.peeked_priority = Some(next);
                ImmediateQueueStatus::HasMore
            }
            // Priority closed or empty; no pre-fetch needed.
            Poll::Ready(None) | Poll::Pending => ImmediateQueueStatus::Empty,
        }
    }
}
