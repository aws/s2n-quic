// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::tokio::Clock,
    event,
    stream::{
        runtime::{tokio as runtime, ArcHandle},
        server::accept,
        socket,
    },
};
use s2n_quic_platform::features;
use std::{io, net::SocketAddr, sync::Arc};

pub mod pool;
pub mod tcp;
pub mod udp;

pub struct Builder<Sub>
where
    Sub: event::Subscriber,
{
    clock: Option<Clock>,
    gso: Option<features::Gso>,
    socket_options: Option<socket::Options>,
    reader_rt: Option<runtime::Shared<Sub>>,
    writer_rt: Option<runtime::Shared<Sub>>,
    thread_name_prefix: Option<String>,
    threads: Option<usize>,
    pool: Option<pool::Config>,
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
            clock: None,
            gso: None,
            socket_options: None,
            reader_rt: None,
            writer_rt: None,
            thread_name_prefix: None,
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

    pub fn with_pool(mut self, config: pool::Config) -> Self {
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
            clock,
            gso,
            socket_options,
            reader_rt,
            writer_rt,
            thread_name_prefix,
            threads,
            pool,
            acceptor,
            subscriber,
        } = self;
        let clock = clock.unwrap_or_default();
        let gso = gso.unwrap_or_else(|| {
            // rather than clamping it to the max burst size, let the CCA be the only
            // component that controls send quantums
            features::gso::MAX_SEGMENTS.into()
        });
        let socket_options = socket_options.unwrap_or_default();

        let thread_count = threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|v| v.get())
                .unwrap_or(1)
        });
        let thread_name_prefix = thread_name_prefix.as_deref().unwrap_or("dc_quic");

        let make_rt = |suffix: &str| {
            let mut builder = tokio::runtime::Builder::new_multi_thread();
            Ok(builder
                .enable_all()
                .worker_threads(thread_count)
                .thread_name(format!("{thread_name_prefix}::{suffix}"))
                .build()?
                .into())
        };

        let reader_rt = reader_rt
            .map(<io::Result<_>>::Ok)
            .unwrap_or_else(|| make_rt("reader"))?;
        let writer_rt = writer_rt
            .map(<io::Result<_>>::Ok)
            .unwrap_or_else(|| make_rt("writer"))?;

        let mut env = Environment {
            clock,
            gso,
            socket_options,
            reader_rt,
            writer_rt,
            recv_pool: None,
            subscriber,
        };

        if let Some(config) = pool {
            let pool = pool::Pool::new(&env, thread_count, config, acceptor)?;
            env.recv_pool = Some(Arc::new(pool));
        };

        Ok(env)
    }
}

#[derive(Clone)]
pub struct Environment<Sub> {
    clock: Clock,
    gso: features::Gso,
    socket_options: socket::Options,
    reader_rt: runtime::Shared<Sub>,
    writer_rt: runtime::Shared<Sub>,
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
        self.recv_pool.as_ref().and_then(|v| v.local_addr())
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
    fn clock(&self) -> &Self::Clock {
        &self.clock
    }

    #[inline]
    fn gso(&self) -> features::Gso {
        self.gso.clone()
    }

    #[inline]
    fn reader_rt(&self) -> ArcHandle<Self::Subscriber> {
        self.reader_rt.handle()
    }

    #[inline]
    fn spawn_reader<F: 'static + Send + std::future::Future<Output = ()>>(&self, f: F) {
        self.reader_rt.spawn(f);
    }

    #[inline]
    fn writer_rt(&self) -> ArcHandle<Self::Subscriber> {
        self.writer_rt.handle()
    }

    #[inline]
    fn spawn_writer<F: 'static + Send + std::future::Future<Output = ()>>(&self, f: F) {
        self.writer_rt.spawn(f);
    }
}
