// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{
    recv::application::{self as recv, Reader},
    send::application::{self as send, Writer},
    shared::ArcShared,
    socket,
};
use core::fmt;
use s2n_quic_core::buffer;
use std::{io, net::SocketAddr};

pub struct Builder {
    pub read: recv::Builder,
    pub write: send::Builder,
    pub shared: ArcShared,
    pub sockets: Box<dyn socket::application::Builder>,
}

impl Builder {
    #[inline]
    pub fn build(self) -> io::Result<Stream> {
        let Self {
            read,
            write,
            shared,
            sockets,
        } = self;
        let sockets = sockets.build()?;
        let read = read.build(shared.clone(), sockets.clone());
        let write = write.build(shared, sockets);
        Ok(Stream { read, write })
    }
}

pub struct Stream {
    read: Reader,
    write: Writer,
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Stream")
            .field("peer_addr", &self.peer_addr().unwrap())
            .field("local_addr", &self.local_addr().unwrap())
            .finish()
    }
}

impl Stream {
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
    pub async fn write_from(
        &mut self,
        buf: &mut impl buffer::reader::storage::Infallible,
    ) -> io::Result<usize> {
        self.write.write_from(buf).await
    }

    #[inline]
    pub async fn read_into(
        &mut self,
        out_buf: &mut impl buffer::writer::Storage,
    ) -> io::Result<usize> {
        self.read.read_into(out_buf).await
    }

    #[inline]
    pub fn split(&mut self) -> (&mut Reader, &mut Writer) {
        (&mut self.read, &mut self.write)
    }

    #[inline]
    pub fn into_split(self) -> (Reader, Writer) {
        (self.read, self.write)
    }
}

#[cfg(feature = "tokio")]
mod tokio_impl {
    use super::Stream;
    use core::{
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::{self, AsyncRead, AsyncWrite, ReadBuf};

    impl AsyncRead for Stream {
        #[inline]
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Pin::new(&mut self.read).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for Stream {
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
