// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    either::Either,
    event::{self, builder::StreamTcpConnectErrorReason, EndpointPublisher},
    msg,
    path::secret,
    stream::{
        application::Stream,
        client::{rpc as rpc_internal, tokio as client},
        endpoint,
        environment::{
            tokio::{self as env, Environment},
            udp as udp_pool, Environment as _,
        },
        recv, socket,
    },
};
use s2n_quic_core::time::Clock;
use std::{io, net::SocketAddr, time::Duration};
use tokio::net::TcpStream;

pub mod rpc {
    pub use crate::stream::client::rpc::{InMemoryResponse, Request, Response};
}

// This trait is a temporary solution to abstract handshake_with_entry,
// local_addr, and map methods until we implement the handshake provider
#[allow(async_fn_in_trait)]
pub trait Handshake: Clone {
    /// Handshake with the remote peer
    async fn handshake_with_entry(
        &self,
        remote_handshake_addr: SocketAddr,
    ) -> std::io::Result<(secret::map::Peer, secret::HandshakeKind)>;

    fn local_addr(&self) -> std::io::Result<SocketAddr>;

    fn map(&self) -> &secret::Map;
}

#[derive(Clone)]
pub struct Client<H: Handshake + Clone, S: event::Subscriber + Clone> {
    env: Environment<S>,
    handshake: H,
    default_protocol: socket::Protocol,
    linger: Option<Duration>,
}

impl<H: Handshake + Clone, S: event::Subscriber + Clone> Client<H, S> {
    #[inline]
    pub fn new(handshake: H, subscriber: S) -> io::Result<Self> {
        Self::builder().build(handshake, subscriber)
    }

    #[inline]
    pub fn builder() -> Builder {
        Builder::default()
    }

    pub fn drop_state(&self) {
        self.handshake.map().drop_state()
    }

    pub fn handshake_state(&self) -> &H {
        &self.handshake
    }

    #[inline]
    pub async fn handshake_with(
        &self,
        remote_handshake_addr: SocketAddr,
    ) -> io::Result<secret::HandshakeKind> {
        let (_peer, kind) = self
            .handshake
            .handshake_with_entry(remote_handshake_addr)
            .await?;
        Ok(kind)
    }

    #[inline]
    async fn handshake_for_connect(
        &self,
        remote_handshake_addr: SocketAddr,
    ) -> io::Result<secret::map::Peer> {
        let (peer, _kind) = self
            .handshake
            .handshake_with_entry(remote_handshake_addr)
            .await?;
        Ok(peer)
    }

    /// Connects using the preferred protocol
    #[inline]
    pub async fn connect(
        &self,
        handshake_addr: SocketAddr,
        acceptor_addr: SocketAddr,
    ) -> io::Result<Stream<S>> {
        match self.default_protocol {
            socket::Protocol::Udp => self.connect_udp(handshake_addr, acceptor_addr).await,
            socket::Protocol::Tcp => self.connect_tcp(handshake_addr, acceptor_addr).await,
            protocol => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid default protocol {protocol:?}"),
            )),
        }
    }

    /// Makes an RPC request using the preferred protocol
    pub async fn rpc<Req, Res>(
        &self,
        handshake_addr: SocketAddr,
        acceptor_addr: SocketAddr,
        request: Req,
        response: Res,
    ) -> io::Result<Res::Output>
    where
        Req: rpc::Request,
        Res: rpc::Response,
    {
        match self.default_protocol {
            socket::Protocol::Udp => {
                self.rpc_udp(handshake_addr, acceptor_addr, request, response)
                    .await
            }
            socket::Protocol::Tcp => {
                self.rpc_tcp(handshake_addr, acceptor_addr, request, response)
                    .await
            }
            protocol => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid default protocol {protocol:?}"),
            )),
        }
    }

    /// Connects using the UDP transport layer
    #[inline]
    pub async fn connect_udp(
        &self,
        handshake_addr: SocketAddr,
        acceptor_addr: SocketAddr,
    ) -> io::Result<Stream<S>> {
        // ensure we have a secret for the peer
        let handshake = self.handshake_for_connect(handshake_addr);

        let mut stream = client::connect_udp(handshake, acceptor_addr, &self.env).await?;
        Self::write_prelude(&mut stream).await?;
        Ok(stream)
    }

    /// Makes an RPC request using the UDP transport layer
    #[inline]
    pub async fn rpc_udp<Req, Res>(
        &self,
        handshake_addr: SocketAddr,
        acceptor_addr: SocketAddr,
        request: Req,
        response: Res,
    ) -> io::Result<Res::Output>
    where
        Req: rpc::Request,
        Res: rpc::Response,
    {
        // ensure we have a secret for the peer
        let handshake = self.handshake_for_connect(handshake_addr);

        let stream = client::connect_udp(handshake, acceptor_addr, &self.env).await?;
        rpc_internal::from_stream(stream, request, response).await
    }

    /// Connects using the TCP transport layer
    #[inline]
    pub async fn connect_tcp(
        &self,
        handshake_addr: SocketAddr,
        acceptor_addr: SocketAddr,
    ) -> io::Result<Stream<S>> {
        // ensure we have a secret for the peer
        let handshake = self.handshake_for_connect(handshake_addr);

        let mut stream =
            client::connect_tcp(handshake, acceptor_addr, &self.env, self.linger).await?;
        Self::write_prelude(&mut stream).await?;
        Ok(stream)
    }

    /// Makes an RPC request using the TCP transport layer
    #[inline]
    pub async fn rpc_tcp<Req, Res>(
        &self,
        handshake_addr: SocketAddr,
        acceptor_addr: SocketAddr,
        request: Req,
        response: Res,
    ) -> io::Result<Res::Output>
    where
        Req: rpc::Request,
        Res: rpc::Response,
    {
        // ensure we have a secret for the peer
        let handshake = self.handshake_for_connect(handshake_addr);

        let stream = client::connect_tcp(handshake, acceptor_addr, &self.env, self.linger).await?;
        rpc_internal::from_stream(stream, request, response).await
    }

    /// Connects with a pre-existing TCP stream
    #[inline]
    pub async fn connect_tcp_with(
        &self,
        handshake_addr: SocketAddr,
        stream: TcpStream,
    ) -> io::Result<Stream<S>> {
        // ensure we have a secret for the peer
        let handshake = self.handshake_for_connect(handshake_addr).await?;

        let mut stream = client::connect_tcp_with(handshake, stream, &self.env).await?;
        Self::write_prelude(&mut stream).await?;
        Ok(stream)
    }

    #[inline]
    async fn write_prelude(stream: &mut Stream<S>) -> io::Result<()> {
        // TODO should we actually write the prelude here or should we do late sealer binding on
        // the first packet to reduce secret reordering on the peer

        stream
            .write_from(&mut s2n_quic_core::buffer::reader::storage::Empty)
            .await
            .map(|_| ())
    }
}

#[derive(Default)]
pub struct Builder {
    default_protocol: Option<socket::Protocol>,
    background_threads: Option<usize>,
    linger: Option<Duration>,
    send_buffer: Option<usize>,
    recv_buffer: Option<usize>,
}

impl Builder {
    pub fn with_tcp(self, enabled: bool) -> Self {
        self.with_default_protocol(if enabled {
            socket::Protocol::Tcp
        } else {
            socket::Protocol::Udp
        })
    }

    pub fn with_udp(self, enabled: bool) -> Self {
        self.with_default_protocol(if enabled {
            socket::Protocol::Udp
        } else {
            socket::Protocol::Tcp
        })
    }

    pub fn with_default_protocol(mut self, protocol: socket::Protocol) -> Self {
        self.default_protocol = Some(protocol);
        self
    }

    pub fn with_background_threads(mut self, threads: usize) -> Self {
        self.background_threads = Some(threads);
        self
    }

    pub fn with_linger(mut self, linger: Duration) -> Self {
        self.linger = Some(linger);
        self
    }

    /// Sets the send buffer for the OS socket handle.
    ///
    /// See `SO_SNDBUF` for more information.
    ///
    /// Note that this only applies to sockets that are created by s2n-quic-dc. Any sockets
    /// provided by the application will not inherit this value.
    pub fn with_send_buffer(mut self, bytes: usize) -> Self {
        self.send_buffer = Some(bytes);
        self
    }

    /// Sets the recv buffer for the OS socket handle.
    ///
    /// See `SO_RCVBUF` for more information.
    ///
    /// Note that this only applies to sockets that are created by s2n-quic-dc. Any sockets
    /// provided by the application will not inherit this value.
    pub fn with_recv_buffer(mut self, bytes: usize) -> Self {
        self.recv_buffer = Some(bytes);
        self
    }

    #[inline]
    pub fn build<H: Handshake + Clone, S: event::Subscriber + Clone>(
        self,
        handshake: H,
        subscriber: S,
    ) -> io::Result<Client<H, S>> {
        // bind the sockets to the same address family as the handshake
        let mut local_addr = handshake.local_addr()?;
        local_addr.set_port(0);
        let mut options = socket::Options::new(local_addr);

        options.send_buffer = self.send_buffer;
        options.recv_buffer = self.recv_buffer;

        let mut env = env::Builder::new(subscriber).with_socket_options(options);

        let pool = udp_pool::Config::new(handshake.map().clone());
        env = env.with_pool(pool);

        if let Some(threads) = self.background_threads {
            env = env.with_threads(threads);
        }
        let env = env.build()?;

        // default to UDP
        let default_protocol = self.default_protocol.unwrap_or(socket::Protocol::Udp);

        let linger = self.linger;

        Ok(Client {
            env,
            handshake,
            default_protocol,
            linger,
        })
    }
}

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

    debug_assert_eq!(stream.protocol(), socket::Protocol::Udp);

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

    // Clear the guard, we were successful. This stops emitting a metric indicating we dropped
    // before the stream was connected.
    if error.is_none() {
        guard.reason = None;
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

    debug_assert_eq!(stream.protocol(), socket::Protocol::Tcp);

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

    debug_assert_eq!(stream.protocol(), socket::Protocol::Tcp);

    Ok(stream)
}

#[inline]
fn recv_buffer() -> recv::shared::RecvBuffer {
    // TODO replace this with a parameter once everything is in place
    let recv_buffer = recv::buffer::Local::new(msg::recv::Message::new(9000), None);
    Either::A(recv_buffer)
}
