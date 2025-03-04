// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    socket::recv::{pool, router::Router},
    stream::socket::fd::udp,
};
use std::net::UdpSocket;

/// Receives packets from a blocking [`UdpSocket`] and dispatches into the provided [`Router`]
pub fn blocking<R: Router>(socket: UdpSocket, mut pool: pool::Pool, mut router: R) {
    loop {
        let mut unfilled = pool.alloc_or_grow();
        loop {
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
