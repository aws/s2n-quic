// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::accept;
use crate::{
    either::Either,
    event::{self, EndpointPublisher, IntoEvent},
    msg,
    path::secret,
    stream::{
        endpoint,
        environment::tokio::{self as env, Environment},
        recv, server,
        socket::Socket,
    },
};
use core::{
    ops::ControlFlow,
    pin::Pin,
    task::{self, Poll},
    time::Duration,
};
use s2n_codec::DecoderError;
use s2n_quic_core::{
    inet::SocketAddress,
    ready,
    time::{Clock, Timestamp},
};
use std::io;
use tokio::{io::AsyncWrite as _, net::TcpStream};
use tracing::debug;

pub struct Context<Sub>
where
    Sub: event::Subscriber + Clone,
{
    recv_buffer: msg::recv::Message,
    sender: accept::Sender<Sub>,
    env: Environment<Sub>,
    secrets: secret::Map,
    accept_flavor: accept::Flavor,
    local_port: u16,
}

impl<Sub> Context<Sub>
where
    Sub: event::Subscriber + Clone,
{
    #[inline]
    pub fn new(acceptor: &super::Acceptor<Sub>) -> Self {
        Self {
            recv_buffer: msg::recv::Message::new(u16::MAX),
            sender: acceptor.sender.clone(),
            env: acceptor.env.clone(),
            secrets: acceptor.secrets.clone(),
            accept_flavor: acceptor.accept_flavor,
            local_port: acceptor.socket.local_addr().unwrap().port(),
        }
    }
}

pub struct Worker<Sub>
where
    Sub: event::Subscriber + Clone,
{
    queue_time: Timestamp,
    stream: Option<(TcpStream, SocketAddress)>,
    subscriber_ctx: Option<Sub::ConnectionContext>,
    state: WorkerState,
}

impl<Sub> Worker<Sub>
where
    Sub: event::Subscriber + Clone,
{
    #[inline]
    pub fn new(now: Timestamp) -> Self {
        Self {
            queue_time: now,
            stream: None,
            subscriber_ctx: None,
            state: WorkerState::Init,
        }
    }
}

impl<Sub> super::manager::Worker for Worker<Sub>
where
    Sub: event::Subscriber + Clone,
{
    type ConnectionContext = Sub::ConnectionContext;
    type Stream = TcpStream;
    type Context = Context<Sub>;

    #[inline]
    fn replace<Pub, C>(
        &mut self,
        remote_address: SocketAddress,
        stream: TcpStream,
        linger: Option<Duration>,
        subscriber_ctx: Self::ConnectionContext,
        publisher: &Pub,
        clock: &C,
    ) where
        Pub: EndpointPublisher,
        C: Clock,
    {
        // Make sure TCP_NODELAY is set
        let _ = stream.set_nodelay(true);

        if linger.is_some() {
            let _ = stream.set_linger(linger);
        }

        let now = clock.get_time();

        let prev_queue_time = core::mem::replace(&mut self.queue_time, now);
        let prev_state = core::mem::replace(&mut self.state, WorkerState::Init);
        let prev_stream = core::mem::replace(&mut self.stream, Some((stream, remote_address)));
        let prev_ctx = core::mem::replace(&mut self.subscriber_ctx, Some(subscriber_ctx));

        if let Some(remote_address) = prev_stream.map(|(socket, remote_address)| {
            // If linger wasn't already set or it was set to a value other than 0, then override it
            if linger.is_none() || linger != Some(Duration::ZERO) {
                // close the stream immediately and send a reset to the client
                let _ = socket.set_linger(Some(Duration::ZERO));
            }
            remote_address
        }) {
            let sojourn_time = now.saturating_duration_since(prev_queue_time);
            let buffer_len = match prev_state {
                WorkerState::Init => 0,
                WorkerState::Buffering { buffer, .. } => buffer.payload_len(),
                WorkerState::Erroring { .. } => 0,
            };
            publisher.on_acceptor_tcp_stream_replaced(event::builder::AcceptorTcpStreamReplaced {
                remote_address: &remote_address,
                sojourn_time,
                buffer_len,
            });
        }

        if let Some(ctx) = prev_ctx {
            // TODO emit an event
            let _ = ctx;
        }
    }

    #[inline]
    fn poll<Pub, C>(
        &mut self,
        task_cx: &mut task::Context,
        context: &mut Context<Sub>,
        publisher: &Pub,
        clock: &C,
    ) -> Poll<Result<ControlFlow<()>, Option<io::Error>>>
    where
        Pub: EndpointPublisher,
        C: Clock,
    {
        // if we don't have a stream then it's a bug in the worker impl - in production just return
        // `Ready`, which will correct the state
        if self.stream.is_none() {
            debug_assert!(
                false,
                "Worker::poll should only be called with an active socket"
            );
            return Poll::Ready(Ok(ControlFlow::Continue(())));
        }

        // make sure another worker didn't leave around a buffer
        context.recv_buffer.clear();

        let res = ready!(self.state.poll(
            task_cx,
            context,
            &mut self.stream,
            &mut self.subscriber_ctx,
            self.queue_time,
            clock.get_time(),
            publisher,
        ));

        // if we're ready then reset the worker
        self.state = WorkerState::Init;
        self.stream = None;

        if let Some(ctx) = self.subscriber_ctx.take() {
            // TODO emit event on the context
            let _ = ctx;
        }

        Poll::Ready(res)
    }

    #[inline]
    fn queue_time(&self) -> Timestamp {
        self.queue_time
    }

    #[inline]
    fn is_active(&self) -> bool {
        let is_active = self.stream.is_some();
        if !is_active {
            debug_assert!(matches!(self.state, WorkerState::Init));
            debug_assert!(self.subscriber_ctx.is_none());
        }
        is_active
    }
}

#[derive(Debug)]
enum WorkerState {
    /// Worker is waiting for a packet
    Init,
    /// Worker received a partial packet and is waiting on more data
    Buffering {
        buffer: msg::recv::Message,
        /// The number of times we got Pending from the `recv` call
        blocked_count: usize,
    },
    /// Worker encountered an error and is trying to send a response
    Erroring {
        offset: usize,
        buffer: Vec<u8>,
        error: io::Error,
    },
}

impl WorkerState {
    fn poll<Sub, Pub>(
        &mut self,
        cx: &mut task::Context,
        context: &mut Context<Sub>,
        stream: &mut Option<(TcpStream, SocketAddress)>,
        subscriber_ctx: &mut Option<Sub::ConnectionContext>,
        queue_time: Timestamp,
        now: Timestamp,
        publisher: &Pub,
    ) -> Poll<Result<ControlFlow<()>, Option<io::Error>>>
    where
        Sub: event::Subscriber + Clone,
        Pub: EndpointPublisher,
    {
        let sojourn_time = now.saturating_duration_since(queue_time);

        loop {
            // figure out where to put the received bytes
            let (recv_buffer, blocked_count) = match self {
                // borrow the context's recv buffer initially
                WorkerState::Init => (&mut context.recv_buffer, 0),
                // we have our own recv buffer to use
                WorkerState::Buffering {
                    buffer,
                    blocked_count,
                } => (buffer, *blocked_count),
                // we encountered an error so try and send it back
                WorkerState::Erroring { offset, buffer, .. } => {
                    let (stream, _remote_address) = stream.as_mut().unwrap();
                    let len = ready!(Pin::new(stream).poll_write(cx, &buffer[*offset..]))?;

                    *offset += len;

                    // if we still need to send part of the buffer then loop back around
                    if *offset < buffer.len() {
                        continue;
                    }

                    // io::Error doesn't implement clone so we have to take the error to return it
                    let WorkerState::Erroring { error, .. } = core::mem::replace(self, Self::Init)
                    else {
                        unreachable!()
                    };

                    return Err(Some(error)).into();
                }
            };

            // try to read an initial packet from the socket
            let res = {
                let (stream, remote_address) = stream.as_mut().unwrap();
                Self::poll_initial_packet(
                    cx,
                    stream,
                    remote_address,
                    recv_buffer,
                    sojourn_time,
                    publisher,
                )
            };

            let Poll::Ready(res) = res else {
                // if we got `Pending` but we don't own the recv buffer then we need to copy it
                // into the worker so we can resume where we left off last time
                if blocked_count == 0 {
                    let buffer = recv_buffer.take();
                    *self = Self::Buffering {
                        buffer,
                        blocked_count,
                    };
                }

                if let Self::Buffering { blocked_count, .. } = self {
                    *blocked_count += 1;
                }

                return Poll::Pending;
            };

            let initial_packet = res?;

            let subscriber_ctx = subscriber_ctx.take().unwrap();
            let (socket, remote_address) = stream.take().unwrap();

            let recv_buffer = recv::buffer::Local::new(recv_buffer.take(), None);
            let recv_buffer = Either::A(recv_buffer);

            let peer = env::tcp::Reregistered {
                socket,
                peer_addr: remote_address,
                local_port: context.local_port,
                recv_buffer,
            };

            let stream_builder = match endpoint::accept_stream(
                now,
                &context.env,
                peer,
                &initial_packet,
                &context.secrets,
                subscriber_ctx,
                None,
            ) {
                Ok(stream) => stream,
                Err(error) => {
                    if let Some(env::tcp::Reregistered { socket, .. }) = error.peer {
                        if !error.secret_control.is_empty() {
                            // if we need to send an error then update the state and loop back
                            // around
                            *stream = Some((socket, remote_address));
                            *self = WorkerState::Erroring {
                                offset: 0,
                                buffer: error.secret_control,
                                error: error.error,
                            };
                            continue;
                        } else {
                            // close the stream immediately and send a reset to the client
                            let _ = socket.set_linger(Some(Duration::ZERO));
                            drop(socket);
                        }
                    }
                    return Err(Some(error.error)).into();
                }
            };

            {
                let remote_address: SocketAddress = stream_builder.shared.remote_addr();
                let remote_address = &remote_address;
                let creds = stream_builder.shared.credentials();
                let credential_id = &*creds.id;
                let stream_id = creds.key_id.as_u64();
                publisher.on_acceptor_tcp_stream_enqueued(
                    event::builder::AcceptorTcpStreamEnqueued {
                        remote_address,
                        credential_id,
                        stream_id,
                        sojourn_time,
                        blocked_count,
                    },
                );
            }

            let res = match context.accept_flavor {
                accept::Flavor::Fifo => context.sender.send_back(stream_builder),
                accept::Flavor::Lifo => context.sender.send_front(stream_builder),
            };

            return Poll::Ready(Ok(match res {
                Ok(prev) => {
                    if let Some(stream) = prev {
                        stream.prune(
                            event::builder::AcceptorStreamPruneReason::AcceptQueueCapacityExceeded,
                        );
                    }
                    ControlFlow::Continue(())
                }
                Err(_err) => {
                    debug!("application accept queue dropped; shutting down");
                    ControlFlow::Break(())
                }
            }));
        }
    }

    #[inline]
    fn poll_initial_packet<Pub>(
        cx: &mut task::Context,
        stream: &mut TcpStream,
        remote_address: &SocketAddress,
        recv_buffer: &mut msg::recv::Message,
        sojourn_time: Duration,
        publisher: &Pub,
    ) -> Poll<Result<server::InitialPacket, Option<io::Error>>>
    where
        Pub: EndpointPublisher,
    {
        loop {
            if recv_buffer.payload_len() > 10_000 {
                publisher.on_acceptor_tcp_packet_dropped(
                    event::builder::AcceptorTcpPacketDropped {
                        remote_address,
                        reason: DecoderError::UnexpectedBytes(recv_buffer.payload_len())
                            .into_event(),
                        sojourn_time,
                    },
                );

                // close the stream immediately and send a reset to the client
                let _ = stream.set_linger(Some(Duration::ZERO));

                return Err(None).into();
            }

            let res = ready!(stream.poll_recv_buffer(cx, recv_buffer)).map_err(Some)?;

            match server::InitialPacket::peek(recv_buffer, 16) {
                Ok(packet) => {
                    publisher.on_acceptor_tcp_packet_received(
                        event::builder::AcceptorTcpPacketReceived {
                            remote_address,
                            credential_id: &*packet.credentials.id,
                            stream_id: packet.stream_id.into_varint().as_u64(),
                            payload_len: packet.payload_len,
                            is_fin: packet.is_fin,
                            is_fin_known: packet.is_fin_known,
                            sojourn_time,
                        },
                    );
                    return Ok(packet).into();
                }
                Err(err) => {
                    if matches!(err, DecoderError::UnexpectedEof(_)) && res > 0 {
                        // we don't have enough bytes buffered so try reading more
                        continue;
                    }

                    publisher.on_acceptor_tcp_packet_dropped(
                        event::builder::AcceptorTcpPacketDropped {
                            remote_address,
                            reason: err.into_event(),
                            sojourn_time,
                        },
                    );

                    // close the stream immediately and send a reset to the client
                    let _ = stream.set_linger(Some(Duration::ZERO));

                    return Err(None).into();
                }
            }
        }
    }
}
