// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::bach::Clock,
    event,
    stream::{
        environment::udp::Config as PoolConfig,
        runtime::{bach as runtime, ArcHandle},
        server::accept,
        socket,
    },
};
use bach::ext::*;
use s2n_quic_platform::features;
use std::{io, net::SocketAddr, sync::Arc};
use tracing::{info_span, Instrument};

mod pool;
pub mod udp;

pub struct Builder<Sub>
where
    Sub: event::Subscriber,
{
    gso: Option<features::Gso>,
    socket_options: Option<socket::Options>,
    pool: Option<PoolConfig>,
    threads: Option<usize>,
    acceptor: Option<accept::Sender<Sub>>,
    subscriber: Sub,
}

impl<Sub> Default for Builder<Sub>
where
    Sub: event::Subscriber + Clone + Default,
{
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<Sub> Builder<Sub>
where
    Sub: event::Subscriber + Clone,
{
    pub fn new(subscriber: Sub) -> Self {
        Self {
            gso: None,
            socket_options: None,
            threads: None,
            acceptor: None,
            pool: None,
            subscriber,
        }
    }

    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = Some(threads);
        self
    }

    pub fn with_pool(mut self, config: PoolConfig) -> Self {
        self.pool = Some(config);
        self
    }

    pub fn with_socket_options(mut self, socket_options: socket::Options) -> Self {
        self.socket_options = Some(socket_options);
        self
    }

    pub fn with_acceptor(mut self, sender: accept::Sender<Sub>) -> Self {
        self.acceptor = Some(sender);
        self
    }

    #[inline]
    pub fn build(self) -> io::Result<Environment<Sub>> {
        let Self {
            gso,
            socket_options,
            pool,
            threads,
            acceptor,
            subscriber,
        } = self;
        let gso = gso.unwrap_or_else(|| {
            // rather than clamping it to the max burst size, let the CCA be the only
            // component that controls send quantums
            features::gso::MAX_SEGMENTS.into()
        });
        let socket_options = socket_options.unwrap_or_default();

        let rt = Arc::new(runtime::Handle::current());

        let mut env = Environment {
            gso,
            socket_options,
            rt,
            recv_pool: None,
            subscriber,
        };

        let config = pool.expect("bach only supports pooled sockets");

        let workers = threads.unwrap_or(1);
        let pool = pool::Pool::new(&env, workers, config, acceptor)?;
        env.recv_pool = Some(Arc::new(pool));

        Ok(env)
    }
}

#[derive(Clone)]
pub struct Environment<Sub> {
    gso: features::Gso,
    socket_options: socket::Options,
    rt: Arc<runtime::Handle>,
    subscriber: Sub,
    recv_pool: Option<Arc<pool::Pool>>,
}

impl<Sub> Default for Environment<Sub>
where
    Sub: event::Subscriber + Clone + Default,
{
    #[inline]
    fn default() -> Self {
        Self::builder().build().unwrap()
    }
}

impl<Sub> Environment<Sub>
where
    Sub: event::Subscriber + Clone + Default,
{
    #[inline]
    pub fn builder() -> Builder<Sub> {
        Default::default()
    }
}

impl<Sub> Environment<Sub>
where
    Sub: event::Subscriber + Clone,
{
    #[inline]
    pub fn builder_with_subscriber(subscriber: Sub) -> Builder<Sub> {
        Builder::new(subscriber)
    }

    #[inline]
    pub fn has_recv_pool(&self) -> bool {
        self.recv_pool.is_some()
    }

    #[inline]
    pub fn pool_addr(&self) -> Option<SocketAddr> {
        self.recv_pool.as_ref().map(|v| v.local_addr())
    }
}

impl<Sub> super::Environment for Environment<Sub>
where
    Sub: event::Subscriber + Clone,
{
    type Clock = Clock;
    type Subscriber = Sub;

    #[inline]
    fn subscriber(&self) -> &Self::Subscriber {
        &self.subscriber
    }

    #[inline]
    fn clock(&self) -> Self::Clock {
        Clock::default()
    }

    #[inline]
    fn gso(&self) -> features::Gso {
        self.gso.clone()
    }

    #[inline]
    fn reader_rt(&self) -> ArcHandle<Self::Subscriber> {
        self.rt.clone()
    }

    #[inline]
    #[track_caller]
    fn spawn_reader<F: 'static + Send + std::future::Future<Output = ()>>(&self, f: F) {
        self.rt.spawn(f.instrument(info_span!("reader")).primary());
    }

    #[inline]
    fn writer_rt(&self) -> ArcHandle<Self::Subscriber> {
        self.rt.clone()
    }

    #[inline]
    #[track_caller]
    fn spawn_writer<F: 'static + Send + std::future::Future<Output = ()>>(&self, f: F) {
        self.rt.spawn(f.instrument(info_span!("writer")).primary());
    }
}
