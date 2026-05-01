// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::udp::{ApplicationSendSocket, ArcSocket, WorkerSendSocket};
use crate::{
    clock::bach::Clock,
    credentials::Credentials,
    event,
    socket::{
        pool::{self, Pool as Packets},
        recv::{router::Router, udp},
        send,
    },
    stream::{
        self,
        environment::{
            bach::Environment,
            udp::{Config, Workers},
            Environment as _,
        },
        load_balance::PickTwo,
        recv::dispatch::{Allocator as Queues, Control, Stream},
        server::{accept, udp::Acceptor},
        socket::{application::Single, Tracing},
    },
};
use bach::{
    net::{socket, SocketAddr, UdpSocket},
    rand::Any,
};
use s2n_quic_platform::socket::options::{Options, ReusePort};
use std::{
    io::Result,
    sync::{Arc, Mutex},
};
use tracing::Instrument;

pub(super) struct Pool {
    sockets: Box<[PoolSocket]>,
    transmission_pool: pool::Pool,
    local_addr: SocketAddr,
    load_balancer: PickTwo,
}

struct PoolSocket {
    socket: ArcSocket,
    worker: WorkerSendSocket,
    application: Box<[ApplicationSendSocket]>,
    recv_queue: Mutex<Queues>,
}

impl PoolSocket {
    fn new(socket: Arc<UdpSocket>, recv_queue: Mutex<Queues>, config: &Config) -> Self {
        let local_addr = socket.local_addr().unwrap();

        let create_socket = || {
            // TODO: Reimplement with new channel-based wheel
            // let wheel = send::wheel::Wheel::new(...);
            // stream::socket::Wheel::new(wheel, local_addr)
            todo!("create_socket needs channel-based wheel")
        };

        let worker = Arc::new(Tracing(create_socket()));

        let application = (0..config.priority_levels)
            .map(|_| Arc::new(Single(Tracing(create_socket()))))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            worker,
            application,
            socket,
            recv_queue,
        }
    }

    fn spawn_send_worker(&self, _config: &Config, _clock: Clock) {
        // TODO: Reimplement with new channel-based wheel architecture
        // let socket = Tracing(self.socket.clone());
        //
        // let mut wheels = vec![send::wheel::Wheel::clone(&self.worker)];
        //
        // for application in &self.application {
        //     wheels.push(send::wheel::Wheel::clone(application));
        // }
        //
        // let rate = config.rate();
        //
        // let span = tracing::trace_span!("send_socket_worker");
        // let task = send::udp::non_blocking(socket, wheels, clock, rate);
        //
        // if span.is_disabled() {
        //     bach::spawn(task);
        // } else {
        //     bach::spawn(task.instrument(span));
        // }
        todo!("Reimplement spawn_send_worker with new channel-based wheel")
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

        assert!(
            matches!(config.send_workers, Workers::Environment(_)),
            "bach only supports environment socket workers"
        );
        assert!(
            matches!(config.recv_workers, Workers::Environment(_)),
            "bach only supports environment socket workers"
        );

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

        let transmission_pool = config.tx_packet_pool();

        let unroutable_packets = {
            // TODO pace these packets
            let socket = Tracing(sockets[0].socket.clone());
            let (tx, task) = config.unroutable_packets(socket);

            bach::spawn(task);

            tx
        };

        macro_rules! spawn {
            ($create_router:expr) => {
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
                let secret_socket = Tracing(sockets[0].socket.clone());

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
            |upper_bound| (0..upper_bound).any(),
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

    fn spawn_non_blocking<Sub, R>(
        env: &Environment<Sub>,
        config: &Config,
        sockets: &[PoolSocket],
        create_router: impl Fn(&Packets, &PoolSocket) -> R,
    ) -> Result<()>
    where
        Sub: event::Subscriber + Clone,
        R: 'static + Send + Router,
    {
        for (udp_socket_worker, socket) in sockets.iter().enumerate() {
            let packets = config.rx_packet_pool();
            let router = create_router(&packets, socket);
            let recv_socket = socket.socket.clone();
            let span = tracing::trace_span!("udp_socket_worker", udp_socket_worker);
            let task = async move {
                udp::non_blocking(recv_socket, packets, router).await;
            };
            if span.is_disabled() {
                bach::spawn(task);
            } else {
                bach::spawn(task.instrument(span));
            }

            socket.spawn_send_worker(config, env.clock())
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
                let socket = build_udp(&options)?;

                if config.reuse_port {
                    // for any additional sockets, set reuse port before bind
                    options.reuse_port = ReusePort::BeforeBind;

                    // in case the application bound to a wildcard, resolve the local address
                    options.addr = socket.local_addr()?;
                }

                socket
            } else {
                build_udp(&options)?
            };

            let queue = if let Some(shared_queue) = &shared_queue {
                shared_queue.clone()
            } else {
                create_queue()
            };

            let socket = Arc::new(socket);
            let queue = Mutex::new(queue);
            let socket = PoolSocket::new(socket, queue, config);

            sockets.push(socket);
        }

        Ok(sockets)
    }
}

fn build_udp(options: &Options) -> Result<UdpSocket> {
    let mut opts = socket::Options::default();
    opts.local_addr = options.addr;
    opts.reuse_port = !matches!(options.reuse_port, ReusePort::Disabled);

    // TODO send buffer, recv buffer

    let socket = opts.build_udp()?;
    Ok(socket)
}
