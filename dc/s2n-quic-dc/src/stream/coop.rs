// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::{Context, Poll};

/// Frames a single stream may process per poll before yielding the worker.
/// Counts individual frames, NOT whole poll calls.
pub(crate) const BUDGET: u16 = 128;

pub struct Coop {
    budget: u16,
}

impl Default for Coop {
    #[inline]
    fn default() -> Self {
        Self { budget: BUDGET }
    }
}

impl Coop {
    /// Consume one frame of budget. Returns `false` when exhausted (caller must
    /// stop and arrange to resume). Does NOT refill — refill happens in `poll`,
    /// when the worker actually yields.
    #[inline]
    pub fn consume(&mut self) -> bool {
        if self.budget == 0 {
            return false;
        }
        self.budget -= 1;
        true
    }

    #[inline]
    pub fn is_exhausted(&self) -> bool {
        self.budget == 0
    }

    #[inline]
    pub fn reset(&mut self) {
        self.budget = BUDGET;
    }
}

pub trait HasCoop {
    fn coop(&mut self) -> &mut Coop;
}

/// Cooperative-yield gate. If the per-frame budget is exhausted on entry,
/// refill it, self-wake, and yield (`Poll::Pending`) so other tasks run.
/// Otherwise run `f`. The budget is decremented per-frame by the body via
/// `Coop::consume`.
///
/// When the body returns `Pending` the budget is refilled, but the response
/// differs by *why* it is pending:
///
/// - **Coop-break** (`is_exhausted()` — the body drained the budget mid-work and
///   broke with more work to do): the body did NOT register a waker for that
///   remaining work, so we must self-wake to guarantee a re-poll. After refill
///   the next poll runs the body again and makes forward progress.
/// - **Natural wait** (budget remains — waiting on credit, an empty channel,
///   flow-control): a real waker is registered elsewhere, so no self-wake is
///   needed. Refilling avoids charging the wait against this stream's slice,
///   which would otherwise drain it across unrelated waits and cause spurious
///   yields later.
///
/// Forward progress (`Poll::Ready` while the body decremented the budget) leaves
/// the budget where it is, so the next poll's entry check yields once the slice
/// is spent.
pub fn poll<S, T>(
    this: &mut S,
    cx: &mut Context,
    f: impl FnOnce(&mut S, &mut Context) -> Poll<T>,
) -> Poll<T>
where
    S: HasCoop,
{
    if this.coop().is_exhausted() {
        this.coop().reset();
        cx.waker().wake_by_ref();
        return Poll::Pending;
    }
    let result = f(this, cx);
    if matches!(result, Poll::Pending) {
        // Self-wake BEFORE refill: a body that broke on an exhausted budget has
        // unfinished work but registered no waker for it. A natural wait (budget
        // not yet spent) already has its own waker, so leave it to fire.
        if this.coop().is_exhausted() {
            cx.waker().wake_by_ref();
        }
        this.coop().reset();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        task::{RawWaker, RawWakerVTable, Waker},
    };
    use std::sync::Arc;

    struct CountingWaker {
        wake_count: AtomicUsize,
    }

    impl CountingWaker {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                wake_count: AtomicUsize::new(0),
            })
        }

        fn count(&self) -> usize {
            self.wake_count.load(Ordering::SeqCst)
        }

        fn waker(self: Arc<Self>) -> Waker {
            unsafe fn clone(data: *const ()) -> RawWaker {
                let arc = Arc::from_raw(data as *const CountingWaker);
                let cloned = arc.clone();
                let _ = Arc::into_raw(arc);
                RawWaker::new(Arc::into_raw(cloned) as *const (), &VTABLE)
            }
            unsafe fn wake(data: *const ()) {
                let arc = Arc::from_raw(data as *const CountingWaker);
                arc.wake_count.fetch_add(1, Ordering::SeqCst);
            }
            unsafe fn wake_by_ref(data: *const ()) {
                let arc = Arc::from_raw(data as *const CountingWaker);
                arc.wake_count.fetch_add(1, Ordering::SeqCst);
                let _ = Arc::into_raw(arc);
            }
            unsafe fn drop_fn(data: *const ()) {
                let _ = Arc::from_raw(data as *const CountingWaker);
            }
            static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop_fn);
            let raw = RawWaker::new(Arc::into_raw(self) as *const (), &VTABLE);
            unsafe { Waker::from_raw(raw) }
        }
    }

    struct Holder {
        coop: Coop,
    }

    impl HasCoop for Holder {
        fn coop(&mut self) -> &mut Coop {
            &mut self.coop
        }
    }

    #[test]
    fn consume_does_not_auto_refill() {
        let mut coop = Coop::default();
        // Burn the entire budget.
        for _ in 0..BUDGET {
            assert!(coop.consume());
        }
        // Two consecutive `false` after exhaustion — proves no auto-refill.
        assert!(!coop.consume());
        assert!(!coop.consume());
        assert!(coop.is_exhausted());
    }

    #[test]
    fn reset_restores_full_budget() {
        let mut coop = Coop::default();
        for _ in 0..BUDGET {
            assert!(coop.consume());
        }
        assert!(coop.is_exhausted());
        coop.reset();
        assert!(!coop.is_exhausted());
        for _ in 0..BUDGET {
            assert!(coop.consume());
        }
    }

    #[test]
    fn poll_yields_when_exhausted() {
        let mut holder = Holder {
            coop: Coop::default(),
        };
        // Drain the budget directly.
        while holder.coop.consume() {}
        assert!(holder.coop.is_exhausted());

        let counting = CountingWaker::new();
        let waker = counting.clone().waker();
        let mut cx = Context::from_waker(&waker);

        let mut body_invoked = false;
        let result: Poll<()> = poll(&mut holder, &mut cx, |_this, _cx| {
            body_invoked = true;
            Poll::Ready(())
        });

        assert!(matches!(result, Poll::Pending));
        assert!(!body_invoked, "body must NOT run when budget is exhausted");
        assert_eq!(counting.count(), 1, "wake_by_ref must fire exactly once");
        assert!(!holder.coop.is_exhausted(), "budget must be reset on yield");
    }

    #[test]
    fn poll_runs_body_when_budget_available() {
        let mut holder = Holder {
            coop: Coop::default(),
        };
        let counting = CountingWaker::new();
        let waker = counting.clone().waker();
        let mut cx = Context::from_waker(&waker);

        let mut body_invoked = false;
        let result: Poll<u32> = poll(&mut holder, &mut cx, |_this, _cx| {
            body_invoked = true;
            Poll::Ready(42)
        });

        assert!(matches!(result, Poll::Ready(42)));
        assert!(body_invoked);
        assert_eq!(counting.count(), 0, "no self-wake when body runs");
    }
}
