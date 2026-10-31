// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::udp::{ApplicationSocket, RecvSocket, WorkerSocket};
use crate::{
    credentials::Credentials,
    event,
    socket::recv::{descriptor, pool::Pool as Packets, router::Router, udp},
    stream::{
        environment::{tokio::Environment, udp::Config},
        recv::dispatch::{Allocator as Queues, Control, Dispatch, Stream},
        server::{accept, udp::Acceptor},
        socket::{application::Single, fd::udp::CachedAddr, SendOnly, Tracing},
    },
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

pub(super) struct Pool {
    sockets: Box<[Socket]>,
    current: AtomicUsize,
    mask: usize,
    local_addr: SocketAddr,
    // Serialize credential lookup and allocation to prevent duplicate streams.
    handoff: Mutex<()>,
}

/// Selects the receive queue for a forwarded packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketKind {
    Stream,
    Control,
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

/// Routes queue-zero packets to existing streams by credentials.
/// Clients lack the application's queue ID until its first reply.
struct ByCredentials(Dispatch);

impl Router for ByCredentials {
    #[inline]
    fn is_open(&self) -> bool {
        self.0.is_open()
    }

    #[inline]
    fn handle_stream_packet(
        &mut self,
        _remote_address: s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: crate::packet::stream::decoder::Packet,
    ) {
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        _tag: crate::packet::stream::Tag,
        _id: crate::packet::stream::Id,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        if let Some(queue_id) = self.0.queue_id_for_key(&credentials) {
            let _ = self.0.send_stream(queue_id, segment);
        }
    }

    #[inline]
    fn handle_control_packet(
        &mut self,
        _remote_address: s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: crate::packet::control::decoder::Packet,
    ) {
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        _tag: crate::packet::control::Tag,
        _id: Option<crate::packet::stream::Id>,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        if let Some(queue_id) = self.0.queue_id_for_key(&credentials) {
            let _ = self.0.send_control(queue_id, segment);
        }
    }
}

impl Pool {
    #[allow(clippy::unwrap_in_result, reason = "see unwrap_used lints below")]
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

        let mut options = env.socket_options.clone();
        options.blocking = config.blocking;

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
                #[expect(
                    clippy::unwrap_used,
                    reason = "lock is only poisoned if another thread already panicked while holding it"
                )]
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
        } else if config.route_by_credentials {
            spawn!(|_packets: &Packets, socket: &Socket| {
                #[expect(
                    clippy::unwrap_used,
                    reason = "lock is only poisoned if another thread already panicked while holding it"
                )]
                let dispatch = socket.queue.lock().unwrap().dispatcher();
                let zero = ByCredentials(dispatch.clone());
                dispatch.with_map(config.map.clone()).with_zero_router(zero)
            });
        } else {
            spawn!(|_packets: &Packets, socket: &Socket| {
                #[expect(
                    clippy::unwrap_used,
                    reason = "lock is only poisoned if another thread already panicked while holding it"
                )]
                let dispatch = socket.queue.lock().unwrap().dispatcher();
                dispatch.with_map(config.map.clone())
            });
        }

        Ok(Self {
            sockets: sockets.into(),
            current: AtomicUsize::new(0),
            mask,
            local_addr,
            handoff: Mutex::new(()),
        })
    }

    /// Routes to an existing stream, returning `false` if none is registered.
    pub fn forward(
        &self,
        credentials: &Credentials,
        kind: PacketKind,
        segment: descriptor::Filled,
    ) -> bool {
        #[expect(
            clippy::unwrap_used,
            reason = "lock is only poisoned if another thread already panicked while holding it"
        )]
        let _guard = self.handoff.lock().unwrap();
        self.send_to_existing(credentials, kind, segment).is_none()
    }

    /// Routes `segment` to the stream registered for `credentials`, or allocates a new stream
    /// for it and returns the stream's queues and sockets.
    pub fn handoff(
        &self,
        credentials: &Credentials,
        segment: descriptor::Filled,
    ) -> Option<(Control, Stream, ApplicationSocket, WorkerSocket)> {
        #[expect(
            clippy::unwrap_used,
            reason = "lock is only poisoned if another thread already panicked while holding it"
        )]
        let _guard = self.handoff.lock().unwrap();
        let segment = self.send_to_existing(credentials, PacketKind::Stream, segment)?;
        let (control, stream, app_socket, worker_socket) = self.alloc(Some(credentials));
        let _ = stream.push(segment);
        Some((control, stream, app_socket, worker_socket))
    }

    /// Sends to a registered stream, returning the packet if none exists.
    fn send_to_existing(
        &self,
        credentials: &Credentials,
        kind: PacketKind,
        segment: descriptor::Filled,
    ) -> Option<descriptor::Filled> {
        for socket in self.sockets.iter() {
            #[expect(
                clippy::unwrap_used,
                reason = "lock is only poisoned if another thread already panicked while holding it"
            )]
            let mut dispatch = socket.queue.lock().unwrap().dispatcher();
            if let Some(queue_id) = dispatch.queue_id_for_key(credentials) {
                let _ = match kind {
                    PacketKind::Stream => dispatch.send_stream(queue_id, segment),
                    PacketKind::Control => dispatch.send_control(queue_id, segment),
                };
                return None;
            }
        }
        Some(segment)
    }

    pub fn alloc(
        &self,
        credentials: Option<&Credentials>,
    ) -> (Control, Stream, ApplicationSocket, WorkerSocket) {
        let idx = self.current.fetch_add(1, Ordering::Relaxed);
        let idx = idx & self.mask;
        let socket = &self.sockets[idx];
        #[expect(
            clippy::unwrap_used,
            reason = "lock is only poisoned if another thread already panicked while holding it"
        )]
        let (control, stream) = socket.queue.lock().unwrap().alloc_or_grow(credentials);
        let app_socket = socket.application_socket.clone();
        let worker_socket = socket.worker_socket.clone();

        (control, stream, app_socket, worker_socket)
    }

    pub fn local_addr(&self) -> SocketAddr {
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
            std::thread::Builder::new()
                .name(format!("udp-worker-{udp_socket_worker}"))
                .spawn(move || {
                    let _span = span.entered();
                    udp::blocking(recv_socket, packets, router);
                })
                .expect("failed to spawn udp worker thread");
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
            let recv_socket = unsafe {
                // SAFETY: the Arc keeps the socket alive, so its fd is stable until drop.
                AsyncFd::register(socket.recv_socket.clone())
            }?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet;
    use s2n_codec::{Encoder, EncoderBuffer};
    use s2n_quic_core::{inet::SocketAddress, varint::VarInt};

    fn credentials(key_id: u8) -> Credentials {
        Credentials {
            id: Default::default(),
            key_id: key_id.into(),
        }
    }

    fn pool() -> Pool {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let local_addr = socket.local_addr().unwrap();
        let socket = Socket::new(socket, Queues::new_non_zero(8, 8)).unwrap();
        Pool {
            sockets: vec![socket].into_boxed_slice(),
            current: AtomicUsize::new(0),
            mask: 0,
            local_addr,
            handoff: Mutex::new(()),
        }
    }

    fn segment(payload: &[u8]) -> descriptor::Filled {
        let mut packets = Packets::new(1024, 1);
        packets
            .alloc_or_grow()
            .recv_with(|addr, _cmsg, mut buffer| {
                let remote: SocketAddress = "127.0.0.1:1234".parse::<SocketAddr>().unwrap().into();
                addr.set(remote);
                buffer[..payload.len()].copy_from_slice(payload);
                Ok::<_, io::Error>(payload.len())
            })
            .unwrap()
            .next()
            .unwrap()
    }

    // Only structural validity is needed here; the stream authenticates queued packets later.
    fn packet(kind: PacketKind, credentials: &Credentials, queue_id: VarInt) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        let mut encoder = EncoderBuffer::new(&mut bytes);
        let id = packet::stream::Id::normal(queue_id).unwrap();
        match kind {
            PacketKind::Stream => encoder.encode(&packet::stream::Tag::default()),
            PacketKind::Control => {
                let mut tag = packet::control::Tag::default();
                tag.set_is_stream(true);
                encoder.encode(&tag);
            }
        }
        encoder.encode(credentials);
        encoder.encode(&packet::WireVersion::ZERO);
        if kind == PacketKind::Stream {
            encoder.encode(&0u16);
        }
        encoder.encode(&id);
        encoder.encode(&VarInt::ZERO); // packet number
        if kind == PacketKind::Stream {
            encoder.encode(&0u32); // retransmission offset
            encoder.encode(&VarInt::ZERO); // next expected control packet
            encoder.encode(&VarInt::ZERO); // stream offset
        }
        encoder.encode(&VarInt::ZERO); // payload or control-data length
        encoder.write_repeated(16, 0);
        let len = encoder.len();
        bytes.truncate(len);
        bytes
    }

    #[test]
    fn forwarded_packets_select_existing_stream_and_control_queues() {
        let pool = pool();
        let credentials = credentials(0);
        let (control, stream, _, _) = pool.alloc(Some(&credentials));
        assert_ne!(stream.queue_id(), VarInt::ZERO);

        for queue_id in [VarInt::ZERO, 99u8.into()] {
            let bytes = packet(PacketKind::Stream, &credentials, queue_id);
            assert!(pool.forward(&credentials, PacketKind::Stream, segment(&bytes)));
            assert_eq!(stream.try_recv().unwrap().unwrap().payload(), bytes);
            assert!(control.try_recv().unwrap().is_none());

            let bytes = packet(PacketKind::Control, &credentials, queue_id);
            assert!(pool.forward(&credentials, PacketKind::Control, segment(&bytes)));
            assert_eq!(control.try_recv().unwrap().unwrap().payload(), bytes);
            assert!(stream.try_recv().unwrap().is_none());
        }
    }

    #[test]
    fn forwards_do_not_allocate_unknown_or_closed_streams() {
        let pool = pool();
        let unknown = credentials(1);
        for kind in [PacketKind::Stream, PacketKind::Control] {
            assert!(!pool.forward(&unknown, kind, segment(b"unknown")));
        }
        assert_eq!(pool.current.load(Ordering::Relaxed), 0);

        let registered = credentials(0);
        let (control, stream, _, _) = pool.alloc(Some(&registered));
        for kind in [PacketKind::Stream, PacketKind::Control] {
            assert!(!pool.forward(&unknown, kind, segment(b"unknown")));
        }
        assert!(control.try_recv().unwrap().is_none());
        assert!(stream.try_recv().unwrap().is_none());
        drop(control);
        drop(stream);

        for kind in [PacketKind::Stream, PacketKind::Control] {
            assert!(!pool.forward(&registered, kind, segment(b"closed")));
        }
        assert_eq!(pool.current.load(Ordering::Relaxed), 1);
        assert_eq!(
            pool.sockets[0]
                .queue
                .lock()
                .unwrap()
                .dispatcher()
                .queue_id_for_key(&registered),
            None
        );
    }

    #[test]
    fn direct_queue_zero_packets_route_by_credentials_without_allocation() {
        let registered = credentials(0);
        let unknown = credentials(1);
        let mut queues = Queues::new_non_zero(8, 8);
        let (control, stream) = queues.alloc_or_grow(Some(&registered));
        let mut router = queues
            .dispatcher()
            .with_zero_router(ByCredentials(queues.dispatcher()));

        for (queue_id, credentials) in [
            (VarInt::ZERO, registered),
            (stream.queue_id(), registered),
            (VarInt::ZERO, unknown),
        ] {
            for kind in [PacketKind::Stream, PacketKind::Control] {
                let bytes = packet(kind, &credentials, queue_id);
                router.on_segment(segment(&bytes));
                let received = match kind {
                    PacketKind::Stream => stream.try_recv().unwrap(),
                    PacketKind::Control => control.try_recv().unwrap(),
                };
                if credentials == registered {
                    assert_eq!(received.unwrap().payload(), bytes);
                } else {
                    assert!(received.is_none());
                    assert_eq!(queues.dispatcher().queue_id_for_key(&unknown), None);
                }
            }
        }
        drop(control);
        drop(stream);
        for kind in [PacketKind::Stream, PacketKind::Control] {
            router.on_segment(segment(&packet(kind, &registered, VarInt::ZERO)));
        }
        assert_eq!(queues.dispatcher().queue_id_for_key(&registered), None);
    }
}
