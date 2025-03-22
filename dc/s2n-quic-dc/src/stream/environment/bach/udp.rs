// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{
        environment::{bach::Environment, Peer, SetupResult},
        recv::{buffer, dispatch::Control},
        socket::{application::Single, Tracing},
        TransportFeatures,
    },
};
use bach::net::UdpSocket;
use s2n_quic_core::inet::SocketAddress;
use std::sync::Arc;

pub(super) type RecvSocket = Arc<UdpSocket>;
pub(super) type WorkerSocket = Arc<Tracing<RecvSocket>>;
pub(super) type ApplicationSocket = Arc<Single<Tracing<RecvSocket>>>;

#[derive(Debug)]
pub struct Pooled(pub SocketAddress);

impl<Sub> Peer<Environment<Sub>> for Pooled
where
    Sub: event::Subscriber + Clone,
{
    type ReadWorkerSocket = WorkerSocket;
    type WriteWorkerSocket = (WorkerSocket, buffer::Channel<Control>);

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
    }

    #[inline]
    fn setup(
        self,
        env: &Environment<Sub>,
    ) -> SetupResult<Self::ReadWorkerSocket, Self::WriteWorkerSocket> {
        let peer_addr = self.0;
        let recv_pool = env.recv_pool.as_ref().expect("pool not configured");
        // the client doesn't need to associate credentials since it's already chosen a queue_id
        let credentials = None;
        let (control, stream, application_socket, worker_socket) = recv_pool.alloc(credentials);
        crate::stream::environment::udp::Pooled {
            peer_addr,
            control,
            stream,
            application_socket,
            worker_socket,
        }
        .setup(env)
    }
}
