// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::sync::primitive::{Arc, AtomicU64, Ordering};
use core::{
    fmt,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

/// Checks that if a function returns [`Poll::Pending`], then the function called [`Waker::clone`],
/// [`Waker::wake`], or [`Waker::wake_by_ref`] on the [`Context`]'s [`Waker`].
pub struct Contract {
    state: Arc<State>,
    waker: Waker,
}

struct State {
    inner: Waker,
    clone_count: AtomicU64,
    drop_count: AtomicU64,
    wake_count: AtomicU64,
    wake_by_ref_count: AtomicU64,
}

unsafe fn contract_clone(data: *const ()) -> RawWaker {
    let state_ptr = data as *const State;
    Arc::increment_strong_count(state_ptr);
    (*state_ptr).clone_count.fetch_add(1, Ordering::Release);
    RawWaker::new(data, &VTABLE)
}

unsafe fn contract_wake(data: *const ()) {
    let arc = Arc::from_raw(data as *const State);
    arc.wake_count.fetch_add(1, Ordering::Release);
    arc.inner.wake_by_ref();
    arc.drop_count.fetch_add(1, Ordering::Release);
    // arc drops here, decrementing strong count
}

unsafe fn contract_wake_by_ref(data: *const ()) {
    let state = &*(data as *const State);
    state.wake_by_ref_count.fetch_add(1, Ordering::Release);
    state.inner.wake_by_ref();
}

unsafe fn contract_drop(data: *const ()) {
    let arc = Arc::from_raw(data as *const State);
    arc.drop_count.fetch_add(1, Ordering::Release);
    // arc drops here, decrementing strong count
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(
    contract_clone,
    contract_wake,
    contract_wake_by_ref,
    contract_drop,
);

impl Contract {
    /// Wraps a [`Context`] in the contract checker
    #[inline]
    pub fn new(cx: &mut Context) -> Self {
        let state = State {
            inner: cx.waker().clone(),
            clone_count: AtomicU64::new(0),
            drop_count: AtomicU64::new(0),
            wake_count: AtomicU64::new(0),
            wake_by_ref_count: AtomicU64::new(0),
        };
        let state = Arc::new(state);
        // Clone the Arc and convert to raw pointer for the waker.
        // This "leaks" one strong count into the raw pointer, which
        // contract_drop will reclaim via Arc::from_raw.
        let ptr = Arc::into_raw(state.clone()) as *const ();
        let raw = RawWaker::new(ptr, &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        Self { state, waker }
    }

    /// Returns a new [`Context`] to be checked
    #[inline]
    pub fn context(&self) -> Context<'_> {
        Context::from_waker(&self.waker)
    }

    /// Checks the state of the waker based on the provided `outcome`
    #[inline]
    #[track_caller]
    pub fn check_outcome<T, C: fmt::Debug, F: FnOnce() -> Option<C>>(
        self,
        outcome: &Poll<T>,
        context: F,
    ) {
        if outcome.is_ready() {
            return;
        }

        let clone_count = self.state.clone_count.load(Ordering::Acquire);
        let drop_count = self.state.drop_count.load(Ordering::Acquire);
        let wake_count = self.state.wake_count.load(Ordering::Acquire);
        let wake_by_ref_count = self.state.wake_by_ref_count.load(Ordering::Acquire);

        let live_clones = clone_count.saturating_sub(drop_count);
        let is_cloned = live_clones > 0;
        let wake_called = (wake_count + wake_by_ref_count) > 0;

        let is_ok = is_cloned || wake_called;
        if !is_ok {
            if let Some(context) = context() {
                panic!(
                    "clone_count = {clone_count}; drop_count = {drop_count}; \
                     wake_count = {wake_count}; wake_by_ref_count = {wake_by_ref_count}; \
                     live_clones = {live_clones}; contract_context = {context:?}"
                );
            } else {
                panic!(
                    "clone_count = {clone_count}; drop_count = {drop_count}; \
                     wake_count = {wake_count}; wake_by_ref_count = {wake_by_ref_count}; \
                     live_clones = {live_clones}"
                );
            }
        }
    }
}

/// Checks that if a function returns [`Poll::Pending`], then the function called [`Waker::clone`],
/// [`Waker::wake`], or [`Waker::wake_by_ref`] on the [`Context`]'s [`Waker`].
#[inline(always)]
#[track_caller]
pub fn assert_contract<F: FnOnce(&mut Context) -> Poll<R>, R>(cx: &mut Context, f: F) -> Poll<R> {
    assert_contract_with_context(cx, f, || Option::<()>::None)
}

/// Checks that if a function returns [`Poll::Pending`], then the function called [`Waker::clone`],
/// [`Waker::wake`], or [`Waker::wake_by_ref`] on the [`Context`]'s [`Waker`].
///
/// Includes optional `context` in panic output when the contract is violated.
#[inline(always)]
#[track_caller]
pub fn assert_contract_with_context<
    F: FnOnce(&mut Context) -> Poll<R>,
    Ctx: FnOnce() -> Option<C>,
    R,
    C: fmt::Debug,
>(
    cx: &mut Context,
    f: F,
    context: Ctx,
) -> Poll<R> {
    let contract = Contract::new(cx);
    let mut cx = contract.context();
    let outcome = f(&mut cx);
    contract.check_outcome(&outcome, context);
    outcome
}

/// Checks that if a function returns [`Poll::Pending`], then the function called [`Waker::clone`],
/// [`Waker::wake`], or [`Waker::wake_by_ref`] on the [`Context`]'s [`Waker`].
///
/// This is only enabled with `debug_assertions`.
#[inline(always)]
#[track_caller]
pub fn debug_assert_contract<F: FnOnce(&mut Context) -> Poll<R>, R>(
    cx: &mut Context,
    f: F,
) -> Poll<R> {
    debug_assert_contract_with_context(cx, f, || Option::<()>::None)
}

/// Checks that if a function returns [`Poll::Pending`], then the function called [`Waker::clone`],
/// [`Waker::wake`], or [`Waker::wake_by_ref`] on the [`Context`]'s [`Waker`].
///
/// Includes optional `context` in panic output when the contract is violated.
///
/// This is only enabled with `debug_assertions`.
#[inline(always)]
#[track_caller]
pub fn debug_assert_contract_with_context<
    F: FnOnce(&mut Context) -> Poll<R>,
    Ctx: FnOnce() -> Option<C>,
    R,
    C: fmt::Debug,
>(
    cx: &mut Context,
    f: F,
    context: Ctx,
) -> Poll<R> {
    #[cfg(debug_assertions)]
    return assert_contract_with_context(cx, f, context);

    #[cfg(not(debug_assertions))]
    return f(cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{task::waker, testing::loom};

    #[test]
    fn correct_test() {
        let waker = waker::noop();
        let mut cx = Context::from_waker(&waker);

        // the contract isn't violated when returning Ready
        let _ = assert_contract(&mut cx, |_cx| Poll::Ready(()));

        // the contract isn't violated if the waker is immediately woken
        let _ = assert_contract(&mut cx, |cx| {
            cx.waker().wake_by_ref();
            Poll::<()>::Pending
        });

        // the contract isn't violated if the waker is cloned then immediately woken
        let _ = assert_contract(&mut cx, |cx| {
            let waker = cx.waker().clone();
            waker.wake();
            Poll::<()>::Pending
        });

        // the contract isn't violated if the waker is cloned and stored for later
        let mut stored = None;
        let _ = assert_contract(&mut cx, |cx| {
            stored = Some(cx.waker().clone());
            Poll::<()>::Pending
        });
    }

    #[test]
    #[should_panic]
    fn incorrect_test() {
        let waker = waker::noop();
        let mut cx = Context::from_waker(&waker);

        // the contract is violated if we return Pending without doing anything
        let _ = assert_contract(&mut cx, |_cx| Poll::<()>::Pending);
    }

    /// Verifies the contract checker is correct under concurrent wake-and-check.
    ///
    /// Models the interaction between a poller (which clones the waker into a queue
    /// and then checks the contract) and a pusher (which takes the waker from the
    /// queue and calls wake, consuming it).
    #[test]
    #[cfg_attr(miri, ignore)]
    fn loom_concurrent_wake_and_check() {
        use loom::sync::{Arc, Mutex};

        loom::model(|| {
            // Shared slot: poller stores a waker clone, pusher takes and wakes it
            let slot: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));

            // Thread A (poller): stores waker via assert_contract, returns Pending
            let poller_slot = slot.clone();
            let poller = loom::thread::spawn(move || {
                let noop = waker::noop();
                let mut cx = Context::from_waker(&noop);
                let _ = assert_contract_with_context::<_, _, (), ()>(
                    &mut cx,
                    |cx| {
                        *poller_slot.lock().unwrap() = Some(cx.waker().clone());
                        Poll::<()>::Pending
                    },
                    || None,
                );
            });

            // Thread B (pusher): takes and wakes (simulating push -> take_waker -> wake)
            let pusher_slot = slot.clone();
            let pusher = loom::thread::spawn(move || {
                if let Some(waker) = pusher_slot.lock().unwrap().take() {
                    waker.wake();
                }
            });

            poller.join().unwrap();
            pusher.join().unwrap();
        });
    }
}
