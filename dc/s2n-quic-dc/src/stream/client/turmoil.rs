// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    path::secret,
    stream::{
        application::Stream,
        endpoint,
        environment::turmoil::{self as env, Environment},
        socket::Protocol,
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
    let entry = handshake.await?;

    let peer = env::udp::Pooled(acceptor_addr.into());
    let stream = endpoint::open_stream(env, entry, peer, None)?;

    let stream = stream.connect()?;

    debug_assert_eq!(stream.protocol(), Protocol::Udp);

    Ok(stream)
}
