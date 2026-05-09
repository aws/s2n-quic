// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::socket::{BusyPoll, Gso as GsoSocket, Options, ReusePort};
use s2n_quic_platform::features;
use std::{io, net::SocketAddr};

const DEFAULT_BUFFER_SIZE: usize = 200 * 1024 * 1024;

/// Configuration for send socket creation.
pub struct SendConfig {
    pub num_sockets: usize,
    pub bind_addr: SocketAddr,
    pub gso: features::Gso,
    pub send_buffer: usize,
}

impl SendConfig {
    pub fn new(num_sockets: usize, bind_addr: SocketAddr, gso: features::Gso) -> Self {
        Self {
            num_sockets,
            bind_addr,
            gso,
            send_buffer: DEFAULT_BUFFER_SIZE,
        }
    }

    /// Creates send sockets with GSO support.
    ///
    /// Each socket binds to an ephemeral port on the given address. Recv buffer is zeroed
    /// since these sockets don't receive.
    pub fn create(&self) -> io::Result<Vec<GsoSocket<BusyPoll<std::net::UdpSocket>>>> {
        let mut sockets = Vec::with_capacity(self.num_sockets);

        let mut bind_addr = self.bind_addr;
        bind_addr.set_port(0);

        for _ in 0..self.num_sockets {
            let mut opts = Options::default();
            opts.addr = bind_addr;
            opts.blocking = false;
            opts.send_buffer = Some(self.send_buffer);
            opts.recv_buffer = Some(0);
            let socket = opts.build_udp()?;

            let socket = BusyPoll(socket);
            let socket = GsoSocket(socket, self.gso.clone());
            sockets.push(socket);
        }

        Ok(sockets)
    }
}

/// Configuration for receive socket creation.
pub struct RecvConfig {
    pub num_sockets: usize,
    pub bind_addr: SocketAddr,
    pub recv_buffer: usize,
}

impl RecvConfig {
    pub fn new(num_sockets: usize, bind_addr: SocketAddr) -> Self {
        Self {
            num_sockets,
            bind_addr,
            recv_buffer: DEFAULT_BUFFER_SIZE,
        }
    }

    /// Creates receive sockets with REUSEPORT for kernel-level load balancing.
    ///
    /// The first socket binds to the requested address (getting an ephemeral port if port is 0).
    /// Subsequent sockets share the same port via SO_REUSEPORT. GRO is enabled for coalescing
    /// received segments. Send buffer is zeroed since these sockets don't send.
    pub fn create(&self) -> io::Result<Vec<BusyPoll<std::net::UdpSocket>>> {
        let mut sockets = Vec::with_capacity(self.num_sockets);

        let mut opts = Options::default();
        opts.addr = self.bind_addr;
        if self.num_sockets > 1 {
            opts.reuse_address = true;
            opts.reuse_port = ReusePort::AfterBind;
        }
        opts.gro = true;
        opts.blocking = false;
        opts.recv_buffer = Some(self.recv_buffer);
        opts.send_buffer = Some(0);
        let first_socket = opts.build_udp()?;
        sockets.push(BusyPoll(first_socket));

        if self.num_sockets > 1 {
            let bound_addr = sockets[0].0.local_addr()?;
            assert_ne!(bound_addr.port(), 0);
            opts.reuse_port = ReusePort::BeforeBind;
            opts.addr = bound_addr;
            for _ in 1..self.num_sockets {
                sockets.push(BusyPoll(opts.build_udp()?));
            }
        }

        Ok(sockets)
    }
}
