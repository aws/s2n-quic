// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock, event,
    stream::{recv, runtime, socket, TransportFeatures},
};
use core::future::Future;
use s2n_quic_core::{inet::SocketAddress, varint::VarInt};
use s2n_quic_platform::features;
use std::{io, sync::Arc};

use super::recv::buffer::Buffer;

type Result<T = (), E = io::Error> = core::result::Result<T, E>;

#[cfg(feature = "tokio")]
pub mod tokio;

pub trait Environment {
    type Clock: Clone + clock::Clock;
    type Subscriber: event::Subscriber;

    fn clock(&self) -> &Self::Clock;
    fn gso(&self) -> features::Gso;
    fn reader_rt(&self) -> runtime::ArcHandle<Self::Subscriber>;
    fn spawn_reader<F: 'static + Send + Future<Output = ()>>(&self, f: F);
    fn writer_rt(&self) -> runtime::ArcHandle<Self::Subscriber>;
    fn spawn_writer<F: 'static + Send + Future<Output = ()>>(&self, f: F);
}

pub struct SocketSet<R, W = R> {
    pub application: Box<dyn socket::application::Builder>,
    pub read_worker: Option<R>,
    pub write_worker: Option<W>,
    pub remote_addr: SocketAddress,
    pub source_control_port: u16,
    pub source_queue_id: Option<VarInt>,
}

type SetupResult<ReadWorker, WriteWorker> =
    Result<(SocketSet<ReadWorker, WriteWorker>, recv::shared::RecvBuffer)>;

pub trait Peer<E: Environment> {
    type ReadWorkerSocket: ReadWorkerSocket;
    type WriteWorkerSocket: WriteWorkerSocket;

    fn features(&self) -> TransportFeatures;
    fn with_source_control_port(&mut self, port: u16);
    fn setup(self, env: &E) -> SetupResult<Self::ReadWorkerSocket, Self::WriteWorkerSocket>;
}

pub trait ReadWorkerSocket {
    type Socket: super::socket::Socket;

    fn setup(self) -> Self::Socket;
}

impl ReadWorkerSocket for () {
    type Socket = super::socket::SendOnly<Arc<std::net::UdpSocket>>;

    #[inline]
    fn setup(self) -> Self::Socket {
        unreachable!()
    }
}

impl<T: super::socket::Socket> ReadWorkerSocket for T {
    type Socket = T;

    #[inline]
    fn setup(self) -> Self::Socket {
        self
    }
}

pub trait WriteWorkerSocket {
    type Socket: super::socket::Socket;
    type Buffer: 'static + Buffer + Send;

    fn setup(self) -> (Self::Socket, Self::Buffer);
}

impl WriteWorkerSocket for () {
    type Socket = super::socket::SendOnly<Arc<std::net::UdpSocket>>;
    type Buffer = recv::buffer::Local;

    #[inline]
    fn setup(self) -> (Self::Socket, Self::Buffer) {
        unreachable!()
    }
}

impl<T: super::socket::Socket, B: 'static + Buffer + Send> WriteWorkerSocket for (T, B) {
    type Socket = T;
    type Buffer = B;

    #[inline]
    fn setup(self) -> (Self::Socket, Self::Buffer) {
        self
    }
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
}
