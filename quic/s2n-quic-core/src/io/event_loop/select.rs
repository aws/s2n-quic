// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint::CloseError;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use pin_project_lite::pin_project;

pin_project!(
    /// The main event loop future for selecting readiness of sub-tasks
    ///
    /// This future ensures all sub-tasks are polled fairly by yielding once
    /// after completing any of the sub-tasks. This is especially important when the TX queue is
    /// flushed quickly and we never get notified of the RX socket having packets to read.
    pub struct Select<Rx, Tx, Wakeup, Sleep>
    where
        Rx: Future,
        Tx: Future,
        Wakeup: Future,
        Sleep: Future,
    {
        #[pin]
        rx: Rx,
        #[pin]
        tx: Tx,
        #[pin]
        wakeup: Wakeup,
        #[pin]
        sleep: Sleep,
    }
);

impl<Rx, Tx, Wakeup, Sleep> Select<Rx, Tx, Wakeup, Sleep>
where
    Rx: Future,
    Tx: Future,
    Wakeup: Future,
    Sleep: Future,
{
    #[inline(always)]
    pub fn new(rx: Rx, tx: Tx, wakeup: Wakeup, sleep: Sleep) -> Self {
        Self {
            rx,
            tx,
            wakeup,
            sleep,
        }
    }
}

#[derive(Debug)]
pub struct Outcome<Rx, Tx> {
    pub rx_result: Option<Rx>,
    pub tx_result: Option<Tx>,
    pub timeout_expired: bool,
    pub application_wakeup: bool,
}

pub type Result<Rx, Tx> = core::result::Result<Outcome<Rx, Tx>, CloseError>;

impl<Rx, Tx, Wakeup, Sleep> Future for Select<Rx, Tx, Wakeup, Sleep>
where
    Rx: Future,
    Tx: Future,
    Wakeup: Future<Output = core::result::Result<usize, CloseError>>,
    Sleep: Future,
{
    type Output = Result<Rx::Output, Tx::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let mut should_wake = false;
        let mut application_wakeup = false;

        if let Poll::Ready(wakeup) = this.wakeup.poll(cx) {
            should_wake = true;
            application_wakeup = true;
            if let Err(err) = wakeup {
                return Poll::Ready(Err(err));
            }
        }

        let mut rx_result = None;
        if let Poll::Ready(v) = this.rx.poll(cx) {
            should_wake = true;
            rx_result = Some(v);
        }

        let mut tx_result = None;
        if let Poll::Ready(v) = this.tx.poll(cx) {
            should_wake = true;
            tx_result = Some(v);
        }

        let mut timeout_expired = false;

        if this.sleep.poll(cx).is_ready() {
            timeout_expired = true;
            should_wake = true;
        }

        // if none of the subtasks are ready, return
        if !should_wake {
            return Poll::Pending;
        }

        Poll::Ready(Ok(Outcome {
            rx_result,
            tx_result,
            timeout_expired,
            application_wakeup,
        }))
    }
}
