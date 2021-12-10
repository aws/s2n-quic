// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::connection;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use s2n_quic_transport::endpoint::close;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct CloseAttempt(pub close::Attempt);

impl Future for CloseAttempt {
    type Output = Result<(), connection::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.0).poll(cx) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}
