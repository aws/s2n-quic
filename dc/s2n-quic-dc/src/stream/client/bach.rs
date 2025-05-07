// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    path::secret,
    stream::{
        application::Stream,
        endpoint,
        environment::bach::{self as env, Environment},
        socket::Protocol,
    },
};
use std::{io, net::SocketAddr};

/// Connects using the UDP transport layer
///
/// Callers should send data immediately after calling this to ensure minimal
/// credential reordering.
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
    // ensure we have a secret for the peer
    let entry = handshake.await?;

    // TODO potentially branch on not using the recv pool if we're under a certain concurrency?
    let peer = env::udp::Pooled(acceptor_addr.into());
    let stream = endpoint::open_stream(env, entry, peer, None)?;

    // build the stream inside the application context
    let stream = stream.connect()?;

    debug_assert_eq!(stream.protocol(), Protocol::Udp);

    Ok(stream)
}
