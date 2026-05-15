// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    msg::{addr::Addr, cmsg},
    socket::{pool, recv::router::Router},
    stream::socket::{fd::udp, Socket},
};
use std::{io, os::fd::AsRawFd};

/// Receives a packet into a pre-allocated stack buffer and discards it.
///
/// This is used as a fallback when the packet allocator is exhausted. We still
/// need to drain the socket so the kernel doesn't hold packets indefinitely,
/// but we have no memory to route them into.
#[inline]
fn blackhole_recv<S: AsRawFd>(socket: &S) {
    let mut addr = Addr::default();
    let mut cmsg_recv = cmsg::Receiver::default();
    let mut buf = [0u8; 1]; // minimal buffer just to drain one packet
    let iov = io::IoSliceMut::new(&mut buf);
    let _ = udp::recv(
        socket,
        &mut addr,
        &mut cmsg_recv,
        &mut [iov],
        Default::default(),
    );
}

/// Receives packets from a blocking [`std::net::UdpSocket`] and dispatches into the provided [`Router`]
///
/// Note: This function is synchronous and doesn't use the channel adapters since
/// it operates in a blocking context. For non-blocking operation, use `non_blocking`.
pub fn blocking<S: AsRawFd, R: Router>(socket: S, alloc: pool::Pool, mut router: R) {
    while router.is_open() {
        let Some(mut unfilled) = alloc.alloc() else {
            // Allocator exhausted — drain the socket to make progress but discard the packet
            blackhole_recv(&socket);
            continue;
        };

        while router.is_open() {
            let res = unfilled.fill_with(|addr, cmsg, buffer| {
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
pub async fn non_blocking<S: Socket, R: Router>(socket: S, alloc: pool::Pool, router: R) {
    use crate::socket::channel::{
        FlattenSegments, InspectErr, ReceiverExt, RouterAdapter, SocketReceiver,
    };

    // Chain the adapters: socket → SocketReceiver → InspectErr → FlattenSegments → RouterAdapter
    let rx = SocketReceiver::new(socket, alloc);
    let rx = InspectErr::new(rx, |err| {
        tracing::error!("socket recv error (kind={:?}): {err}", err.kind());
    });
    let rx = FlattenSegments::new(rx);
    let rx = RouterAdapter::new(rx, router);

    // Drain with budget of 10 items per poll before yielding
    rx.drain_budgeted(Some(10)).await;
}
