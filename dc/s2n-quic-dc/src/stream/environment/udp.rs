// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{
    environment::{Environment, Peer, SetupResult, SocketSet},
    recv::{
        buffer,
        dispatch::{Control, Stream},
        shared::RecvBuffer,
    },
    socket, TransportFeatures,
};
use s2n_quic_core::inet::SocketAddress;
use std::sync::Arc;

#[derive(Debug)]
pub struct Pooled<S: socket::application::Application, W: socket::Socket> {
    pub peer_addr: SocketAddress,
    pub control: Control,
    pub stream: Stream,
    pub application_socket: Arc<S>,
    pub worker_socket: Arc<W>,
}

impl<E, S, W> Peer<E> for Pooled<S, W>
where
    E: Environment,
    S: socket::application::Application + 'static,
    W: socket::Socket + 'static,
{
    type ReadWorkerSocket = Arc<W>;
    type WriteWorkerSocket = (Arc<W>, buffer::Channel<Control>);

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
    }

    #[inline]
    fn setup(self, _env: &E) -> SetupResult<Self::ReadWorkerSocket, Self::WriteWorkerSocket> {
        let remote_addr = self.peer_addr;
        let control = self.control;
        let stream = self.stream;
        let queue_id = control.queue_id();

        let application = Box::new(self.application_socket);
        let read_worker = Some(self.worker_socket.clone());
        let write_worker = Some((self.worker_socket, buffer::Channel::new(control)));

        let socket = SocketSet {
            application,
            read_worker,
            write_worker,
            remote_addr,
            source_queue_id: Some(queue_id),
        };

        let recv_buffer = RecvBuffer::B(buffer::Channel::new(stream));

        Ok((socket, recv_buffer))
    }
}
