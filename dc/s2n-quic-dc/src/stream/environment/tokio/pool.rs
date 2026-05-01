// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::udp::{ApplicationSendSocket, ArcSocket, WorkerSendSocket};
use crate::{
    credentials::Credentials,
    event,
    socket::{
        pool::{self, Pool as Packets},
        recv::{router::Router, udp},
    },
    stream::{
        self,
        environment::{
            tokio::Environment,
            udp::{Config, Workers},
            Environment as _,
        },
        load_balance::PickTwo,
        recv::dispatch::{Allocator as Queues, Control, Stream},
        server::{accept, udp::Acceptor},
        socket::{application::Single, BusyPoll, Events, SendOnly, Tracing},
    },
};
use s2n_quic_platform::socket::options::{Options, ReusePort};
use std::{
    io::Result,
    net::{SocketAddr, UdpSocket},
    sync::{Arc, Mutex},
};
use tokio::io::unix::AsyncFd;
use tracing::Instrument;

pub(super) struct Pool {
    sockets: Box<[PoolSocket]>,
    local_addr: SocketAddr,
    transmission_pool: pool::Pool,
    load_balancer: PickTwo,
}

struct PoolSocket {
    socket: ArcSocket,
    worker: WorkerSendSocket,
    application: Box<[ApplicationSendSocket]>,
    recv_queue: Mutex<Queues>,
    // Channel receiver for wheel to read transmissions from (taken on first use)
    #[allow(dead_code)] // TODO: use this when implementing new wheel architecture
    wheel_rx: Mutex<
        Option<
            crate::socket::channel::intrusive_queue::sync::Receiver<
                crate::stream::send::state::transmission::Transmission,
            >,
        >,
    >,
}

macro_rules! spawn_span {
    ($span:expr, $task:ident, | $spanned:ident | $spawn:block) => {
        let span = $span;
        if span.is_disabled() {
            let $spanned = $task;
            $spawn
        } else {
            let $spanned = ($task).instrument(span);
            $spawn
        }
    };
}

impl PoolSocket {
    fn new(socket: UdpSocket, recv_queue: Mutex<Queues>, config: &Config) -> Self {
        let socket = Arc::new(socket);
        let local_addr = socket.local_addr().unwrap();

        // Create a channel for transmissions from streams to wheel
        let (tx, rx) = crate::socket::channel::intrusive_queue::sync::new();
        let worker_socket = stream::socket::Wheel::new(tx, local_addr.into());
        let worker = Arc::new(Tracing(worker_socket));

        // Create channels for application sockets (higher priorities)
        let application = (0..config.priority_levels)
            .map(|_| {
                // For now, share the same channel as worker
                // TODO: create separate channels per priority level
                Arc::new(Single(Tracing(worker.0.clone())))
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            worker,
            application,
            socket,
            recv_queue,
            wheel_rx: Mutex::new(Some(rx)),
        }
    }

    fn create_send_socket_worker(
        &self,
        _config: &Config,
        _env: &Environment<impl event::Subscriber + Clone>,
        _clock: impl crate::clock::precision::Clock + Clone + 'static,
    ) -> impl core::future::Future<Output = ()> + Send + 'static {
        // TODO: Implement new channel-based wheel architecture
        // The implementation needs to:
        // 1. Take the receiver from self.wheel_rx
        // 2. Create a Wheel that reads from the channel
        // 3. Pump the wheel output into a channel
        // 4. Pass the channel receiver to send::udp::non_blocking
        // 5. Poll both the pump and sender tasks concurrently
        //
        // Current blocker: HRTB lifetime issue with Weak<dyn Notify>
        // See: https://github.com/rust-lang/rust/issues/...
        async move { todo!("implement new wheel-based send worker") }
    }

    fn spawn_non_blocking_send_worker(
        &self,
        config: &Config,
        env: &Environment<impl event::Subscriber + Clone>,
    ) {
        let clock = env.clock();
        let task = self.create_send_socket_worker(config, env, clock);
        let span = tracing::trace_span!("send_socket_worker");
        spawn_span!(span, task, |task| {
            env.writer_rt.spawn(task);
        });
    }

    fn spawn_non_blocking_recv_worker(
        &self,
        _config: &Config,
        env: &Environment<impl event::Subscriber + Clone>,
        alloc: pool::Pool,
        router: impl Router + Send + 'static,
    ) {
        let socket = Tracing(AsyncFd::new(self.socket.clone()).unwrap());
        let socket = Events::new(socket, env.subscriber.clone(), env.clock());
        let task = udp::non_blocking(socket, alloc, router);
        let span = tracing::trace_span!("recv_socket_worker");
        spawn_span!(span, task, |task| {
            env.reader_rt.spawn(task);
        });
    }

    fn spawn_busy_poll_send_worker(
        &self,
        config: &Config,
        env: &Environment<impl event::Subscriber + Clone>,
        handle: &crate::busy_poll::Handle,
    ) {
        let clock = crate::busy_poll::clock::Timer::new(env.clock());
        let task = self.create_send_socket_worker(config, env, clock);
        let span = tracing::trace_span!("send_socket_worker");
        spawn_span!(span, task, |task| {
            handle.spawn_with_priority(task, config.flow_priority);
        });
    }

    fn spawn_busy_poll_recv_worker(
        &self,
        config: &Config,
        env: &Environment<impl event::Subscriber + Clone>,
        alloc: pool::Pool,
        router: impl Router + Send + 'static,
        handle: &crate::busy_poll::Handle,
    ) {
        let socket = BusyPoll(self.socket.clone());
        let clock = env.clock();
        let socket = Events::new(socket, env.subscriber.clone(), clock);
        let task = udp::non_blocking(socket, alloc, router);
        let span = tracing::trace_span!("recv_socket_worker");
        spawn_span!(span, task, |task| {
            handle.spawn_with_priority(task, config.flow_priority);
        });
    }
}

impl Pool {
    pub fn new<Sub>(
        env: &Environment<Sub>,
        workers: usize,
        mut config: Config,
        acceptor: Option<accept::Sender<Sub>>,
    ) -> Result<Self>
    where
        Sub: event::Subscriber + Clone,
    {
        debug_assert_ne!(workers, 0);

        let options = env.socket_options.clone();

        config.send_workers.set_default(workers);
        config.recv_workers.set_default(workers);

        if acceptor.is_some() && config.socket_count() > 1 {
            config.reuse_port = true;
        }

        let create_queue = || {
            if acceptor.is_some() {
                Queues::new_non_zero(config.stream_recv_queue, config.control_recv_queue)
            } else {
                Queues::new(config.stream_recv_queue, config.control_recv_queue)
            }
        };
        let sockets = Self::create_workers(options, &config, create_queue)?;

        let local_addr = sockets[0].socket.local_addr()?;
        if cfg!(debug_assertions) && config.reuse_port {
            for socket in sockets.iter().skip(1) {
                debug_assert_eq!(local_addr, socket.socket.local_addr()?);
            }
        }

        let unroutable_packets = {
            // TODO pace these packets
            let socket = Tracing(SendOnly(sockets[0].socket.clone()));
            let (tx, task) = config.unroutable_packets(socket);

            env.reader_rt.spawn(task);

            tx
        };

        let transmission_pool = config.tx_packet_pool();

        macro_rules! spawn {
            ($create_router:expr) => {
                let _rt = env.reader_rt.enter();
                Self::spawn_non_blocking(env, &config, &sockets, $create_router)?;
            };
        }

        if let Some(sender) = acceptor {
            // Collect all application sockets from all workers for load balancing
            let app_sockets: Box<[_]> = sockets.iter().map(|s| s.application[0].clone()).collect();

            spawn!(|_packets: &Packets, socket: &PoolSocket| {
                let queues = socket.recv_queue.lock().unwrap();
                let worker_socket = socket.worker.clone();

                // TODO pace these packets
                let secret_socket = Tracing(SendOnly(sockets[0].socket.clone()));

                let acceptor = Acceptor::new(
                    env.clone(),
                    sender.clone(),
                    config.map.clone(),
                    config.accept_flavor,
                    queues.clone(),
                    app_sockets.clone(),
                    worker_socket,
                    secret_socket,
                    transmission_pool.clone(),
                    unroutable_packets.clone(),
                );

                let router = queues
                    .dispatcher(unroutable_packets.clone())
                    .with_map(config.map.clone());
                router.with_zero_router(acceptor)
            });
        } else {
            spawn!(|_packets: &Packets, socket: &PoolSocket| {
                let dispatch = socket
                    .recv_queue
                    .lock()
                    .unwrap()
                    .dispatcher(unroutable_packets.clone());
                dispatch.with_map(config.map.clone())
            });
        }

        Ok(Self {
            sockets: sockets.into(),
            local_addr,
            transmission_pool,
            load_balancer: PickTwo::new(),
        })
    }

    pub fn alloc(
        &self,
        credentials: &Credentials,
    ) -> (
        Control,
        Stream,
        ApplicationSendSocket,
        WorkerSendSocket,
        pool::Pool,
    ) {
        let idx = self.load_balancer.select(
            &self.sockets,
            |socket| Arc::strong_count(&socket.application[0]),
            |upper_bound| rand::random_range(..upper_bound),
        );

        let socket = &self.sockets[idx];

        let (control, stream) = socket.recv_queue.lock().unwrap().alloc_or_grow(credentials);

        // Application sockets currently only have 1 priority
        // TODO take this in as a parameter
        let priority = 0;

        let worker_socket = socket.worker.clone();
        let app_socket = socket.application[priority].clone();
        let transmission_pool = self.transmission_pool.clone();

        (
            control,
            stream,
            app_socket,
            worker_socket,
            transmission_pool,
        )
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    fn spawn_non_blocking<R>(
        env: &Environment<impl event::Subscriber + Clone>,
        config: &Config,
        sockets: &[PoolSocket],
        create_router: impl Fn(&Packets, &PoolSocket) -> R,
    ) -> Result<()>
    where
        R: 'static + Send + Router,
    {
        for (udp_socket_worker, socket) in sockets.iter().enumerate() {
            let alloc = config.rx_packet_pool();
            let router = create_router(&alloc, socket);

            match &config.send_workers {
                Workers::BusyPoll(pool) => {
                    let idx = udp_socket_worker % pool.len();
                    let handle = &pool[idx];
                    socket.spawn_busy_poll_send_worker(config, env, handle);
                }
                Workers::Environment(_) => {
                    socket.spawn_non_blocking_send_worker(config, env);
                }
            }

            match &config.recv_workers {
                Workers::BusyPoll(pool) => {
                    let idx = udp_socket_worker % pool.len();
                    let handle = &pool[idx];
                    socket.spawn_busy_poll_recv_worker(config, env, alloc, router, handle);
                }
                Workers::Environment(_) => {
                    socket.spawn_non_blocking_recv_worker(config, env, alloc, router);
                }
            }
        }
        Ok(())
    }

    fn create_workers(
        mut options: Options,
        config: &Config,
        create_queue: impl Fn() -> Queues,
    ) -> Result<Vec<PoolSocket>> {
        let mut sockets = vec![];

        let shared_queue = if config.reuse_port {
            // if we are reusing the port, we need to share the queue_ids
            Some(create_queue())
        } else {
            // otherwise, each worker can get its own queue to reduce thread contention
            None
        };

        let socket_count = config.socket_count();

        for i in 0..socket_count {
            let socket = if i == 0 && socket_count > 1 {
                if config.reuse_port {
                    // set reuse port after we bind for the first socket
                    options.reuse_port = ReusePort::AfterBind;
                }
                let socket = options.build_udp()?;

                if config.reuse_port {
                    // for any additional sockets, set reuse port before bind
                    options.reuse_port = ReusePort::BeforeBind;

                    // in case the application bound to a wildcard, resolve the local address
                    options.addr = socket.local_addr()?;
                }

                socket
            } else {
                options.build_udp()?
            };

            let queue = if let Some(shared_queue) = &shared_queue {
                shared_queue.clone()
            } else {
                create_queue()
            };

            let queue = Mutex::new(queue);
            let socket = PoolSocket::new(socket, queue, config);

            sockets.push(socket);
        }

        Ok(sockets)
    }
}
