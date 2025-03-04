// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{
    recv::{
        buffer,
        dispatch::{Control, Stream},
        shared::RecvBuffer,
    },
    socket::{fd::udp, SendOnly},
    TransportFeatures,
};
use s2n_quic_core::inet::SocketAddress;
use std::sync::Arc;

#[derive(Debug)]
pub struct PoolSocket<S: udp::Socket> {
    pub peer_addr: SocketAddress,
    pub control: Control,
    pub stream: Stream,
    pub socket: Arc<S>,
}

impl<E, S> super::Peer<E> for PoolSocket<S>
where
    E: super::Environment,
    S: udp::Socket + 'static,
{
    type ReadWorkerSocket = SendOnly<Arc<S>>;
    type WriteWorkerSocket = (SendOnly<Arc<S>>, buffer::Channel<Control>);

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
    }

    #[inline]
    fn with_source_control_port(&mut self, port: u16) {
        self.peer_addr.set_port(port);
    }

    #[inline]
    fn setup(
        self,
        _env: &E,
    ) -> super::Result<(
        super::SocketSet<Self::ReadWorkerSocket, Self::WriteWorkerSocket>,
        RecvBuffer,
    )> {
        let remote_addr = self.peer_addr;
        let control = self.control;
        let stream = self.stream;
        let socket = self.socket;
        let source_control_port = udp::Socket::local_addr(&socket)?.port();

        let socket = SendOnly(socket);

        let application = Box::new(socket.clone());
        let read_worker = Some(socket.clone());
        let write_worker = Some((socket, buffer::Channel::new(control)));

        let socket = super::SocketSet {
            application,
            read_worker,
            write_worker,
            remote_addr,
            source_control_port,
            source_stream_port: None,
        };

        let recv_buffer = RecvBuffer::B(buffer::Channel::new(stream));

        Ok((socket, recv_buffer))
    }
}
