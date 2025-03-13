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
        socket::{application::Single, fd::udp::CachedAddr, SendOnly, Tracing},
    },
    sync::ring_deque::Capacity,
};
use s2n_quic_platform::socket::options::{Options, ReusePort};
use std::{
    io,
    io::Result,
    net::UdpSocket,
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
                max: 4096,
                initial: 256,
            },

            // set the control queue depth shallow, since we really only need the most recent ones
            control_queue: Capacity { max: 8, initial: 8 },

            // Allocate 1MB at a time
            max_packet_size: u16::MAX,
            packet_count: 16,

            workers: None,
            map,
        }
    }
}

pub(super) struct Pool {
    sockets: Box<[Socket]>,
    current: AtomicUsize,
    mask: usize,
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
    pub fn new<Sub>(env: &Environment<Sub>, workers: usize, mut config: Config) -> Result<Self>
    where
        Sub: event::Subscriber,
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

        let max_packet_size = config.max_packet_size;
        let packet_count = config.packet_count;
        let create_packets = || Packets::new(max_packet_size, packet_count);

        if config.blocking {
            Self::spawn_blocking(&config.map, &sockets, create_packets)
        } else {
            let _rt = env.reader_rt.enter();
            Self::spawn_non_blocking(&config.map, &sockets, create_packets)?;
        }

        Ok(Self {
            sockets: sockets.into(),
            current: AtomicUsize::new(0),
            mask,
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

    fn spawn_blocking(map: &Map, sockets: &[Socket], create_packets: impl Fn() -> Packets) {
        for (udp_socket_worker, socket) in sockets.iter().enumerate() {
            let dispatch = socket.queue.lock().unwrap().dispatcher();
            let dispatch = dispatch.with_map(map.clone());
            let socket = socket.recv_socket.clone();
            let packets = create_packets();
            let span = tracing::trace_span!("udp_socket_worker", udp_socket_worker);
            std::thread::spawn(move || {
                let _span = span.entered();
                udp::blocking(socket, packets, dispatch);
            });
        }
    }

    fn spawn_non_blocking(
        map: &Map,
        sockets: &[Socket],
        create_packets: impl Fn() -> Packets,
    ) -> Result<()> {
        for (udp_socket_worker, socket) in sockets.iter().enumerate() {
            let dispatch = socket.queue.lock().unwrap().dispatcher();
            let dispatch = dispatch.with_map(map.clone());
            let socket = AsyncFd::new(socket.recv_socket.clone())?;
            let packets = create_packets();
            let span = tracing::trace_span!("udp_socket_worker", udp_socket_worker);
            let task = async move {
                udp::non_blocking(socket, packets, dispatch).await;
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
