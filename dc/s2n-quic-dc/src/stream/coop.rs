// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::{Context, Poll};

const BUDGET: u8 = 128;

pub struct Coop {
    budget: u8,
}

impl Default for Coop {
    #[inline]
    fn default() -> Self {
        Self { budget: BUDGET }
    }
}

pub trait HasCoop {
    fn coop(&mut self) -> &mut Coop;
}

pub fn poll<S, T>(
    this: &mut S,
    cx: &mut Context,
    f: impl FnOnce(&mut S, &mut Context) -> Poll<T>,
) -> Poll<T>
where
    S: HasCoop,
    Poll<T>: IsProgress,
{
    let budget = &mut this.coop().budget;
    if *budget == 0 {
        *budget = BUDGET;
        cx.waker().wake_by_ref();
        return Poll::Pending;
    }

    let result = f(this, cx);

    let budget = &mut this.coop().budget;
    if result.is_progress() {
        *budget -= 1;
    } else {
        *budget = BUDGET;
    }

    result
}

pub trait IsProgress {
    fn is_progress(&self) -> bool;
}

impl<T, E> IsProgress for Poll<Result<T, E>> {
    #[inline]
    fn is_progress(&self) -> bool {
        matches!(self, Poll::Ready(Ok(_)))
    }
}
