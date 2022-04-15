// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use futures::future::{Fuse, FutureExt};
use pin_project::pin_project;
use s2n_quic_core::endpoint::CloseError;

/// The main event loop future for selecting readiness of sub-tasks
///
/// This future ensures all sub-tasks are polled fairly by yielding once
/// after completing any of the sub-tasks. This is especially important when the TX queue is
/// flushed quickly and we never get notified of the RX socket having packets to read.
#[pin_project]
pub struct Select<Rx, Tx, Wakeup, Sleep>
where
    Rx: Future,
    Tx: Future,
    Wakeup: Future,
    Sleep: Future,
{
    #[pin]
    rx: Fuse<Rx>,
    rx_out: Option<Rx::Output>,
    #[pin]
    tx: Fuse<Tx>,
    tx_out: Option<Tx::Output>,
    #[pin]
    wakeup: Fuse<Wakeup>,
    #[pin]
    sleep: Sleep,
}

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
            rx: rx.fuse(),
            rx_out: None,
            tx: tx.fuse(),
            tx_out: None,
            wakeup: wakeup.fuse(),
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

        if let Poll::Ready(v) = this.rx.poll(cx) {
            should_wake = true;
            *this.rx_out = Some(v);
        }

        if let Poll::Ready(v) = this.tx.poll(cx) {
            should_wake = true;
            *this.tx_out = Some(v);
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
            rx_result: this.rx_out.take(),
            tx_result: this.tx_out.take(),
            timeout_expired,
            application_wakeup,
        }))
    }
}
