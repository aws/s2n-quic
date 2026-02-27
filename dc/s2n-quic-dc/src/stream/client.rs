// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(test, feature = "testing"))]
pub mod bach;
pub mod rpc;
#[cfg(feature = "tokio")]
pub mod tokio;
#[cfg(feature = "unstable-provider-io-turmoil")]
pub mod turmoil;

use crate::{
    event,
    path::secret,
    stream::{application::Stream, endpoint, environment::{Environment, Peer}, socket::Protocol},
};
use std::{io, net::SocketAddr};

/// Generic UDP connect for simulation environments (bach/turmoil)
#[cfg(any(test, feature = "testing"))]
pub async fn connect_udp<H, E, P, Sub>(
    handshake: H,
    acceptor_addr: SocketAddr,
    env: &E,
    make_peer: impl FnOnce(SocketAddr) -> P,
) -> io::Result<Stream<Sub>>
where
    H: core::future::Future<Output = io::Result<secret::map::Peer>>,
    E: Environment<Subscriber = Sub>,
    P: Peer<E>,
    Sub: event::Subscriber + Clone,
{
    let entry = handshake.await?;
    let peer = make_peer(acceptor_addr);
    let stream = endpoint::open_stream(env, entry, peer, None)?;
    let stream = stream.connect()?;
    debug_assert_eq!(stream.protocol(), Protocol::Udp);
    Ok(stream)
}
