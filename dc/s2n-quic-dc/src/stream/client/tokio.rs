// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
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
pub async fn connect_udp<H>(
    handshake_addr: SocketAddr,
    handshake: H,
    acceptor_addr: SocketAddr,
    env: &Environment,
    map: &secret::Map,
) -> io::Result<Stream>
where
    H: core::future::Future<Output = io::Result<secret::HandshakeKind>>,
{
    // ensure we have a secret for the peer
    handshake.await?;

    let stream = endpoint::open_stream(
        env,
        handshake_addr.into(),
        env::UdpUnbound(acceptor_addr.into()),
        map,
        None,
    )?;

    // build the stream inside the application context
    let mut stream = stream.build()?;

    debug_assert_eq!(stream.protocol(), Protocol::Udp);

    write_prelude(&mut stream).await?;

    Ok(stream)
}

/// Connects using the TCP transport layer
#[inline]
pub async fn connect_tcp<H>(
    handshake_addr: SocketAddr,
    handshake: H,
    acceptor_addr: SocketAddr,
    env: &Environment,
    map: &secret::Map,
) -> io::Result<Stream>
where
    H: core::future::Future<Output = io::Result<secret::HandshakeKind>>,
{
    // Race TCP handshake with the TLS handshake
    let (socket, _) = tokio::try_join!(TcpStream::connect(acceptor_addr), handshake,)?;

    let stream = endpoint::open_stream(
        env,
        handshake_addr.into(),
        env::TcpRegistered(socket),
        map,
        None,
    )?;

    // build the stream inside the application context
    let mut stream = stream.build()?;

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
pub async fn connect_tcp_with(
    handshake_addr: SocketAddr,
    stream: TcpStream,
    env: &Environment,
    map: &secret::Map,
) -> io::Result<Stream> {
    let stream = endpoint::open_stream(
        env,
        handshake_addr.into(),
        env::TcpRegistered(stream),
        map,
        None,
    )?;

    // build the stream inside the application context
    let mut stream = stream.build()?;

    debug_assert_eq!(stream.protocol(), Protocol::Tcp);

    write_prelude(&mut stream).await?;

    Ok(stream)
}

#[inline]
async fn write_prelude(stream: &mut Stream) -> io::Result<()> {
    // TODO should we actually write the prelude here or should we do late sealer binding on
    // the first packet to reduce secret reordering on the peer

    stream
        .write_from(&mut s2n_quic_core::buffer::reader::storage::Empty)
        .await
        .map(|_| ())
}
