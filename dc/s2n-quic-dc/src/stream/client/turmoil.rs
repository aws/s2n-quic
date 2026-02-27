// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    path::secret,
    stream::{
        application::Stream,
        environment::turmoil::{udp, Environment},
    },
};
use std::{io, net::SocketAddr};

/// Connects using the UDP transport layer over turmoil's simulated network
#[inline]
pub async fn connect_udp<H, Sub>(
    handshake: H,
    acceptor_addr: SocketAddr,
    env: &Environment<Sub>,
) -> io::Result<Stream<Sub>>
where
    H: core::future::Future<Output = io::Result<secret::map::Peer>>,
    Sub: event::Subscriber + Clone,
{
    super::connect_udp(handshake, acceptor_addr, env, |addr| udp::Pooled(addr.into())).await
}
