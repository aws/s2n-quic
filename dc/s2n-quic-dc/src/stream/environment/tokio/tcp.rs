// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{
        environment::{tokio::Environment, Peer, SetupResult, SocketSet},
        recv::shared::RecvBuffer,
        TransportFeatures,
    },
};
use s2n_quic_core::inet::SocketAddress;
use tokio::net::TcpStream;

/// A socket that is already registered with the application runtime
pub struct Registered {
    pub socket: TcpStream,
    pub peer_addr: SocketAddress,
    pub local_port: u16,
    pub recv_buffer: RecvBuffer,
}

impl<Sub> Peer<Environment<Sub>> for Registered
where
    Sub: event::Subscriber,
{
    type ReadWorkerSocket = ();
    type WriteWorkerSocket = ();

    fn features(&self) -> TransportFeatures {
        TransportFeatures::TCP
    }

    #[inline]
    fn with_source_control_port(&mut self, port: u16) {
        let _ = port;
    }

    #[inline]
    fn setup(
        self,
        _env: &Environment<Sub>,
    ) -> SetupResult<Self::ReadWorkerSocket, Self::WriteWorkerSocket> {
        let remote_addr = self.peer_addr;
        let source_control_port = self.local_port;
        let application = Box::new(self.socket);
        let socket = SocketSet {
            application,
            read_worker: None,
            write_worker: None,
            remote_addr,
            source_control_port,
            source_queue_id: None,
        };
        Ok((socket, self.recv_buffer))
    }
}

/// A socket that should be reregistered with the application runtime
pub struct Reregistered {
    pub socket: TcpStream,
    pub peer_addr: SocketAddress,
    pub local_port: u16,
    pub recv_buffer: RecvBuffer,
}

impl<Sub> Peer<Environment<Sub>> for Reregistered
where
    Sub: event::Subscriber,
{
    type ReadWorkerSocket = ();
    type WriteWorkerSocket = ();

    fn features(&self) -> TransportFeatures {
        TransportFeatures::TCP
    }

    #[inline]
    fn with_source_control_port(&mut self, port: u16) {
        let _ = port;
    }

    #[inline]
    fn setup(
        self,
        _env: &Environment<Sub>,
    ) -> SetupResult<Self::ReadWorkerSocket, Self::WriteWorkerSocket> {
        let source_control_port = self.local_port;
        let remote_addr = self.peer_addr;
        let application = Box::new(self.socket.into_std()?);
        let socket = SocketSet {
            application,
            read_worker: None,
            write_worker: None,
            remote_addr,
            source_control_port,
            source_queue_id: None,
        };
        Ok((socket, self.recv_buffer))
    }
}
