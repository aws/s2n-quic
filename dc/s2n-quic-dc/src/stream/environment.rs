// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock,
    crypto::encrypt::Key as _,
    msg, packet,
    path::secret::Map,
    random::Random,
    stream::{
        application, recv, runtime,
        send::{self, flow},
        server, shared, socket, TransportFeatures,
    },
};
use core::{cell::UnsafeCell, future::Future};
use s2n_quic_core::{
    dc, endpoint,
    inet::{ExplicitCongestionNotification, SocketAddress},
    varint::VarInt,
};
use s2n_quic_platform::features;
use std::{io, sync::Arc};
use tracing::{debug_span, Instrument as _};

type Result<T = (), E = io::Error> = core::result::Result<T, E>;

#[cfg(feature = "tokio")]
pub mod tokio;

pub trait Environment {
    type Clock: Clone + clock::Clock;

    fn clock(&self) -> &Self::Clock;
    fn gso(&self) -> features::Gso;
    fn reader_rt(&self) -> runtime::ArcHandle;
    fn spawn_reader<F: 'static + Send + Future<Output = ()>>(&self, f: F);
    fn writer_rt(&self) -> runtime::ArcHandle;
    fn spawn_writer<F: 'static + Send + Future<Output = ()>>(&self, f: F);
}

pub struct SocketSet<S> {
    pub application: Box<dyn socket::application::Builder>,
    pub read_worker: Option<S>,
    pub write_worker: Option<S>,
    pub remote_addr: SocketAddress,
    pub source_control_port: u16,
    pub source_stream_port: Option<u16>,
}

pub trait Peer<E: Environment> {
    type WorkerSocket: socket::Socket;

    fn features(&self) -> TransportFeatures;
    fn with_source_control_port(&mut self, port: u16);
    fn setup(self, env: &E) -> Result<SocketSet<Self::WorkerSocket>>;
}

pub struct AcceptError<Peer> {
    pub secret_control: Vec<u8>,
    pub peer: Option<Peer>,
    pub error: io::Error,
}

pub struct Builder<E: Environment> {
    env: E,
}

impl<E: Environment> Builder<E> {
    #[inline]
    pub fn new(env: E) -> Self {
        Self { env }
    }

    #[inline]
    pub fn clock(&self) -> &E::Clock {
        self.env.clock()
    }

    #[inline]
    pub fn open_stream<P>(
        &self,
        handshake_addr: SocketAddress,
        peer: P,
        map: &Map,
        parameter_override: Option<&dyn Fn(dc::ApplicationParams) -> dc::ApplicationParams>,
    ) -> Result<application::Builder>
    where
        P: Peer<E>,
    {
        // derive secrets for the new stream
        let Some((sealer, opener, mut parameters)) = map.pair_for_peer(handshake_addr.into())
        else {
            // the application didn't perform a handshake with the server before opening the stream
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing credentials for server: {handshake_addr}"),
            ));
        };

        if let Some(o) = parameter_override {
            parameters = o(parameters);
        }

        // TODO get a flow ID. for now we'll use the sealer credentials
        let key_id = sealer.credentials().key_id;
        let stream_id = packet::stream::Id {
            key_id,
            is_reliable: true,
            is_bidirectional: true,
        };

        let crypto = shared::Crypto::new(sealer, opener, map);

        self.build_stream(
            peer,
            stream_id,
            None,
            crypto,
            parameters,
            None,
            None,
            endpoint::Type::Client,
        )
    }

    #[inline]
    pub fn accept_stream<P>(
        &self,
        mut peer: P,
        packet: &server::InitialPacket,
        handshake: Option<server::handshake::Receiver>,
        buffer: Option<&mut msg::recv::Message>,
        map: &Map,
        parameter_override: Option<&dyn Fn(dc::ApplicationParams) -> dc::ApplicationParams>,
    ) -> Result<application::Builder, AcceptError<P>>
    where
        P: Peer<E>,
    {
        let credentials = &packet.credentials;
        let mut secret_control = vec![];
        let Some((sealer, opener, mut parameters)) =
            map.pair_for_credentials(credentials, &mut secret_control)
        else {
            let error = io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing credentials for client: {credentials:?}"),
            );
            let error = AcceptError {
                secret_control,
                peer: Some(peer),
                error,
            };
            return Err(error);
        };

        if let Some(o) = parameter_override {
            parameters = o(parameters);
        }

        // inform the value of what the source_control_port is
        peer.with_source_control_port(packet.source_control_port);

        let crypto = shared::Crypto::new(sealer, opener, map);

        let res = self.build_stream(
            peer,
            packet.stream_id,
            packet.source_stream_port,
            crypto,
            parameters,
            handshake,
            buffer,
            endpoint::Type::Server,
        );

        match res {
            Ok(stream) => Ok(stream),
            Err(error) => {
                let error = AcceptError {
                    secret_control,
                    peer: None,
                    error,
                };
                Err(error)
            }
        }
    }

    #[inline]
    fn build_stream<P>(
        &self,
        peer: P,
        stream_id: packet::stream::Id,
        remote_stream_port: Option<u16>,
        crypto: shared::Crypto,
        parameters: dc::ApplicationParams,
        handshake: Option<server::handshake::Receiver>,
        recv_buffer: Option<&mut msg::recv::Message>,
        endpoint_type: endpoint::Type,
    ) -> Result<application::Builder>
    where
        P: Peer<E>,
    {
        let features = peer.features();

        let sockets = peer.setup(&self.env)?;

        // construct shared reader state
        let reader =
            recv::shared::State::new(stream_id, &parameters, handshake, features, recv_buffer);

        let writer = {
            let worker = sockets
                .write_worker
                .map(|socket| (send::state::State::new(stream_id, &parameters), socket));

            let (flow_offset, send_quantum, bandwidth) =
                if let Some((worker, _socket)) = worker.as_ref() {
                    let flow_offset = worker.flow_offset();
                    let send_quantum = worker.send_quantum_packets();
                    let bandwidth = Some(worker.cca.bandwidth());

                    (flow_offset, send_quantum, bandwidth)
                } else {
                    debug_assert!(
                        features.is_flow_controlled(),
                        "transports without flow control need background workers"
                    );

                    let flow_offset = VarInt::MAX;
                    let send_quantum = 10;
                    let bandwidth = None;

                    (flow_offset, send_quantum, bandwidth)
                };

            let flow = flow::non_blocking::State::new(flow_offset);

            let path = send::path::Info {
                max_datagram_size: parameters.max_datagram_size,
                send_quantum,
                ecn: ExplicitCongestionNotification::Ect0,
                next_expected_control_packet: VarInt::ZERO,
            };

            // construct shared writer state
            let state = send::shared::State::new(flow, path, bandwidth);

            (state, worker)
        };

        // construct shared common state between readers/writers
        let common = {
            let application = send::application::state::State {
                stream_id,
                source_control_port: sockets.source_control_port,
                source_stream_port: sockets.source_stream_port,
            };

            let fixed = shared::FixedValues {
                remote_ip: UnsafeCell::new(sockets.remote_addr.ip()),
                source_control_port: UnsafeCell::new(sockets.source_control_port),
                application: UnsafeCell::new(application),
            };

            let remote_port = sockets.remote_addr.port();
            let write_remote_port = remote_stream_port.unwrap_or(remote_port);

            shared::Common {
                clock: self.env.clock().clone(),
                gso: self.env.gso(),
                read_remote_port: remote_port.into(),
                write_remote_port: write_remote_port.into(),
                last_peer_activity: Default::default(),
                fixed,
                closed_halves: 0u8.into(),
            }
        };

        let shared = Arc::new(shared::Shared {
            receiver: reader,
            sender: writer.0,
            common,
            crypto,
        });

        // spawn the read worker
        if let Some(socket) = sockets.read_worker {
            let shared = shared.clone();

            let task = async move {
                let mut reader = recv::worker::Worker::new(socket, shared, endpoint_type);

                let mut prev_waker: Option<core::task::Waker> = None;
                core::future::poll_fn(|cx| {
                    // update the waker if needed
                    if prev_waker
                        .as_ref()
                        .map_or(true, |prev| !prev.will_wake(cx.waker()))
                    {
                        prev_waker = Some(cx.waker().clone());
                        reader.update_waker(cx);
                    }

                    // drive the reader to completion
                    reader.poll(cx)
                })
                .await;
            };

            let span = debug_span!("worker::read");

            if span.is_disabled() {
                self.env.spawn_reader(task);
            } else {
                self.env.spawn_reader(task.instrument(span));
            }
        }

        // spawn the write worker
        if let Some((worker, socket)) = writer.1 {
            let shared = shared.clone();

            let task = async move {
                let mut writer = send::worker::Worker::new(
                    socket,
                    Random::default(),
                    shared,
                    worker,
                    endpoint_type,
                );

                let mut prev_waker: Option<core::task::Waker> = None;
                core::future::poll_fn(|cx| {
                    // update the waker if needed
                    if prev_waker
                        .as_ref()
                        .map_or(true, |prev| !prev.will_wake(cx.waker()))
                    {
                        prev_waker = Some(cx.waker().clone());
                        writer.update_waker(cx);
                    }

                    // drive the writer to completion
                    writer.poll(cx)
                })
                .await;
            };

            let span = debug_span!("worker::write");

            if span.is_disabled() {
                self.env.spawn_writer(task);
            } else {
                self.env.spawn_writer(task.instrument(span));
            }
        }

        let read = recv::application::Builder::new(endpoint_type, self.env.reader_rt());
        let write = send::application::Builder::new(self.env.writer_rt());

        let stream = application::Builder {
            read,
            write,
            shared,
            sockets: sockets.application,
        };

        Ok(stream)
    }
}
