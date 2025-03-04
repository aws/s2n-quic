// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Environment;
use crate::{
    event,
    path::secret::Map,
    socket::recv::{pool::Pool as Packets, router::Router, udp},
    stream::recv::dispatch::{Allocator as Queues, Control, Stream},
    sync::ring_deque::Capacity,
};
use s2n_quic_platform::socket::options::{Options, ReusePort};
use std::{
    io::Result,
    net::UdpSocket,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tokio::io::unix::AsyncFd;

pub struct Pool {
    // avoid allocating in parallel
    queues: Mutex<Queues>,
    sockets: Box<[Arc<UdpSocket>]>,
    current: AtomicUsize,
    mask: usize,
}

impl Pool {
    pub fn new<Sub>(
        env: &Environment<Sub>,
        workers: usize,
        map: Map,
        blocking: bool,
        reuse_port: bool,
    ) -> Result<Self>
    where
        Sub: event::Subscriber,
    {
        debug_assert_ne!(workers, 0);

        let workers = workers.next_power_of_two();
        let mask = workers - 1;

        let mut options = env.socket_options.clone();
        options.blocking = blocking;
        let sockets = create_workers(options, workers, reuse_port)?;

        // TODO tune these numbers
        let stream_cap = Capacity {
            initial: 256,
            max: 4096,
        };
        // set the control queue depth shallow, since we really only need the most recent ones
        let control_cap = Capacity { initial: 8, max: 8 };

        let queues = Queues::new_non_zero(stream_cap, control_cap);
        let dispatch = queues.dispatcher();
        let dispatch = dispatch.with_map(map);

        // Allocate 1MB at a time
        // TODO reduce is if GRO isn't supported
        let max_packet_size = u16::MAX;
        let packet_count = 16;
        let create_packets = || Packets::new(max_packet_size, packet_count);

        if blocking {
            Self::spawn_blocking(&sockets, dispatch, create_packets)
        } else {
            Self::spawn_non_blocking(env, &sockets, dispatch, create_packets)?;
        }

        Ok(Self {
            queues: Mutex::new(queues),
            sockets: sockets.into(),
            current: AtomicUsize::new(0),
            mask,
        })
    }

    pub fn alloc(&self) -> (Control, Stream, Arc<UdpSocket>) {
        let (control, stream) = self.queues.lock().unwrap().alloc_or_grow();

        let idx = self.current.fetch_add(1, Ordering::Relaxed);
        let idx = idx & self.mask;
        let socket = self.sockets[idx].clone();

        (control, stream, socket)
    }

    fn spawn_blocking(
        sockets: &[Arc<UdpSocket>],
        dispatch: impl Router + Clone + Send + 'static,
        create_packets: impl Fn() -> Packets,
    ) {
        for socket in sockets {
            let socket = socket.clone();
            let dispatch = dispatch.clone();
            let packets = create_packets();
            std::thread::spawn(move || {
                udp::blocking(socket, packets, dispatch);
            });
        }
    }

    fn spawn_non_blocking<Sub>(
        env: &Environment<Sub>,
        sockets: &[Arc<UdpSocket>],
        dispatch: impl Router + Clone + Send + 'static,
        create_packets: impl Fn() -> Packets,
    ) -> Result<()>
    where
        Sub: event::Subscriber,
    {
        for socket in sockets {
            let socket = AsyncFd::new(socket.clone())?;
            let dispatch = dispatch.clone();
            let packets = create_packets();
            env.reader_rt.spawn(async move {
                udp::non_blocking(socket, packets, dispatch).await;
            });
        }
        Ok(())
    }
}

fn create_workers(
    mut options: Options,
    workers: usize,
    reuse_port: bool,
) -> Result<Vec<Arc<UdpSocket>>> {
    let mut sockets = vec![];

    for i in 0..workers {
        let socket = if i == 0 && workers > 1 {
            if reuse_port {
                // set reuse port after we bind for the first socket
                options.reuse_port = ReusePort::AfterBind;
            }
            let socket = options.build_udp()?;

            if reuse_port {
                // for any additional sockets, set reuse port before bind
                options.reuse_port = ReusePort::BeforeBind;

                // in case the application bound to a wildcard, resolve the local address
                options.addr = socket.local_addr()?;
            }

            socket
        } else {
            options.build_udp()?
        };
        let socket = Arc::new(socket);
        sockets.push(socket);
    }

    Ok(sockets)
}
