// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock,
    stream::{runtime, socket, TransportFeatures},
};
use core::future::Future;
use s2n_quic_core::inet::SocketAddress;
use s2n_quic_platform::features;
use std::io;

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
}
