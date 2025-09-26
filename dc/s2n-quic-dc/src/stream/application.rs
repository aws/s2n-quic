// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::{self, EndpointPublisher as _},
    stream::{
        recv::application::{self as recv, Reader},
        send::application::{self as send, Writer},
        shared::ArcShared,
        socket,
    },
};
use core::{fmt, time::Duration};
use s2n_quic_core::{buffer, time::Timestamp};
use std::{io, net::SocketAddr};

pub struct Builder<Sub: event::Subscriber> {
    pub read: recv::Builder<Sub>,
    pub write: send::Builder<Sub>,
    pub shared: ArcShared<Sub>,
    pub sockets: Box<dyn socket::application::Builder>,
    pub queue_time: Timestamp,
}

impl<Sub> Builder<Sub>
where
    Sub: event::Subscriber,
{
    /// Builds the stream and emits an event indicating that the stream was built
    #[inline]
    pub(crate) fn accept(self) -> io::Result<(Stream<Sub>, Duration)> {
        let sojourn_time = {
            let remote_address = self.shared.remote_addr();
            let remote_address = &remote_address;
            let creds = self.shared.credentials();
            let credential_id = &*creds.id;
            let stream_id = creds.key_id.as_u64();
            let now = self.shared.common.clock.get_time();
            let sojourn_time = now.saturating_duration_since(self.queue_time);

            self.shared
                .endpoint_publisher(now)
                .on_acceptor_stream_dequeued(event::builder::AcceptorStreamDequeued {
                    remote_address,
                    credential_id,
                    stream_id,
                    sojourn_time,
                });

            // TODO emit event

            sojourn_time
        };

        self.build().map(|stream| (stream, sojourn_time))
    }

    #[inline]
    pub(crate) fn connect(self) -> io::Result<Stream<Sub>> {
        self.build()
    }

    #[inline]
    pub(crate) fn build(self) -> io::Result<Stream<Sub>> {
        let Self {
            read,
            write,
            shared,
            sockets,
            queue_time: _,
        } = self;

        // TODO emit event

        let sockets = sockets.build()?;
        let read = read.build(shared.clone(), sockets.clone());
        let write = write.build(shared, sockets);
        Ok(Stream { read, write })
    }

    /// Emits an event indicating that the stream was pruned
    #[inline]
    pub(crate) fn prune(self, reason: event::builder::AcceptorStreamPruneReason) {
        let now = self.shared.clock.get_time();
        let remote_address = self.shared.remote_addr();
        let remote_address = &remote_address;
        let creds = self.shared.credentials();
        let credential_id = &*creds.id;
        let stream_id = creds.key_id.as_u64();
        let sojourn_time = now.saturating_duration_since(self.queue_time);

        self.shared
            .endpoint_publisher(now)
            .on_acceptor_stream_pruned(event::builder::AcceptorStreamPruned {
                remote_address,
                credential_id,
                stream_id,
                sojourn_time,
                reason,
            });

        self.shared.receiver.on_prune();
        self.shared.sender.on_prune();
    }
}

pub struct Stream<Sub>
where
    Sub: event::Subscriber,
{
    read: Reader<Sub>,
    write: Writer<Sub>,
}

impl<Sub> fmt::Debug for Stream<Sub>
where
    Sub: event::Subscriber,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut s = f.debug_struct("Stream");

        for (name, addr) in [
            ("peer_addr", self.peer_addr()),
            ("local_addr", self.local_addr()),
        ] {
            if let Ok(addr) = addr {
                s.field(name, &addr);
            }
        }

        s.finish()
    }
}

impl<Sub> Stream<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.read.peer_addr()
    }

    #[inline]
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.read.local_addr()
    }

    #[inline]
    pub fn protocol(&self) -> socket::Protocol {
        self.read.protocol()
    }

    #[inline]
    pub fn set_read_mode(&mut self, read_mode: recv::ReadMode) -> &mut Self {
        self.read.set_read_mode(read_mode);
        self
    }

    #[inline]
    pub fn set_ack_mode(&mut self, ack_mode: recv::AckMode) -> &mut Self {
        self.read.set_ack_mode(ack_mode);
        self
    }

    #[inline]
    pub async fn write_from(
        &mut self,
        buf: &mut impl buffer::reader::storage::Infallible,
    ) -> io::Result<usize> {
        self.write.write_from(buf).await
    }

    #[inline]
    pub async fn write_all_from(
        &mut self,
        buf: &mut impl buffer::reader::storage::Infallible,
    ) -> io::Result<usize> {
        self.write.write_all_from(buf).await
    }

    #[inline]
    pub async fn write_from_fin(
        &mut self,
        buf: &mut impl buffer::reader::storage::Infallible,
    ) -> io::Result<usize> {
        self.write.write_from_fin(buf).await
    }

    #[inline]
    pub async fn write_all_from_fin(
        &mut self,
        buf: &mut impl buffer::reader::storage::Infallible,
    ) -> io::Result<usize> {
        self.write.write_all_from_fin(buf).await
    }

    #[inline]
    pub async fn read_into(
        &mut self,
        out_buf: &mut impl buffer::writer::Storage,
    ) -> io::Result<usize> {
        self.read.read_into(out_buf).await
    }

    #[inline]
    pub fn split(&mut self) -> (&mut Reader<Sub>, &mut Writer<Sub>) {
        (&mut self.read, &mut self.write)
    }

    #[inline]
    pub fn into_split(self) -> (Reader<Sub>, Writer<Sub>) {
        (self.read, self.write)
    }
}

#[cfg(feature = "tokio")]
mod tokio_impl {
    use super::{event, Stream};
    use core::{
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::{self, AsyncRead, AsyncWrite, ReadBuf};

    impl<Sub> AsyncRead for Stream<Sub>
    where
        Sub: event::Subscriber,
    {
        #[inline]
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Pin::new(&mut self.read).poll_read(cx, buf)
        }
    }

    impl<Sub> AsyncWrite for Stream<Sub>
    where
        Sub: event::Subscriber,
    {
        #[inline]
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.write).poll_write(cx, buf)
        }

        #[inline]
        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[std::io::IoSlice],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.write).poll_write_vectored(cx, buf)
        }

        #[inline]
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.write).poll_flush(cx)
        }

        #[inline]
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), io::Error>> {
            Pin::new(&mut self.write).poll_shutdown(cx)
        }

        #[inline(always)]
        fn is_write_vectored(&self) -> bool {
            self.write.is_write_vectored()
        }
    }
}
