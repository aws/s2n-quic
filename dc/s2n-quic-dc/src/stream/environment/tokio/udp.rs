// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{
        environment::{tokio::Environment, Peer, SetupResult, SocketSet},
        recv::{buffer, shared::RecvBuffer},
        socket::{self, Socket as _},
        TransportFeatures,
    },
};
use s2n_quic_core::{
    ensure,
    inet::{SocketAddress, Unspecified},
};
use std::{io, net::UdpSocket, sync::Arc};
use tokio::io::unix::AsyncFd;

type AsyncUdpSocket = AsyncFd<Arc<UdpSocket>>;

#[derive(Debug)]
pub struct Owned(pub SocketAddress, pub RecvBuffer);

impl<Sub> Peer<Environment<Sub>> for Owned
where
    Sub: event::Subscriber,
{
    type ReadWorkerSocket = AsyncUdpSocket;
    type WriteWorkerSocket = (AsyncUdpSocket, buffer::Local);

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
    }

    #[inline]
    fn with_source_control_port(&mut self, port: u16) {
        self.0.set_port(port);
    }

    #[inline]
    fn setup(
        self,
        env: &Environment<Sub>,
    ) -> SetupResult<Self::ReadWorkerSocket, Self::WriteWorkerSocket> {
        let remote_addr = self.0;
        let recv_buffer = self.1;
        let mut options = env.socket_options.clone();

        match remote_addr {
            SocketAddress::IpV6(_) if options.addr.is_ipv4() => {
                let addr: SocketAddress = options.addr.into();
                if addr.ip().is_unspecified() {
                    options.addr.set_ip(std::net::Ipv6Addr::UNSPECIFIED.into());
                } else {
                    let addr = addr.to_ipv6_mapped();
                    options.addr = addr.into();
                }
            }
            SocketAddress::IpV4(_) if options.addr.is_ipv6() => {
                let addr: SocketAddress = options.addr.into();
                if addr.ip().is_unspecified() {
                    options.addr.set_ip(std::net::Ipv4Addr::UNSPECIFIED.into());
                } else {
                    let addr = addr.unmap();
                    // ensure the local IP maps to v4, otherwise it won't bind correctly
                    ensure!(
                        matches!(addr, SocketAddress::IpV4(_)),
                        Err(io::ErrorKind::Unsupported.into())
                    );
                    options.addr = addr.into();
                }
            }
            _ => {}
        }

        let socket::Pair { writer, reader } = socket::Pair::open(options)?;

        let writer = Arc::new(writer);
        let reader = Arc::new(reader);

        let read_worker = {
            let _guard = env.reader_rt.enter();
            AsyncFd::new(reader.clone())?
        };

        let write_worker = {
            let _guard = env.writer_rt.enter();
            AsyncFd::new(writer.clone())?
        };

        debug_assert_eq!(
            read_worker.local_port()?,
            write_worker.local_port()?,
            "worker ports must match with owned socket implementation"
        );

        let source_control_port = write_worker.local_port()?;

        let application = Box::new(reader);

        let read_worker = Some(read_worker);
        let write_worker_buffer = crate::msg::recv::Message::new(u16::MAX);
        let write_worker_buffer = buffer::Local::new(write_worker_buffer, None);
        let write_worker = Some((write_worker, write_worker_buffer));

        let socket = SocketSet {
            application,
            read_worker,
            write_worker,
            remote_addr,
            source_control_port,
            source_queue_id: None,
        };

        Ok((socket, recv_buffer))
    }
}
