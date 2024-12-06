// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    path::secret,
    stream::{
        application::Stream,
        endpoint,
        environment::tokio::{self as env, Environment},
        socket::Protocol,
    },
};
use std::{io, net::SocketAddr};
use tokio::net::TcpStream;

/// Connects using the UDP transport layer
#[inline]
pub async fn connect_udp<H, Sub>(
    handshake: H,
    acceptor_addr: SocketAddr,
    env: &Environment<Sub>,
    subscriber: Sub,
) -> io::Result<Stream<Sub>>
where
    H: core::future::Future<Output = io::Result<secret::map::Peer>>,
    Sub: event::Subscriber,
{
    // ensure we have a secret for the peer
    let peer = handshake.await?;

    let stream = endpoint::open_stream(
        env,
        peer,
        env::UdpUnbound(acceptor_addr.into()),
        subscriber,
        None,
    )?;

    // build the stream inside the application context
    let mut stream = stream.connect()?;

    debug_assert_eq!(stream.protocol(), Protocol::Udp);

    write_prelude(&mut stream).await?;

    Ok(stream)
}

/// Connects using the TCP transport layer
#[inline]
pub async fn connect_tcp<H, Sub>(
    handshake: H,
    acceptor_addr: SocketAddr,
    env: &Environment<Sub>,
    subscriber: Sub,
) -> io::Result<Stream<Sub>>
where
    H: core::future::Future<Output = io::Result<secret::map::Peer>>,
    Sub: event::Subscriber,
{
    // Race TCP handshake with the TLS handshake
    let (socket, peer) = tokio::try_join!(TcpStream::connect(acceptor_addr), handshake,)?;

    // Make sure TCP_NODELAY is set
    let _ = socket.set_nodelay(true);

    // if the acceptor_ip isn't known, then ask the socket to resolve it for us
    let peer_addr = if acceptor_addr.ip().is_unspecified() {
        socket.peer_addr()?
    } else {
        acceptor_addr
    }
    .into();
    let local_port = socket.local_addr()?.port();

    let stream = endpoint::open_stream(
        env,
        peer,
        env::TcpRegistered {
            socket,
            peer_addr,
            local_port,
        },
        subscriber,
        None,
    )?;

    // build the stream inside the application context
    let mut stream = stream.connect()?;

    debug_assert_eq!(stream.protocol(), Protocol::Tcp);

    write_prelude(&mut stream).await?;

    Ok(stream)
}

/// Connects with a pre-existing TCP stream
///
/// # Note
///
/// The provided `map` must contain a shared secret for the `handshake_addr`
#[inline]
pub async fn connect_tcp_with<Sub>(
    peer: secret::map::Peer,
    socket: TcpStream,
    env: &Environment<Sub>,
    subscriber: Sub,
) -> io::Result<Stream<Sub>>
where
    Sub: event::Subscriber,
{
    let local_port = socket.local_addr()?.port();
    let peer_addr = socket.peer_addr()?.into();
    let stream = endpoint::open_stream(
        env,
        peer,
        env::TcpRegistered {
            socket,
            peer_addr,
            local_port,
        },
        subscriber,
        None,
    )?;

    // build the stream inside the application context
    let mut stream = stream.connect()?;

    debug_assert_eq!(stream.protocol(), Protocol::Tcp);

    write_prelude(&mut stream).await?;

    Ok(stream)
}

#[inline]
async fn write_prelude<Sub>(stream: &mut Stream<Sub>) -> io::Result<()>
where
    Sub: event::Subscriber,
{
    // TODO should we actually write the prelude here or should we do late sealer binding on
    // the first packet to reduce secret reordering on the peer

    stream
        .write_from(&mut s2n_quic_core::buffer::reader::storage::Empty)
        .await
        .map(|_| ())
}
