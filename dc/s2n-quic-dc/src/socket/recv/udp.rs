// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    socket::recv::{pool, router::Router},
    stream::socket::{fd::udp, Socket},
};
use std::{io, os::fd::AsRawFd, task::Poll};

/// Receives packets from a blocking [`std::net::UdpSocket`] and dispatches into the provided [`Router`]
pub fn blocking<S: AsRawFd, R: Router>(socket: S, mut alloc: pool::Pool, mut router: R) {
    while router.is_open() {
        let mut unfilled = alloc.alloc_or_grow();
        while router.is_open() {
            let res = unfilled.recv_with(|addr, cmsg, buffer| {
                udp::recv(&socket, addr, cmsg, &mut [buffer], Default::default())
            });

            match res {
                Ok(segments) => {
                    for segment in segments {
                        router.on_segment(segment);
                    }
                    break;
                }
                Err((desc, err)) => {
                    tracing::error!("socket recv error: {err}");
                    unfilled = desc;
                    continue;
                }
            }
        }
    }
}

/// Receives packets from a non-blocking [`std::net::UdpSocket`] and dispatches into the provided [`Router`]
pub async fn non_blocking<S: Socket, R: Router>(socket: S, mut alloc: pool::Pool, mut router: R) {
    let mut pending = None;
    core::future::poll_fn(move |cx| {
        while router.is_open() {
            let unfilled = pending.take().unwrap_or_else(|| alloc.alloc_or_grow());

            let res = unfilled.recv_with(|addr, cmsg, buffer| {
                match socket.poll_recv(cx, addr, cmsg, &mut [buffer]) {
                    Poll::Pending => Err(io::ErrorKind::WouldBlock.into()),
                    Poll::Ready(Ok(len)) => Ok(len),
                    Poll::Ready(Err(err)) => Err(err),
                }
            });

            match res {
                Ok(segments) => {
                    for segment in segments {
                        router.on_segment(segment);
                    }

                    // poll the socket again
                    continue;
                }
                Err((desc, err)) => {
                    // put the unfilled segment back in the pool
                    pending = Some(desc);

                    let kind = err.kind();

                    // if we got blocked then yield the future
                    if kind == io::ErrorKind::WouldBlock {
                        return Poll::Pending;
                    }

                    // if tokio is shutting down, it starts returning an `Other` error
                    if kind == io::ErrorKind::Other {
                        tracing::info!("worker shutting down due to: {err}");
                        break;
                    }

                    tracing::error!("socket recv error (kind={:?}): {err}", err.kind());
                }
            }
        }

        Poll::Ready(())
    })
    .await;
}
