// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Turmoil-based environment for deterministic network simulation testing.
//!
//! This environment allows s2n-quic-dc streams to run over turmoil's simulated
//! network, enabling testing of partition/repair scenarios and other network
//! failure modes.

use crate::{
    clock::tokio::Clock,
    event,
    stream::{environment::udp::Config as PoolConfig, runtime::ArcHandle, server::accept},
};
use s2n_quic_platform::features;
use std::{io, net::SocketAddr, sync::Arc};
use tracing::{info_span, Instrument};

mod pool;
pub mod udp;

pub struct Builder<Sub>
where
    Sub: event::Subscriber,
{
    pool: Option<PoolConfig>,
    threads: Option<usize>,
    acceptor: Option<accept::Sender<Sub>>,
    subscriber: Sub,
    addr: SocketAddr,
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
            pool: None,
            threads: None,
            acceptor: None,
            subscriber,
            addr: "0.0.0.0:0".parse().unwrap(),
        }
    }

    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = Some(threads);
        self
    }

    /// No-op for API consistency with other environments.
    /// Turmoil doesn't use socket options.
    pub fn with_socket_options(self, _options: crate::stream::socket::Options) -> Self {
        self
    }

    pub fn with_pool(mut self, config: PoolConfig) -> Self {
        self.pool = Some(config);
        self
    }

    pub fn with_addr(mut self, addr: SocketAddr) -> Self {
        self.addr = addr;
        self
    }

    pub fn with_acceptor(mut self, sender: accept::Sender<Sub>) -> Self {
        self.acceptor = Some(sender);
        self
    }

    #[inline]
    pub fn build(self) -> io::Result<Environment<Sub>> {
        let Self {
            pool,
            threads,
            acceptor,
            subscriber,
            addr,
        } = self;

        // Turmoil doesn't support GSO, so we disable it
        let gso = features::Gso::default();

        let rt = Arc::new(tokio::runtime::Handle::current());

        let mut env = Environment {
            gso,
            rt,
            recv_pool: None,
            subscriber,
        };

        if let Some(config) = pool {
            let workers = threads.unwrap_or(1);
            let pool = pool::Pool::new(&env, workers, config, acceptor, addr)?;
            env.recv_pool = Some(Arc::new(pool));
        }

        Ok(env)
    }
}

#[derive(Clone)]
pub struct Environment<Sub> {
    gso: features::Gso,
    rt: Arc<tokio::runtime::Handle>,
    recv_pool: Option<Arc<pool::Pool>>,
    subscriber: Sub,
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

    /// Initialize the turmoil pool asynchronously.
    /// Must be called from an async context before using the pool.
    pub async fn ensure_ready(&self) -> io::Result<()> {
        if let Some(pool) = &self.recv_pool {
            pool.ensure_initialized().await?;
        }
        Ok(())
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
        self.rt.spawn(f.instrument(info_span!("reader")));
    }

    #[inline]
    fn writer_rt(&self) -> ArcHandle<Self::Subscriber> {
        self.rt.clone()
    }

    #[inline]
    #[track_caller]
    fn spawn_writer<F: 'static + Send + std::future::Future<Output = ()>>(&self, f: F) {
        self.rt.spawn(f.instrument(info_span!("writer")));
    }
}
