// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::udp::{ApplicationSocket, WorkerSocket};
use crate::{
    credentials::Credentials,
    event,
    stream::{
        environment::{turmoil::Environment, udp::Config},
        recv::dispatch::{Allocator as Queues, Control, Stream},
        server::accept,
        socket::{application::Single, Tracing},
    },
};
use std::{
    io::{self, Result},
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::OnceCell;
use turmoil::net::UdpSocket;

pub(super) struct Pool {
    sockets: OnceCell<Box<[Socket]>>,
    current: AtomicUsize,
    mask: usize,
    local_addr: SocketAddr,
    config: Config,
}

struct Socket {
    application_socket: ApplicationSocket,
    worker_socket: WorkerSocket,
    queue: Mutex<Queues>,
}

impl Socket {
    fn new(socket: UdpSocket, queue: Queues) -> io::Result<Self> {
        let recv_socket = Arc::new(socket);
        let send_socket = Tracing(recv_socket);
        let application_socket = Arc::new(Single(send_socket.clone()));
        let worker_socket = Arc::new(send_socket);

        Ok(Socket {
            application_socket,
            worker_socket,
            queue: Mutex::new(queue),
        })
    }
}

impl Pool {
    pub fn new<Sub>(
        _env: &Environment<Sub>,
        workers: usize,
        config: Config,
        _acceptor: Option<accept::Sender<Sub>>,
        addr: SocketAddr,
    ) -> Result<Self>
    where
        Sub: event::Subscriber + Clone,
    {
        debug_assert_ne!(workers, 0);

        let mask = if workers > 1 {
            workers.next_power_of_two() - 1
        } else {
            0
        };

        Ok(Self {
            sockets: OnceCell::new(),
            current: AtomicUsize::new(0),
            mask,
            local_addr: addr,
            config,
        })
    }

    pub async fn ensure_initialized(&self) -> io::Result<()> {
        self.sockets
            .get_or_try_init(|| async {
                let sockets = Self::create_workers(&self.config, self.local_addr).await?;
                io::Result::Ok(sockets.into_boxed_slice())
            })
            .await?;
        Ok(())
    }

    pub fn alloc(
        &self,
        credentials: Option<&Credentials>,
    ) -> (Control, Stream, ApplicationSocket, WorkerSocket) {
        // For turmoil, sockets must be initialized via ensure_initialized() first
        // This is called from an async context before alloc is used
        let sockets = self
            .sockets
            .get()
            .expect("Pool must be initialized before alloc - call ensure_initialized() first");

        let idx = self.current.fetch_add(1, Ordering::Relaxed);
        let idx = idx & self.mask;
        let socket = &sockets[idx];
        let (control, stream) = socket.queue.lock().unwrap().alloc_or_grow(credentials);
        let app_socket = socket.application_socket.clone();
        let worker_socket = socket.worker_socket.clone();

        (control, stream, app_socket, worker_socket)
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    async fn create_workers(config: &Config, addr: SocketAddr) -> Result<Vec<Socket>> {
        let stream_cap = config.stream_queue;
        let control_cap = config.control_queue;

        let socket = UdpSocket::bind(addr).await?;
        let queue = Queues::new(stream_cap, control_cap);
        let socket = Socket::new(socket, queue)?;

        Ok(vec![socket])
    }
}
