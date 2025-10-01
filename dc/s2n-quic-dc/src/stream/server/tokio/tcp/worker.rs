// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{accept, LazyBoundStream};
use crate::{
    either::Either,
    event::{self, EndpointPublisher, IntoEvent},
    msg,
    path::secret,
    stream::{
        endpoint,
        environment::tokio::{self as env, Environment},
        recv, server, TransportFeatures,
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
use tracing::debug;

pub struct Context<Sub, B>
where
    Sub: event::Subscriber + Clone,
    B: PollBehavior<Sub>,
{
    recv_buffer: msg::recv::Message,
    sender: accept::Sender<Sub>,
    env: Environment<Sub>,
    secrets: secret::Map,
    accept_flavor: accept::Flavor,
    local_port: u16,
    _phantom: std::marker::PhantomData<B>,
}

impl<Sub, B> Context<Sub, B>
where
    Sub: event::Subscriber + Clone,
    B: PollBehavior<Sub> + Clone,
{
    #[inline]
    pub fn new(acceptor: &super::Acceptor<Sub, B>) -> Self {
        Self {
            recv_buffer: msg::recv::Message::new(u16::MAX),
            sender: acceptor.sender.clone(),
            env: acceptor.env.clone(),
            secrets: acceptor.secrets.clone(),
            accept_flavor: acceptor.accept_flavor,
            local_port: acceptor.socket.get_ref().local_addr().unwrap().port(),
            _phantom: std::marker::PhantomData::<B>,
        }
    }
}

pub struct Worker<Sub, B>
where
    Sub: event::Subscriber + Clone,
    B: PollBehavior<Sub>,
{
    queue_time: Timestamp,
    stream: Option<(LazyBoundStream, SocketAddress)>,
    subscriber_ctx: Option<Sub::ConnectionContext>,
    state: WorkerState,
    poll_behavior: B,
}

impl<Sub, B> Worker<Sub, B>
where
    Sub: event::Subscriber + Clone,
    B: PollBehavior<Sub>,
{
    #[inline]
    pub fn new(now: Timestamp, poll_behavior: B) -> Self {
        Self {
            queue_time: now,
            stream: None,
            subscriber_ctx: None,
            state: WorkerState::Init,
            poll_behavior,
        }
    }
}

impl<Sub, B> super::manager::Worker for Worker<Sub, B>
where
    Sub: event::Subscriber + Clone,
    B: PollBehavior<Sub>,
{
    type ConnectionContext = Sub::ConnectionContext;
    type Stream = LazyBoundStream;
    type Context = Context<Sub, B>;

    #[inline]
    fn replace<Pub, C>(
        &mut self,
        remote_address: SocketAddress,
        stream: LazyBoundStream,
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
        let prev_stream = self.stream.replace((stream, remote_address));
        let prev_ctx = self.subscriber_ctx.replace(subscriber_ctx);

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
        context: &mut Context<Sub, B>,
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

        let res = ready!(self.state.poll::<Sub, Pub, B>(
            task_cx,
            context,
            &mut self.stream,
            &mut self.subscriber_ctx,
            self.queue_time,
            clock.get_time(),
            publisher,
            &self.poll_behavior
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

pub trait PollBehavior<Sub>
where
    Sub: event::Subscriber + Clone,
{
    fn poll<Pub>(
        &self,
        state: &mut WorkerState,
        cx: &mut task::Context,
        context: &mut Context<Sub, Self>,
        stream: &mut Option<(LazyBoundStream, SocketAddress)>,
        subscriber_ctx: &mut Option<Sub::ConnectionContext>,
        queue_time: Timestamp,
        now: Timestamp,
        publisher: &Pub,
    ) -> Poll<Result<ControlFlow<()>, Option<io::Error>>>
    where
        Pub: EndpointPublisher,
        Self: Sized;
}

#[derive(Debug)]
pub enum WorkerState {
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
    fn poll<Sub, Pub, B>(
        &mut self,
        cx: &mut task::Context,
        context: &mut Context<Sub, B>,
        stream: &mut Option<(LazyBoundStream, SocketAddress)>,
        subscriber_ctx: &mut Option<Sub::ConnectionContext>,
        queue_time: Timestamp,
        now: Timestamp,
        publisher: &Pub,
        poll_behavior: &B,
    ) -> Poll<Result<ControlFlow<()>, Option<io::Error>>>
    where
        Sub: event::Subscriber + Clone,
        Pub: EndpointPublisher,
        B: PollBehavior<Sub>,
    {
        poll_behavior.poll(
            self,
            cx,
            context,
            stream,
            subscriber_ctx,
            queue_time,
            now,
            publisher,
        )
    }

    #[inline]
    fn poll_initial_packet<Pub>(
        cx: &mut task::Context,
        stream: &mut LazyBoundStream,
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

#[derive(Clone)]
pub struct DefaultBehavior;

impl<Sub> PollBehavior<Sub> for DefaultBehavior
where
    Sub: event::Subscriber + Clone,
{
    fn poll<Pub>(
        &self,
        state: &mut WorkerState,
        cx: &mut task::Context,
        context: &mut Context<Sub, Self>,
        stream: &mut Option<(LazyBoundStream, SocketAddress)>,
        subscriber_ctx: &mut Option<Sub::ConnectionContext>,
        queue_time: Timestamp,
        now: Timestamp,
        publisher: &Pub,
    ) -> Poll<Result<ControlFlow<()>, Option<io::Error>>>
    where
        Pub: EndpointPublisher,
    {
        let sojourn_time = now.saturating_duration_since(queue_time);

        loop {
            // figure out where to put the received bytes
            let (recv_buffer, blocked_count) = match state {
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
                    let WorkerState::Erroring { error, .. } =
                        core::mem::replace(state, WorkerState::Init)
                    else {
                        unreachable!()
                    };

                    return Err(Some(error)).into();
                }
            };

            // try to read an initial packet from the socket
            let res = {
                let (stream, remote_address) = stream.as_mut().unwrap();
                WorkerState::poll_initial_packet(
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
                    *state = WorkerState::Buffering {
                        buffer,
                        blocked_count,
                    };
                }

                if let WorkerState::Buffering { blocked_count, .. } = state {
                    *blocked_count += 1;
                }

                return Poll::Pending;
            };

            let initial_packet = res?;

            let subscriber_ctx = subscriber_ctx.take().unwrap();
            let (socket, remote_address) = stream.take().unwrap();

            let recv_buffer = recv::buffer::Local::new(recv_buffer.take(), None);
            let recv_buffer = Either::A(recv_buffer);

            let mut secret_control = vec![];

            let (crypto, parameters) = match endpoint::derive_stream_credentials(
                &initial_packet,
                &context.secrets,
                &TransportFeatures::TCP,
                &mut secret_control,
            ) {
                Ok(result) => result,
                Err(error) => {
                    if !secret_control.is_empty() {
                        *stream = Some((socket, remote_address));
                        *state = WorkerState::Erroring {
                            offset: 0,
                            buffer: secret_control,
                            error,
                        };
                        continue;
                    } else {
                        // Close socket immediately
                        let _ = socket.set_linger(Some(Duration::ZERO));
                        drop(socket);
                    }
                    return Err(Some(error)).into();
                }
            };

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
                crypto,
                parameters,
                secret_control,
            ) {
                Ok(stream) => stream,
                Err(error) => {
                    if let Some(env::tcp::Reregistered { socket, .. }) = error.peer {
                        if !error.secret_control.is_empty() {
                            // if we need to send an error then update the state and loop back
                            // around
                            *stream = Some((socket, remote_address));
                            *state = WorkerState::Erroring {
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
}
