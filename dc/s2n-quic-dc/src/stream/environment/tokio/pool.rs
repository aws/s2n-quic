// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::udp::{ApplicationSocket, RecvSocket, WorkerSocket};
use crate::{
    event,
    path::secret::Map,
    socket::recv::{pool::Pool as Packets, router::Router, udp},
    stream::{
        environment::tokio::Environment,
        recv::dispatch::{Allocator as Queues, Control, Stream},
        server::{accept, udp::Acceptor},
        socket::{application::Single, fd::udp::CachedAddr, SendOnly, Tracing},
    },
    sync::ring_deque::Capacity,
};
use s2n_quic_platform::socket::options::{Options, ReusePort};
use std::{
    io::{self, Result},
    net::{SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tokio::io::unix::AsyncFd;
use tracing::Instrument;

#[derive(Clone)]
#[non_exhaustive]
pub struct Config {
    pub blocking: bool,
    pub reuse_port: bool,
    pub stream_queue: Capacity,
    pub control_queue: Capacity,
    pub max_packet_size: u16,
    pub packet_count: usize,
    pub accept_flavor: accept::Flavor,
    pub workers: Option<usize>,
    pub map: Map,
}

impl Config {
    pub fn new(map: Map) -> Self {
        Self {
            blocking: false,

            reuse_port: false,

            // TODO tune these defaults
            stream_queue: Capacity {
                max: u32::MAX as _,
                initial: 256,
            },

            // set the control queue depth shallow, since we really only need the most recent ones
            control_queue: Capacity { max: 8, initial: 8 },

            // Allocate 1MB at a time
            max_packet_size: u16::MAX,
            packet_count: 16,

            accept_flavor: accept::Flavor::default(),

            workers: None,
            map,
        }
    }
}

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

        let local_addr = recv_socket.local_addr()?;

        let send_socket = Tracing(SendOnly(CachedAddr::new(recv_socket.clone(), local_addr)));

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
        workers: usize,
        mut config: Config,
        acceptor: Option<accept::Sender<Sub>>,
    ) -> Result<Self>
    where
        Sub: event::Subscriber + Clone,
    {
        debug_assert_ne!(workers, 0);

        let workers = workers.next_power_of_two();
        let mask = workers - 1;

        let mut options = env.socket_options.clone();
        options.blocking = config.blocking;

        if config.workers.is_none() {
            config.workers = Some(workers);
        }
        let sockets = Self::create_workers(options, &config)?;

        let local_addr = if config.reuse_port {
            let addr = sockets[0].recv_socket.local_addr()?;
            if cfg!(debug_assertions) {
                for socket in sockets.iter().skip(1) {
                    debug_assert_eq!(addr, socket.recv_socket.local_addr()?);
                }
            }
            Some(addr)
        } else {
            None
        };

        let max_packet_size = config.max_packet_size;
        let packet_count = config.packet_count;
        let create_packets = || Packets::new(max_packet_size, packet_count);

        macro_rules! spawn {
            ($create_router:expr) => {
                if config.blocking {
                    Self::spawn_blocking(&sockets, create_packets, $create_router)
                } else {
                    let _rt = env.reader_rt.enter();
                    Self::spawn_non_blocking(&sockets, create_packets, $create_router)?;
                }
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
            local_addr,
        })
    }

    pub fn alloc(&self) -> (Control, Stream, ApplicationSocket, WorkerSocket) {
        let idx = self.current.fetch_add(1, Ordering::Relaxed);
        let idx = idx & self.mask;
        let socket = &self.sockets[idx];
        let (control, stream) = socket.queue.lock().unwrap().alloc_or_grow();
        let app_socket = socket.application_socket.clone();
        let worker_socket = socket.worker_socket.clone();

        (control, stream, app_socket, worker_socket)
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    fn spawn_blocking<R>(
        sockets: &[Socket],
        create_packets: impl Fn() -> Packets,
        create_router: impl Fn(&Packets, &Socket) -> R,
    ) where
        R: 'static + Send + Router,
    {
        for (udp_socket_worker, socket) in sockets.iter().enumerate() {
            let packets = create_packets();
            let router = create_router(&packets, socket);
            let recv_socket = socket.recv_socket.clone();
            let span = tracing::trace_span!("udp_socket_worker", udp_socket_worker);
            std::thread::spawn(move || {
                let _span = span.entered();
                udp::blocking(recv_socket, packets, router);
            });
        }
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
            let recv_socket = AsyncFd::new(socket.recv_socket.clone())?;
            let span = tracing::trace_span!("udp_socket_worker", udp_socket_worker);
            let task = async move {
                udp::non_blocking(recv_socket, packets, router).await;
            };
            if span.is_disabled() {
                tokio::spawn(task);
            } else {
                tokio::spawn(task.instrument(span));
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
                Queues::new(stream_cap, control_cap)
            };

            let socket = Socket::new(socket, queue)?;

            sockets.push(socket);
        }

        Ok(sockets)
    }
}
