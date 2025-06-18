// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    either::Either,
    event::{self, builder::StreamTcpConnectErrorReason, EndpointPublisher},
    msg,
    path::secret,
    stream::{
        application::Stream,
        endpoint,
        environment::{
            tokio::{self as env, Environment},
            Environment as _,
        },
        recv,
        socket::Protocol,
    },
};
use s2n_quic_core::time::Clock;
use std::{io, net::SocketAddr, time::Duration};
use tokio::net::TcpStream;

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

    // TODO emit events (https://github.com/aws/s2n-quic/issues/2676)

    // TODO potentially branch on not using the recv pool if we're under a certain concurrency?
    let stream = if env.has_recv_pool() {
        let peer = env::udp::Pooled(acceptor_addr.into());
        endpoint::open_stream(env, entry, peer, None)?
    } else {
        let peer = env::udp::Owned(acceptor_addr.into(), recv_buffer());
        endpoint::open_stream(env, entry, peer, None)?
    };

    // build the stream inside the application context
    let stream = stream.connect()?;

    debug_assert_eq!(stream.protocol(), Protocol::Udp);

    Ok(stream)
}

struct DropGuard<'a, S: event::Subscriber + Clone> {
    env: &'a Environment<S>,
    reason: Option<StreamTcpConnectErrorReason>,
}

impl<S: event::Subscriber + Clone> Drop for DropGuard<'_, S> {
    fn drop(&mut self) {
        if let Some(reason) = self.reason.take() {
            self.env
                .endpoint_publisher()
                .on_stream_connect_error(event::builder::StreamConnectError { reason });
        }
    }
}

/// Connects using the TCP transport layer
///
/// Callers should send data immediately after calling this to ensure minimal
/// credential reordering.
#[inline]
pub async fn connect_tcp<H, Sub>(
    handshake: H,
    acceptor_addr: SocketAddr,
    env: &Environment<Sub>,
    linger: Option<Duration>,
) -> io::Result<Stream<Sub>>
where
    H: core::future::Future<Output = io::Result<secret::map::Peer>>,
    Sub: event::Subscriber + Clone,
{
    // This emits the error event in case this future gets dropped.
    let mut guard = DropGuard {
        env,
        reason: Some(StreamTcpConnectErrorReason::Aborted),
    };

    let connect = TcpStream::connect(acceptor_addr);

    tokio::pin!(handshake);
    tokio::pin!(connect);

    // We race the TCP connect() future with either retrieving cached path secret or handshaking to
    // produce those credentials.
    //
    // This should lower our worst-case latency from 3 RTT = TCP (1 RTT) + QUIC (~2 RTT) to just ~2 RTT.
    let mut error = None;
    let mut socket = None;
    let mut peer = None;
    let start = env.clock().get_time();
    while (socket.is_none() || peer.is_none()) && error.is_none() {
        tokio::select! {
            connected = &mut connect, if socket.is_none() => {
                let now = env.clock().get_time();
                env.endpoint_publisher_with_time(now).on_stream_tcp_connect(event::builder::StreamTcpConnect {
                    error: connected.is_err(),
                    latency: now.saturating_duration_since(start),
                });
                match connected {
                    Ok(v) => socket = Some(Ok(v)),
                    Err(e) => {
                        guard.reason = Some(StreamTcpConnectErrorReason::TcpConnect);
                        error = Some(e);
                        socket = Some(Err(()));
                    }
                }
            }
            handshaked = &mut handshake, if peer.is_none() => {
                match handshaked {
                    Ok(v) => peer = Some(Ok(v)),
                    Err(e) => {
                        guard.reason = Some(StreamTcpConnectErrorReason::Handshake);
                        error = Some(e);
                        peer = Some(Err(()));
                    }
                }
            }
        }
    }
    env.endpoint_publisher()
        .on_stream_connect(event::builder::StreamConnect {
            error: error.is_some(),
            handshake_success: match &peer {
                Some(Ok(_)) => event::builder::MaybeBoolCounter::Success,
                Some(Err(_)) => event::builder::MaybeBoolCounter::Failure,
                None => event::builder::MaybeBoolCounter::Aborted,
            },
            tcp_success: match &socket {
                Some(Ok(_)) => event::builder::MaybeBoolCounter::Success,
                Some(Err(_)) => event::builder::MaybeBoolCounter::Failure,
                None => event::builder::MaybeBoolCounter::Aborted,
            },
        });

    let (Some(Ok(socket)), Some(Ok(entry))) = (socket, peer) else {
        // unwrap is OK -- if socket or peer isn't present the error should be set.
        return Err(error.unwrap());
    };

    // Make sure TCP_NODELAY is set
    let _ = socket.set_nodelay(true);

    if linger.is_some() {
        let _ = socket.set_linger(linger);
    }

    // if the acceptor_ip isn't known, then ask the socket to resolve it for us
    let peer_addr = if acceptor_addr.ip().is_unspecified() {
        socket.peer_addr()?
    } else {
        acceptor_addr
    }
    .into();
    let local_port = socket.local_addr()?.port();

    let peer = env::tcp::Registered {
        socket,
        peer_addr,
        local_port,
        recv_buffer: recv_buffer(),
    };

    let stream = endpoint::open_stream(env, entry, peer, None)?;

    // build the stream inside the application context
    let stream = stream.connect()?;

    debug_assert_eq!(stream.protocol(), Protocol::Tcp);

    Ok(stream)
}

/// Connects with a pre-existing TCP stream
///
/// Callers should send data immediately after calling this to ensure minimal
/// credential reordering.
///
/// # Note
///
/// The provided `map` must contain a shared secret for the `handshake_addr`
#[inline]
pub async fn connect_tcp_with<Sub>(
    entry: secret::map::Peer,
    socket: TcpStream,
    env: &Environment<Sub>,
) -> io::Result<Stream<Sub>>
where
    Sub: event::Subscriber + Clone,
{
    let local_port = socket.local_addr()?.port();
    let peer_addr = socket.peer_addr()?.into();

    let peer = env::tcp::Registered {
        socket,
        peer_addr,
        local_port,
        recv_buffer: recv_buffer(),
    };

    // TODO emit events (https://github.com/aws/s2n-quic/issues/2676)

    let stream = endpoint::open_stream(env, entry, peer, None)?;

    // build the stream inside the application context
    let stream = stream.connect()?;

    debug_assert_eq!(stream.protocol(), Protocol::Tcp);

    Ok(stream)
}

#[inline]
fn recv_buffer() -> recv::shared::RecvBuffer {
    // TODO replace this with a parameter once everything is in place
    let recv_buffer = recv::buffer::Local::new(msg::recv::Message::new(9000), None);
    Either::A(recv_buffer)
}
