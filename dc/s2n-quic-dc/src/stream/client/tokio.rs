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
use s2n_quic::server::Name;
use s2n_quic_core::{
    event::IntoEvent,
    inet::ExplicitCongestionNotification,
    time::{Clock, Timestamp},
    varint::VarInt,
};
use std::{cell::UnsafeCell, io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::TcpStream;

pub mod rpc {
    pub use crate::stream::client::rpc::{InMemoryResponse, Request, Response};
}

// This trait is a solution to abstract handshake_with_entry,
// local_addr, and map methods
#[allow(async_fn_in_trait)]
pub trait Handshake: Clone {
    /// Handshake with the remote peer
    async fn handshake_with_entry(
        &self,
        remote_handshake_addr: SocketAddr,
        server_name: Name,
    ) -> std::io::Result<(secret::map::Peer, secret::HandshakeKind)>;

    fn local_addr(&self) -> std::io::Result<SocketAddr>;

    fn map(&self) -> &secret::Map;
}

impl Handshake for crate::psk::client::Provider {
    async fn handshake_with_entry(
        &self,
        remote_handshake_addr: SocketAddr,
        server_name: Name,
    ) -> std::io::Result<(secret::map::Peer, secret::HandshakeKind)> {
        self.handshake_with_entry(remote_handshake_addr, server_name)
            .await
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.local_addr()
    }

    fn map(&self) -> &secret::Map {
        self.map()
    }
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
        server_name: Name,
    ) -> io::Result<secret::HandshakeKind> {
        let (_peer, kind) = self
            .handshake
            .handshake_with_entry(remote_handshake_addr, server_name)
            .await?;
        Ok(kind)
    }

    #[inline]
    async fn handshake_for_connect(
        &self,
        remote_handshake_addr: SocketAddr,
        server_name: Name,
    ) -> io::Result<secret::map::Peer> {
        let (peer, _kind) = self
            .handshake
            .handshake_with_entry(remote_handshake_addr, server_name)
            .await?;
        Ok(peer)
    }

    /// Connects using the preferred protocol
    #[inline]
    pub async fn connect(
        &self,
        handshake_addr: SocketAddr,
        acceptor_addr: SocketAddr,
        server_name: Name,
    ) -> io::Result<Stream<S>> {
        match self.default_protocol {
            socket::Protocol::Udp => {
                self.connect_udp(handshake_addr, acceptor_addr, server_name)
                    .await
            }
            socket::Protocol::Tcp => {
                self.connect_tcp(handshake_addr, acceptor_addr, server_name)
                    .await
            }
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
        server_name: Name,
    ) -> io::Result<Res::Output>
    where
        Req: rpc::Request,
        Res: rpc::Response,
    {
        match self.default_protocol {
            socket::Protocol::Udp => {
                self.rpc_udp(
                    handshake_addr,
                    acceptor_addr,
                    request,
                    response,
                    server_name,
                )
                .await
            }
            socket::Protocol::Tcp => {
                self.rpc_tcp(
                    handshake_addr,
                    acceptor_addr,
                    request,
                    response,
                    server_name,
                )
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
        server_name: Name,
    ) -> io::Result<Stream<S>> {
        // ensure we have a secret for the peer
        let handshake = self.handshake_for_connect(handshake_addr, server_name);

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
        server_name: Name,
    ) -> io::Result<Res::Output>
    where
        Req: rpc::Request,
        Res: rpc::Response,
    {
        // ensure we have a secret for the peer
        let handshake = self.handshake_for_connect(handshake_addr, server_name);

        let stream = client::connect_udp(handshake, acceptor_addr, &self.env).await?;
        rpc_internal::from_stream(stream, request, response).await
    }

    /// Connects using the TCP transport layer
    #[inline]
    pub async fn connect_tcp(
        &self,
        handshake_addr: SocketAddr,
        acceptor_addr: SocketAddr,
        server_name: Name,
    ) -> io::Result<Stream<S>> {
        // ensure we have a secret for the peer
        let handshake = self.handshake_for_connect(handshake_addr, server_name);

        let mut stream =
            client::connect_tcp(handshake, acceptor_addr, &self.env, self.linger).await?;
        Self::write_prelude(&mut stream).await?;
        Ok(stream)
    }

    /// Connects using the TLS over TCP transport layer.
    ///
    /// Note that the handshake and acceptor addresses must be the same for TLS.
    #[inline]
    pub async fn connect_tls(
        &self,
        addr: SocketAddr,
        server_name: Name,
        config: &s2n_tls::config::Config,
    ) -> io::Result<Stream<S>> {
        let stream = client::connect_tls(
            addr,
            server_name,
            config,
            &self.env,
            self.linger,
            self.handshake.map(),
        )
        .await?;
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
        server_name: Name,
    ) -> io::Result<Res::Output>
    where
        Req: rpc::Request,
        Res: rpc::Response,
    {
        // ensure we have a secret for the peer
        let handshake = self.handshake_for_connect(handshake_addr, server_name);

        let stream = client::connect_tcp(handshake, acceptor_addr, &self.env, self.linger).await?;
        rpc_internal::from_stream(stream, request, response).await
    }

    /// Connects with a pre-existing TCP stream
    #[inline]
    pub async fn connect_tcp_with(
        &self,
        handshake_addr: SocketAddr,
        stream: TcpStream,
        server_name: Name,
    ) -> io::Result<Stream<S>> {
        // ensure we have a secret for the peer
        let handshake = self
            .handshake_for_connect(handshake_addr, server_name)
            .await?;

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
    start: Timestamp,
    reason: Option<StreamTcpConnectErrorReason>,
}

impl<S: event::Subscriber + Clone> Drop for DropGuard<'_, S> {
    fn drop(&mut self) {
        if let Some(reason) = self.reason.take() {
            let now = self.env.clock().get_time();
            self.env
                .endpoint_publisher_with_time(now)
                .on_stream_connect_error(event::builder::StreamConnectError {
                    reason,
                    latency: now.saturating_duration_since(self.start),
                });
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
    let start = env.clock().get_time();
    // This emits the error event in case this future gets dropped.
    let mut guard = DropGuard {
        env,
        reason: Some(StreamTcpConnectErrorReason::AbortedPendingBoth),
        start,
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
    while (socket.is_none() || peer.is_none()) && error.is_none() {
        tokio::select! {
            connected = &mut connect, if socket.is_none() => {
                let now = env.clock().get_time();
                env.endpoint_publisher_with_time(now).on_stream_tcp_connect(event::builder::StreamTcpConnect {
                    error: connected.is_err(),
                    latency: now.saturating_duration_since(start),
                });
                match connected {
                    Ok(v) => {
                        socket = Some(Ok(v));
                        guard.reason = match guard.reason.clone() {
                            Some(StreamTcpConnectErrorReason::AbortedPendingBoth) => Some(
                                StreamTcpConnectErrorReason::AbortedPendingHandshake
                            ),
                            other => other,
                        };
                    },
                    Err(e) => {
                        guard.reason = Some(StreamTcpConnectErrorReason::TcpConnect);
                        error = Some(e);
                        socket = Some(Err(()));
                    }
                }
            }
            handshaked = &mut handshake, if peer.is_none() => {
                match handshaked {
                    Ok(v) => {
                        peer = Some(Ok(v));
                        guard.reason = match guard.reason.clone() {
                            Some(StreamTcpConnectErrorReason::AbortedPendingBoth) => Some(
                                StreamTcpConnectErrorReason::AbortedPendingConnect
                            ),
                            other => other,
                        };
                    },
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
        #[allow(deprecated)]
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

/// Connects and negotiated TLS 1.3
#[inline]
pub async fn connect_tls<Sub>(
    addr: SocketAddr,
    server_name: Name,
    config: &s2n_tls::config::Config,
    env: &Environment<Sub>,
    linger: Option<Duration>,
    // FIXME: Do we really need the map for this?
    map: &crate::path::secret::Map,
) -> io::Result<Stream<Sub>>
where
    Sub: event::Subscriber + Clone,
{
    let socket = TcpStream::connect(addr).await?;

    // Make sure TCP_NODELAY is set
    let _ = socket.set_nodelay(true);

    if linger.is_some() {
        #[allow(deprecated)]
        let _ = socket.set_linger(linger);
    }

    use s2n_tls::connection::Builder as _;
    let mut connection = config.build_connection(s2n_tls::enums::Mode::Client)?;
    connection.set_server_name(&server_name)?;

    let socket = Arc::new(crate::stream::socket::application::Single(socket));
    let mut connection =
        crate::stream::tls::S2nTlsConnection::from_connection(socket.clone(), connection)?;

    connection.negotiate().await?;

    // The handshake is complete at this point, so the stream should be considered open. Eventually
    // at this point we'll want to export the TLS keys from the connection and add those into the
    // state below. Right now though we're continuing to use s2n-tls for maintaining relevant
    // state.

    // if the ip isn't known, then ask the socket to resolve it for us
    let peer_addr = if addr.ip().is_unspecified() {
        socket.0.peer_addr()?
    } else {
        addr
    };

    let stream_id = crate::packet::stream::Id {
        queue_id: VarInt::ZERO,
        is_reliable: true,
        is_bidirectional: true,
    };

    let params = s2n_quic_core::dc::ApplicationParams::new(
        1 << 14,
        &Default::default(),
        &Default::default(),
    );

    let meta = event::api::ConnectionMeta {
        id: 0, // TODO use an actual connection ID
        timestamp: env.clock().get_time().into_event(),
    };
    let info = event::api::ConnectionInfo {};

    let subscriber = env.subscriber().clone();
    let subscriber_ctx = subscriber.create_connection_context(&meta, &info);

    // Fake up a secret -- this will need some reworking to store the keys in the TLS state
    // probably?
    let mut secret = [0; 32];
    aws_lc_rs::rand::fill(&mut secret).unwrap();
    let secret = crate::path::secret::schedule::Secret::new(
        crate::path::secret::schedule::Ciphersuite::AES_GCM_128_SHA256,
        s2n_quic_core::dc::SUPPORTED_VERSIONS[0],
        s2n_quic_core::endpoint::Type::Client,
        &secret,
    );

    let common = {
        let application = crate::stream::send::application::state::State { is_reliable: true };

        let fixed = crate::stream::shared::FixedValues {
            remote_ip: UnsafeCell::new(peer_addr.ip().into()),
            application: UnsafeCell::new(application),
            credentials: UnsafeCell::new(crate::credentials::Credentials {
                id: crate::credentials::Id::from([1; 16]),
                key_id: VarInt::ZERO,
            }),
        };

        crate::stream::shared::Common {
            clock: env.clock().clone(),
            gso: env.gso(),
            remote_port: peer_addr.port().into(),
            remote_queue_id: stream_id.queue_id.as_u64().into(),
            local_queue_id: u64::MAX.into(),
            last_peer_activity: Default::default(),
            fixed,
            closed_halves: 0u8.into(),
            subscriber: crate::stream::shared::Subscriber {
                subscriber,
                context: subscriber_ctx,
            },
            s2n_connection: Some(connection),
        }
    };

    let pair = crate::path::secret::map::ApplicationPair::new(
        &secret,
        VarInt::ZERO,
        crate::path::secret::schedule::Initiator::Local,
        // Not currently actually using these credentials.
        crate::path::secret::map::Dedup::disabled(),
    );
    let shared = Arc::new(crate::stream::shared::Shared {
        receiver: crate::stream::recv::shared::State::new(
            stream_id,
            &params,
            crate::stream::TransportFeatures::TCP,
            crate::stream::recv::shared::RecvBuffer::A(recv::buffer::Local::new(
                // FIXME: Maybe use a larger buffer fitting the TLS record size (14kb)?
                msg::recv::Message::new(9000),
                None,
            )),
            s2n_quic_core::endpoint::Type::Client,
            &env.clock(),
        ),
        sender: crate::stream::send::shared::State::new(
            crate::stream::send::flow::non_blocking::State::new(VarInt::MAX),
            crate::stream::send::path::Info {
                max_datagram_size: params.max_datagram_size(),
                send_quantum: 10,
                ecn: ExplicitCongestionNotification::Ect0,
                next_expected_control_packet: VarInt::ZERO,
            },
            None,
        ),
        crypto: crate::stream::shared::Crypto::new(pair.sealer, pair.opener, None, map),
        common,
    });

    let read = crate::stream::recv::application::Builder::new(
        s2n_quic_core::endpoint::Type::Client,
        env.reader_rt(),
    );
    let write = crate::stream::send::application::Builder::new(env.writer_rt());

    let stream = crate::stream::application::Builder {
        read,
        write,
        shared,
        sockets: Box::new(socket),
        queue_time: env.clock().get_time(),
    };

    stream.build()
}

#[cfg(test)]
mod test {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn client_config() -> s2n_tls::config::Config {
        let mut client_config = s2n_tls::config::Builder::new();
        client_config
            .with_system_certs(false)
            .unwrap()
            .trust_pem(s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.as_bytes())
            .unwrap()
            .load_pem(
                s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.as_bytes(),
                s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.as_bytes(),
            )
            .unwrap();
        client_config.build().unwrap()
    }

    fn server_config() -> s2n_tls::config::Config {
        let mut server_config = s2n_tls::config::Builder::new();
        server_config
            .with_system_certs(false)
            .unwrap()
            .set_client_auth_type(s2n_tls::enums::ClientAuthType::Required)
            .unwrap()
            .set_verify_host_callback(VerifyHostNameClientCertVerifier::new("qlaws.qlaws"))
            .unwrap()
            .load_pem(
                s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.as_bytes(),
                s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.as_bytes(),
            )
            .unwrap()
            .trust_pem(s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.as_bytes())
            .unwrap();
        server_config.build().unwrap()
    }

    fn dc_client() -> super::Client<DummyHandshake, crate::event::tracing::Subscriber> {
        let handshake = DummyHandshake {
            map: crate::path::secret::Map::new(
                crate::path::secret::stateless_reset::Signer::new(b"default"),
                50,
                false,
                s2n_quic_core::time::StdClock::default(),
                crate::event::disabled::Subscriber::default(),
            ),
        };
        super::Client::new(handshake, crate::event::tracing::Subscriber::default()).unwrap()
    }

    async fn check(message: &[u8]) {
        let client_config = client_config();
        let server_config = server_config();

        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let acceptor = s2n_tls_tokio::TlsAcceptor::new(server_config);
            while let Ok((conn, _)) = server.accept().await {
                conn.set_nodelay(true).unwrap();

                let mut stream = acceptor.accept(conn).await.unwrap();
                let mut buffer = vec![];
                stream.read_to_end(&mut buffer).await.unwrap();
                eprintln!("server read {} bytes", buffer.len());
                stream.write_all(&buffer).await.unwrap();
                eprintln!("server finished writing {} bytes", buffer.len());
                stream.flush().await.unwrap();
                stream.shutdown().await.unwrap();
                drop(stream);
            }
        });

        let client = dc_client();
        let stream = client
            .connect_tls(
                server_addr,
                super::Name::from_static("qlaws.qlaws"),
                &client_config,
            )
            .await
            .unwrap();
        let (mut reader, mut writer) = stream.into_split();

        writer.write_all_from(&mut &message[..]).await.unwrap();
        eprintln!("finished writing");

        writer.shutdown().unwrap();

        eprintln!("writer.shutdown() done");

        let mut buffer: Vec<u8> = vec![];
        reader.read_to_end(&mut buffer).await.unwrap();
        assert_eq!(buffer, message);
    }

    #[tokio::test]
    async fn short() {
        check(&b"testing"[..]).await;
    }

    #[tokio::test]
    async fn medium() {
        let message = vec![0x3; 1024 * 1024];
        check(&message).await;
    }

    #[tokio::test]
    async fn large() {
        let message = vec![0x3; 50 * 1024 * 1024];
        check(&message).await;
    }

    #[tokio::test]
    async fn closed_during_handshake() {
        let client_config = client_config();

        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((conn, _)) = server.accept().await {
                drop(conn);
            }
        });

        let client = dc_client();
        let err = client
            .connect_tls(
                server_addr,
                super::Name::from_static("qlaws.qlaws"),
                &client_config,
            )
            .await
            .expect_err("handshake failed");
        let err = format!("{:?}", err);
        assert!(err.contains("Connection reset by peer"), "{}", err);
    }

    #[tokio::test]
    async fn incorrect_record_after_handshake() {
        let client_config = client_config();
        let server_config = server_config();

        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let acceptor = s2n_tls_tokio::TlsAcceptor::new(server_config);
            while let Ok((conn, _)) = server.accept().await {
                conn.set_nodelay(true).unwrap();

                let mut stream = acceptor.accept(conn).await.unwrap();
                let mut buffer = vec![];
                stream.read_to_end(&mut buffer).await.unwrap();

                // Bypass the s2n-tls wrapper and write raw bytes to the stream. This confirms the
                // receiver correctly handles closing the stream.
                stream.get_mut().write_all(&buffer).await.unwrap();
                stream.get_mut().flush().await.unwrap();
                stream.get_mut().shutdown().await.unwrap();

                drop(stream);
            }
        });

        let client = dc_client();
        let stream = client
            .connect_tls(
                server_addr,
                super::Name::from_static("qlaws.qlaws"),
                &client_config,
            )
            .await
            .unwrap();

        let message = [0x3; 1024];
        let (mut reader, mut writer) = stream.into_split();

        writer.write_all_from(&mut &message[..]).await.unwrap();
        eprintln!("finished writing");

        writer.shutdown().unwrap();

        eprintln!("writer.shutdown() done");

        let mut buffer: Vec<u8> = vec![];
        let err = reader.read_to_end(&mut buffer).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "{:?}", err);
    }

    #[tokio::test]
    async fn unauthenticated_closure() {
        let client_config = client_config();
        let server_config = server_config();

        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let acceptor = s2n_tls_tokio::TlsAcceptor::new(server_config);
            while let Ok((conn, _)) = server.accept().await {
                conn.set_nodelay(true).unwrap();

                let mut stream = acceptor.accept(conn).await.unwrap();
                let mut buffer = vec![];
                stream.read_to_end(&mut buffer).await.unwrap();

                // Directly close the stream without shutting it down.
                stream.get_mut().flush().await.unwrap();
                stream.get_mut().shutdown().await.unwrap();

                // Ensure the Drop impl can't write anything either. This does leak the fd and some
                // memory but we're OK with that in test code.
                std::mem::forget(stream);
            }
        });

        let client = dc_client();
        let stream = client
            .connect_tls(
                server_addr,
                super::Name::from_static("qlaws.qlaws"),
                &client_config,
            )
            .await
            .unwrap();

        let message = [0x3; 1024];
        let (mut reader, mut writer) = stream.into_split();

        writer.write_all_from(&mut &message[..]).await.unwrap();
        eprintln!("finished writing");

        writer.shutdown().unwrap();

        eprintln!("writer.shutdown() done");

        let mut buffer: Vec<u8> = vec![];
        let err = reader.read_to_end(&mut buffer).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof, "{:?}", err);
    }

    #[derive(Clone)]
    struct DummyHandshake {
        map: crate::path::secret::Map,
    }

    impl super::Handshake for DummyHandshake {
        async fn handshake_with_entry(
            &self,
            _remote_handshake_addr: std::net::SocketAddr,
            _server_name: s2n_quic::server::Name,
        ) -> std::io::Result<(
            crate::path::secret::map::Peer,
            crate::path::secret::HandshakeKind,
        )> {
            todo!()
        }

        fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
            Ok(std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 1111).into())
        }

        fn map(&self) -> &crate::path::secret::Map {
            &self.map
        }
    }

    pub struct VerifyHostNameClientCertVerifier {
        host_name: String,
    }

    impl s2n_tls::callbacks::VerifyHostNameCallback for VerifyHostNameClientCertVerifier {
        fn verify_host_name(&self, host_name: &str) -> bool {
            self.host_name == host_name
        }
    }

    impl VerifyHostNameClientCertVerifier {
        pub fn new(host_name: impl ToString) -> VerifyHostNameClientCertVerifier {
            VerifyHostNameClientCertVerifier {
                host_name: host_name.to_string(),
            }
        }
    }
}
