// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Environment;
use crate::event;
use crate::socket::recv::udp::Allocator;
use s2n_quic_platform::socket::{options::Options, options::ReusePort};
use std::{io::Result, net::UdpSocket, sync::Arc};

#[derive(Clone)]
pub struct Pool {
    // TODO
}

impl Pool {
    pub fn new<Sub>(env: &Environment<Sub>, blocking: bool, workers: usize) -> Result<Self>
    where
        Sub: event::Subscriber,
    {
        let mut options = env.socket_options.clone();
        options.blocking = true;
        let sockets = create_workers(options, workers)?;

        // Allocate 1MB at a time
        // TODO reduce is if GRO isn't supported
        let max_packet_size = u16::MAX;
        let packet_count = 16;

        let allocator = Allocator::new(max_packet_size, packet_count);

        // TODO set up the router

        if blocking {
            Self::new_blocking(env, sockets)
        } else {
            Self::new_non_blocking(env, sockets)
        }
    }

    fn new_blocking<Sub>(env: &Environment<Sub>, workers: Vec<Arc<UdpSocket>>) -> Result<Self>
    where
        Sub: event::Subscriber,
    {
        todo!()
    }

    fn new_non_blocking<Sub>(env: &Environment<Sub>, workers: Vec<Arc<UdpSocket>>) -> Result<Self>
    where
        Sub: event::Subscriber,
    {
        todo!()
    }
}

fn create_workers(mut options: Options, workers: usize) -> Result<Vec<Arc<UdpSocket>>> {
    let mut sockets = vec![];

    for i in 0..workers {
        let socket = if i == 0 && workers > 1 {
            // set reuse port after we bind for the first socket
            options.reuse_port = ReusePort::AfterBind;
            let socket = options.build_udp()?;

            // for any additional sockets, set reuse port before bind
            options.reuse_port = ReusePort::BeforeBind;
            // in case the application bound to a wildcard, resolve the local address
            options.addr = socket.local_addr()?;

            socket
        } else {
            options.build_udp()?
        };
        let socket = Arc::new(socket);
        sockets.push(socket);
    }

    Ok(sockets)
}
