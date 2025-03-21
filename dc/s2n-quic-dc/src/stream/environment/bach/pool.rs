// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::udp::{ApplicationSocket, RecvSocket, WorkerSocket};
use crate::{
    credentials::Credentials,
    event,
    socket::recv::{pool::Pool as Packets, router::Router, udp},
    stream::{
        environment::{bach::Environment, udp::Config},
        recv::dispatch::{Allocator as Queues, Control, Stream},
        server::{accept, udp::Acceptor},
        socket::{application::Single, Tracing},
    },
};
use bach::net::{socket, SocketAddr, UdpSocket};
use s2n_quic_platform::socket::options::{Options, ReusePort};
use std::{
    io::{self, Result},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tracing::Instrument;

pub(super) struct Pool {
    sockets: Box<[Socket]>,
    current: AtomicUsize,
    mask: usize,
    /// The local address if `reuse_port` was enabled
    local_addr: Option<SocketAddr>,
}

struct Socket {
    recv_socket: RecvSocket,
    application_socket: ApplicationSocket,
    worker_socket: WorkerSocket,
    queue: Mutex<Queues>,
}

impl Socket {
    fn new(socket: UdpSocket, queue: Queues) -> io::Result<Self> {
        let recv_socket = Arc::new(socket);

        let send_socket = Tracing(recv_socket.clone());

        let application_socket = Arc::new(Single(send_socket.clone()));

        let worker_socket = Arc::new(send_socket);

        Ok(Socket {
            recv_socket,
            application_socket,
            worker_socket,
            queue: Mutex::new(queue),
        })
    }
}

impl Pool {
    pub fn new<Sub>(
        env: &Environment<Sub>,
        mut workers: usize,
        mut config: Config,
        acceptor: Option<accept::Sender<Sub>>,
    ) -> Result<Self>
    where
        Sub: event::Subscriber + Clone,
    {
        debug_assert_ne!(workers, 0);

        if workers > 1 {
            workers = workers.next_power_of_two();
        }

        let mask = workers - 1;

        let options = env.socket_options.clone();

        if config.workers.is_none() {
            config.workers = Some(workers);
        }

        let sockets = Self::create_workers(options, &config)?;

        let local_addr = sockets[0].recv_socket.local_addr()?;

        if cfg!(debug_assertions) && config.reuse_port {
            for socket in sockets.iter().skip(1) {
                debug_assert_eq!(local_addr, socket.recv_socket.local_addr()?);
            }
        }

        let max_packet_size = config.max_packet_size;
        let packet_count = config.packet_count;
        let create_packets = || Packets::new(max_packet_size, packet_count);

        macro_rules! spawn {
            ($create_router:expr) => {
                Self::spawn_non_blocking(&sockets, create_packets, $create_router)?;
            };
        }

        if let Some(sender) = acceptor {
            spawn!(|_packets: &Packets, socket: &Socket| {
                let queues = socket.queue.lock().unwrap();
                let app_socket = socket.application_socket.clone();
                let worker_socket = socket.worker_socket.clone();

                let acceptor = Acceptor::new(
                    env.clone(),
                    sender.clone(),
                    config.map.clone(),
                    config.accept_flavor,
                    queues.clone(),
                    app_socket,
                    worker_socket,
                );

                let router = queues.dispatcher().with_map(config.map.clone());
                router.with_zero_router(acceptor)
            });
        } else {
            spawn!(|_packets: &Packets, socket: &Socket| {
                let dispatch = socket.queue.lock().unwrap().dispatcher();
                dispatch.with_map(config.map.clone())
            });
        }

        Ok(Self {
            sockets: sockets.into(),
            current: AtomicUsize::new(0),
            mask,
            local_addr: Some(local_addr),
        })
    }

    pub fn alloc(
        &self,
        credentials: Option<&Credentials>,
    ) -> (Control, Stream, ApplicationSocket, WorkerSocket) {
        let idx = self.current.fetch_add(1, Ordering::Relaxed);
        let idx = idx & self.mask;
        let socket = &self.sockets[idx];
        let (control, stream) = socket.queue.lock().unwrap().alloc_or_grow(credentials);
        let app_socket = socket.application_socket.clone();
        let worker_socket = socket.worker_socket.clone();

        (control, stream, app_socket, worker_socket)
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    fn spawn_non_blocking<R>(
        sockets: &[Socket],
        create_packets: impl Fn() -> Packets,
        create_router: impl Fn(&Packets, &Socket) -> R,
    ) -> Result<()>
    where
        R: 'static + Send + Router,
    {
        for (udp_socket_worker, socket) in sockets.iter().enumerate() {
            let packets = create_packets();
            let router = create_router(&packets, socket);
            let recv_socket = socket.recv_socket.clone();
            let span = tracing::trace_span!("udp_socket_worker", udp_socket_worker);
            let task = async move {
                udp::non_blocking(recv_socket, packets, router).await;
            };
            if span.is_disabled() {
                bach::spawn(task);
            } else {
                bach::spawn(task.instrument(span));
            }
        }
        Ok(())
    }

    fn create_workers(mut options: Options, config: &Config) -> Result<Vec<Socket>> {
        let mut sockets = vec![];

        let stream_cap = config.stream_queue;
        let control_cap = config.control_queue;

        let shared_queue = if config.reuse_port {
            // if we are reusing the port, we need to share the queue_ids
            Some(Queues::new_non_zero(stream_cap, control_cap))
        } else {
            // otherwise, each worker can get its own queue to reduce thread contention
            None
        };

        let workers = config.workers.unwrap_or(1).max(1);

        for i in 0..workers {
            let socket = if i == 0 && workers > 1 {
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
                Queues::new(stream_cap, control_cap)
            };

            let socket = Socket::new(socket, queue)?;

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
