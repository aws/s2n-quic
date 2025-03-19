// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{
        environment::{tokio::Environment, Peer, SetupResult, SocketSet},
        recv::{buffer, dispatch::Control, shared::RecvBuffer},
        socket::{
            self,
            application::{builder::TokioUdpSocket, Single},
            fd::udp::CachedAddr,
            SendOnly, Socket as _, Tracing,
        },
        TransportFeatures,
    },
};
use s2n_quic_core::{
    ensure,
    inet::{SocketAddress, Unspecified},
};
use std::{io, net::UdpSocket, sync::Arc};
use tokio::io::unix::AsyncFd;

pub(super) type RecvSocket = Arc<UdpSocket>;
pub(super) type WorkerSocket = Arc<Tracing<SendOnly<CachedAddr<RecvSocket>>>>;
pub(super) type ApplicationSocket = Arc<Single<Tracing<SendOnly<CachedAddr<RecvSocket>>>>>;
type OwnedSocket = AsyncFd<Arc<CachedAddr<UdpSocket>>>;

#[derive(Debug)]
pub struct Owned(pub SocketAddress, pub RecvBuffer);

impl<Sub> Peer<Environment<Sub>> for Owned
where
    Sub: event::Subscriber,
{
    type ReadWorkerSocket = OwnedSocket;
    type WriteWorkerSocket = (OwnedSocket, buffer::Local);

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
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

        let local_addr = writer.local_addr()?;
        let writer = Arc::new(CachedAddr::new(writer, local_addr));
        let reader = Arc::new(CachedAddr::new(reader, local_addr));

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

        let application = Box::new(TokioUdpSocket(reader));

        let read_worker = Some(read_worker);
        let write_worker_buffer = crate::msg::recv::Message::new(u16::MAX);
        let write_worker_buffer = buffer::Local::new(write_worker_buffer, None);
        let write_worker = Some((write_worker, write_worker_buffer));

        let socket = SocketSet {
            application,
            read_worker,
            write_worker,
            remote_addr,
            source_queue_id: None,
        };

        Ok((socket, recv_buffer))
    }
}

#[derive(Debug)]
pub struct Pooled(pub SocketAddress);

impl<Sub> Peer<Environment<Sub>> for Pooled
where
    Sub: event::Subscriber,
{
    type ReadWorkerSocket = WorkerSocket;
    type WriteWorkerSocket = (WorkerSocket, buffer::Channel<Control>);

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
    }

    #[inline]
    fn setup(
        self,
        env: &Environment<Sub>,
    ) -> SetupResult<Self::ReadWorkerSocket, Self::WriteWorkerSocket> {
        let peer_addr = self.0;
        let recv_pool = env.recv_pool.as_ref().expect("pool not configured");
        let (control, stream, application_socket, worker_socket) = recv_pool.alloc();
        crate::stream::environment::udp::Pooled {
            peer_addr,
            control,
            stream,
            application_socket,
            worker_socket,
        }
        .setup(env)
    }
}
