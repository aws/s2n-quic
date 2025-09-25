// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::{self, api::Subscriber as _, IntoEvent as _},
    packet,
    path::secret::{self, map, Map},
    random::Random,
    stream::{
        application,
        environment::{Environment, Peer},
        recv,
        send::{self, flow},
        server, shared,
    },
};
use core::cell::UnsafeCell;
use s2n_quic_core::{
    dc, endpoint,
    inet::ExplicitCongestionNotification,
    time::{Clock as _, Timestamp},
    varint::VarInt,
};
use std::{io, sync::Arc};
use tracing::{debug_span, Instrument as _};

use super::environment::{ReadWorkerSocket as _, WriteWorkerSocket as _};

type Result<T = (), E = io::Error> = core::result::Result<T, E>;

pub struct AcceptError<Peer> {
    pub secret_control: Vec<u8>,
    pub peer: Option<Peer>,
    pub error: io::Error,
}

#[inline]
pub fn open_stream<Env, P>(
    env: &Env,
    entry: map::Peer,
    peer: P,
    parameter_override: Option<&dyn Fn(dc::ApplicationParams) -> dc::ApplicationParams>,
) -> Result<application::Builder<Env::Subscriber>>
where
    Env: Environment,
    P: Peer<Env>,
{
    let (crypto, mut parameters) = entry.pair(&peer.features());

    if let Some(o) = parameter_override {
        parameters = o(parameters);
    }

    let stream_id = packet::stream::Id {
        // the client starts with routing to 0 until the server updates the value
        queue_id: VarInt::ZERO,
        is_reliable: true,
        is_bidirectional: true,
    };

    let now = env.clock().get_time();

    let meta = event::api::ConnectionMeta {
        id: 0, // TODO use an actual connection ID
        timestamp: now.into_event(),
    };
    let info = event::api::ConnectionInfo {};

    let subscriber = env.subscriber().clone();
    let subscriber_ctx = subscriber.create_connection_context(&meta, &info);

    build_stream(
        now,
        env,
        peer,
        stream_id,
        crypto,
        entry.map(),
        parameters,
        endpoint::Type::Client,
        subscriber,
        subscriber_ctx,
    )
}

#[inline]
pub fn accept_stream<Env, P>(
    now: Timestamp,
    env: &Env,
    peer: P,
    packet: &server::InitialPacket,
    map: &Map,
    subscriber_ctx: <Env::Subscriber as event::Subscriber>::ConnectionContext,
    parameter_override: Option<&dyn Fn(dc::ApplicationParams) -> dc::ApplicationParams>,
) -> Result<application::Builder<Env::Subscriber>, AcceptError<P>>
where
    Env: Environment,
    P: Peer<Env>,
{
    let credentials = &packet.credentials;
    let mut secret_control = vec![];
    let Some((crypto, mut parameters)) = map.pair_for_credentials(
        credentials,
        packet.source_queue_id,
        &peer.features(),
        &mut secret_control,
    ) else {
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

    let stream_id = packet::stream::Id {
        // use the client's `source_queue_id`, if specified
        queue_id: packet.source_queue_id.unwrap_or(VarInt::ZERO),
        // inherit the rest of the parameters from the client
        ..packet.stream_id
    };

    let subscriber = env.subscriber().clone();

    let res = build_stream(
        now,
        env,
        peer,
        stream_id,
        crypto,
        map,
        parameters,
        endpoint::Type::Server,
        subscriber,
        subscriber_ctx,
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
fn build_stream<Env, P>(
    now: Timestamp,
    env: &Env,
    peer: P,
    stream_id: packet::stream::Id,
    crypto: secret::map::Bidirectional,
    map: &Map,
    parameters: dc::ApplicationParams,
    endpoint_type: endpoint::Type,
    subscriber: Env::Subscriber,
    subscriber_ctx: <Env::Subscriber as event::Subscriber>::ConnectionContext,
) -> Result<application::Builder<Env::Subscriber>>
where
    Env: Environment,
    P: Peer<Env>,
{
    let features = peer.features();

    let (sockets, recv_buffer) = peer.setup(env)?;
    let source_queue_id = sockets.source_queue_id;

    // construct shared reader state
    let reader = recv::shared::State::new(
        stream_id,
        &parameters,
        features,
        recv_buffer,
        endpoint_type,
        &now,
    );

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
            max_datagram_size: parameters.max_datagram_size(),
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
            is_reliable: stream_id.is_reliable,
        };

        let remote_addr = sockets.remote_addr;

        let fixed = shared::FixedValues {
            remote_ip: UnsafeCell::new(remote_addr.ip()),
            application: UnsafeCell::new(application),
            credentials: UnsafeCell::new(crypto.credentials),
        };

        shared::Common {
            clock: env.clock().clone(),
            gso: env.gso(),
            remote_port: remote_addr.port().into(),
            remote_queue_id: stream_id.queue_id.as_u64().into(),
            local_queue_id: if let Some(id) = source_queue_id {
                id.as_u64()
            } else {
                // use MAX as `None`
                u64::MAX
            }
            .into(),
            last_peer_activity: Default::default(),
            fixed,
            closed_halves: 0u8.into(),
            subscriber: shared::Subscriber {
                subscriber,
                context: subscriber_ctx,
            },
        }
    };

    let crypto = {
        let secret::map::Bidirectional {
            application,
            control,
            credentials: _,
        } = crypto;

        let control = control.map(|c| (c.sealer, c.opener));

        shared::Crypto::new(application.sealer, application.opener, control, map)
    };

    let shared = Arc::new(shared::Shared {
        receiver: reader,
        sender: writer.0,
        common,
        crypto,
    });

    // spawn the read worker
    if let Some(socket) = sockets.read_worker {
        let socket = socket.setup();
        let shared = shared.clone();

        let task = async move {
            let mut reader = recv::worker::Worker::new(socket, shared, endpoint_type, &parameters);

            let mut prev_waker: Option<core::task::Waker> = None;
            core::future::poll_fn(|cx| {
                // update the waker if needed
                if prev_waker
                    .as_ref()
                    .is_none_or(|prev| !prev.will_wake(cx.waker()))
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
            env.spawn_reader(task);
        } else {
            env.spawn_reader(task.instrument(span));
        }
    }

    // spawn the write worker
    if let Some((worker, socket)) = writer.1 {
        let (socket, recv_buffer) = socket.setup();
        let shared = shared.clone();

        let task = async move {
            let mut writer = send::worker::Worker::new(
                socket,
                recv_buffer,
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
                    .is_none_or(|prev| !prev.will_wake(cx.waker()))
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
            env.spawn_writer(task);
        } else {
            env.spawn_writer(task.instrument(span));
        }
    }

    let read = recv::application::Builder::new(endpoint_type, env.reader_rt());
    let write = send::application::Builder::new(env.writer_rt());

    let stream = application::Builder {
        read,
        write,
        shared,
        sockets: sockets.application,
        queue_time: now,
    };

    Ok(stream)
}
