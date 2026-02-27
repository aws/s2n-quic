// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! TLS server support.
//!
//! This implements handling incoming TLS connections. These are accepted on the same TCP port pool as
//! dcQUIC streams, and upon being detected as a TLS connection are forwarded into a separate
//! runtime (via [`TlsServer::spawn`]) for handshaking. Once the handshake completes the stream is
//! built and returned to the application via the normal `accept()`.
//!
//! As of writing, there's no throttling/limiting on accepted connections during handshaking,
//! unlike dcQUIC streams which will limit concurrency in the acceptor and remove slow handshaking
//! streams.
//!
//! All of this is also off by default since the TlsServer isn't built.

use super::accept;
use crate::{
    event,
    event::EndpointPublisher,
    path::secret,
    stream::environment::{tokio::Environment, Environment as _},
};
use s2n_quic_core::time::{Clock as _, Timestamp};
use s2n_tls::config::Config as S2nConfig;
use std::sync::Arc;
use tokio::net::TcpStream;

pub struct Builder {
    pub rt: Arc<tokio::runtime::Runtime>,
    pub config: S2nConfig,
}

impl Builder {
    pub(crate) fn build<Sub>(
        self,
        sender: accept::Sender<Sub>,
        env: Environment<Sub>,
        map: secret::Map,
        accept_flavor: accept::Flavor,
    ) -> TlsServer<Sub>
    where
        Sub: event::Subscriber + Clone,
    {
        TlsServer::new(self.rt, self.config, sender, env, map, accept_flavor)
    }
}

#[derive(Clone)]
pub struct TlsServer<Sub>
where
    Sub: event::Subscriber + Clone,
{
    rt: Option<Arc<tokio::runtime::Runtime>>,
    config: S2nConfig,
    sender: accept::Sender<Sub>,
    env: Environment<Sub>,
    map: secret::Map,
    accept_flavor: accept::Flavor,
}

impl<Sub> Drop for TlsServer<Sub>
where
    Sub: event::Subscriber + Clone,
{
    fn drop(&mut self) {
        if let Some(rt) = Arc::into_inner(self.rt.take().unwrap()) {
            rt.shutdown_background();
        }
    }
}

impl<Sub> TlsServer<Sub>
where
    Sub: event::Subscriber + Clone,
{
    pub fn new(
        rt: Arc<tokio::runtime::Runtime>,
        config: S2nConfig,
        sender: accept::Sender<Sub>,
        env: Environment<Sub>,
        map: secret::Map,
        accept_flavor: accept::Flavor,
    ) -> Self {
        TlsServer {
            rt: Some(rt),
            sender,
            config,
            env,
            map,
            accept_flavor,
        }
    }

    pub(crate) fn spawn(
        &self,
        socket: super::LazyBoundStream,
        remote_address: s2n_quic_core::inet::SocketAddress,
        buffer: crate::msg::recv::Message,
        kernel_start_time: Timestamp,
    ) {
        match self.spawn_inner(socket, remote_address, buffer, kernel_start_time) {
            Ok(()) => {}
            Err(error) => {
                self.env
                    .endpoint_publisher()
                    .on_acceptor_tcp_tls_stream_rejected(
                        event::builder::AcceptorTcpTlsStreamRejected {
                            remote_address: &remote_address,
                            sojourn_time: self
                                .env
                                .clock()
                                .get_time()
                                .saturating_duration_since(kernel_start_time),
                            error: &error.into(),
                        },
                    );
            }
        }
    }

    fn spawn_inner(
        &self,
        socket: super::LazyBoundStream,
        remote_addr: s2n_quic_core::inet::SocketAddress,
        buffer: crate::msg::recv::Message,
        kernel_start_time: Timestamp,
    ) -> Result<(), s2n_tls::error::Error> {
        let mut conn = s2n_tls::connection::Connection::new_server();
        conn.set_config(self.config.clone())?;
        conn.set_blinding(s2n_tls::enums::Blinding::SelfService)?;

        // Rather than cloning we can keep accessing them from `self` if we used poll-like
        // workers...
        let sender = self.sender.clone();
        let env = self.env.clone();
        let map = self.map.clone();
        let flavor = self.accept_flavor;
        // We should be tracking the spawned tasks and aborting them if they take too long to avoid
        // building up resources, similar to the worker implementation (via sojourn times or so)...
        self.rt.as_ref().unwrap().spawn(async move {
            if let Err(error) = accept_conn(
                socket,
                remote_addr,
                buffer,
                conn,
                sender,
                &env,
                map,
                flavor,
                kernel_start_time,
            )
            .await
            {
                env.endpoint_publisher()
                    .on_acceptor_tcp_tls_stream_rejected(
                        event::builder::AcceptorTcpTlsStreamRejected {
                            remote_address: &remote_addr,
                            sojourn_time: env
                                .clock()
                                .get_time()
                                .saturating_duration_since(kernel_start_time),
                            error: &error,
                        },
                    );
            }
        });
        Ok(())
    }
}

async fn accept_conn<Sub: event::Subscriber + Clone>(
    socket: super::LazyBoundStream,
    remote_addr: s2n_quic_core::inet::SocketAddress,
    buffer: crate::msg::recv::Message,
    conn: s2n_tls::connection::Connection,
    sender: accept::Sender<Sub>,
    env: &Environment<Sub>,
    map: secret::Map,
    flavor: accept::Flavor,
    kernel_start_time: Timestamp,
) -> std::io::Result<()> {
    let socket = match socket {
        super::LazyBoundStream::Tokio(tcp_stream) => TcpStream::from_std(tcp_stream.into_std()?)?,
        super::LazyBoundStream::Std(tcp_stream) => TcpStream::from_std(tcp_stream)?,
        super::LazyBoundStream::TempEmpty => unreachable!(),
    };

    let socket = Arc::new(crate::stream::socket::application::Single(socket));
    let mut connection =
        crate::stream::tls::S2nTlsConnection::from_connection(socket.clone(), conn)?;

    connection.negotiate(Some(buffer)).await?;

    // The handshake is complete at this point, so the stream should be considered open. Eventually
    // at this point we'll want to export the TLS keys from the connection and add those into the
    // state below. Right now though we're continuing to use s2n-tls for maintaining relevant
    // state.

    let stream_builder = crate::stream::tls::build_stream(
        remote_addr.into(),
        socket,
        connection,
        env,
        &map,
        s2n_quic_core::endpoint::Type::Server,
    )?;

    {
        let remote_address: s2n_quic_core::inet::SocketAddress =
            stream_builder.shared.remote_addr();
        let remote_address = &remote_address;
        env.endpoint_publisher()
            .on_acceptor_tcp_tls_stream_enqueued(event::builder::AcceptorTcpTlsStreamEnqueued {
                remote_address,
                sojourn_time: env
                    .clock()
                    .get_time()
                    .saturating_duration_since(kernel_start_time),
            });
    }

    let res = match flavor {
        accept::Flavor::Fifo => sender.send_back(stream_builder),
        accept::Flavor::Lifo => sender.send_front(stream_builder),
    };
    match res {
        Ok(prev) => {
            if let Some(stream) = prev {
                stream
                    .prune(event::builder::AcceptorStreamPruneReason::AcceptQueueCapacityExceeded);
            }
        }
        Err(_err) => {
            // Consider failing to send the stream as OK. This typically means the application is
            // shutting down so there's not much more for us to do here.
        }
    }

    Ok(())
}

/// Is the provided buffer a TLS ClientHello?
///
/// This is intended primarily to distinguish between dcQUIC stream packets and TLS records
/// containing a ClientHello.
///
/// Returns:
///
/// * `None` if we don't know yet
/// * `Some(true)` if we're confident it's not a dcQUIC stream (and probably is a ClientHello record)
/// * `Some(false)` if we're confident it's not a TLS ClientHello
pub fn is_client_hello(buffer: &[u8]) -> Option<bool> {
    //= https://www.rfc-editor.org/rfc/rfc8446#section-5.1
    //# handshake(22),
    const HANDSHAKE_TAG: u8 = 22;

    // The TLS record's ContentType must be handshake for a ClientHello.
    //
    // A dcQUIC stream packet tag with value 22 decodes to be a recovery packet, which can
    // never appear in dcQUIC streams over TCP. This assertion confirms that's the case.
    const _: () = {
        assert!(crate::packet::stream::Tag::IS_RECOVERY_PACKET & HANDSHAKE_TAG != 0);
    };

    match buffer.first().copied() {
        Some(HANDSHAKE_TAG) => Some(true),
        Some(_) => Some(false),
        None => None,
    }
}
