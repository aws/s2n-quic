// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{endpoint::error::Error, Reader, Writer};
use s2n_quic_core::buffer;
use std::{io, net::SocketAddr};

/// A bidirectional stream composed of a Reader and Writer
pub struct Stream {
    read: Reader,
    write: Writer,
}

impl Stream {
    pub(crate) fn new(read: Reader, write: Writer) -> Self {
        Self { read, write }
    }

    /// Reset both halves of the stream, sending a single FlowReset to the peer.
    ///
    /// This transitions both the Reader and Writer to their terminal states so their
    /// Drop impls are no-ops. Only one FlowReset packet is sent (from the Reader side).
    pub fn reset(&mut self, error: Error) {
        self.read.send_reset(error.as_varint());
        self.write.force_shutdown();
    }

    /// Returns the stream identifier.
    ///
    /// This is the same ID that the client assigned when opening the stream,
    /// and is echoed by the server side once the stream is accepted.
    pub fn stream_id(&self) -> u64 {
        self.read.stream_id()
    }

    /// Wait for the stream to be validated
    ///
    /// For streams that were already validated (confirmed non-duplicate), this is a no-op.
    /// For pending streams, this polls for the FlowValidated message from the pipeline.
    /// The application should wrap this in its own timeout.
    #[inline]
    pub async fn validate(&mut self) -> io::Result<()> {
        self.read.validate().await
    }

    #[inline]
    pub fn split(&mut self) -> (&mut Reader, &mut Writer) {
        (&mut self.read, &mut self.write)
    }

    #[inline]
    pub fn into_split(self) -> (Reader, Writer) {
        (self.read, self.write)
    }

    #[inline]
    pub fn peer_addr(&self) -> SocketAddr {
        self.read.peer_addr()
    }

    #[inline]
    pub async fn read_into<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::writer::Storage,
    {
        self.read.read_into(buf).await
    }

    #[inline]
    pub async fn write_from<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        self.write.write_from(buf).await
    }

    #[inline]
    pub async fn write_all_from<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        self.write.write_all_from(buf).await
    }

    #[inline]
    pub async fn write_from_fin<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        self.write.write_from_fin(buf).await
    }

    #[inline]
    pub async fn write_all_from_fin<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        self.write.write_all_from_fin(buf).await
    }

    #[inline]
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.write.shutdown()
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
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        #[inline]
        fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.write.shutdown().into()
        }

        #[inline]
        fn is_write_vectored(&self) -> bool {
            true
        }
    }
}
