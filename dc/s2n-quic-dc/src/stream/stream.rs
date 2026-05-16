// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{endpoint::error::Error, Reader, Writer};
use s2n_quic_core::buffer;
use std::{io, net::SocketAddr};

/// A bidirectional stream composed of a [`Reader`] and [`Writer`].
///
/// This is the convenience API for applications that want one handle for both
/// halves. It preserves the semantics of the underlying `Reader` and `Writer`
/// and adds no extra buffering or synchronization of its own.
///
/// # Expectations and guarantees
///
/// - Reads and writes are independent, so request/response style protocols can
///   use both halves concurrently after splitting.
/// - [`shutdown`](Self::shutdown) only closes the write half.
/// - [`reset`](Self::reset) terminates both halves immediately.
/// - When the `tokio` feature is enabled, `Stream` implements both
///   [`tokio::io::AsyncRead`] and [`tokio::io::AsyncWrite`].
///
/// # Footguns
///
/// - [`split`](Self::split) only gives borrowed halves, so the returned values
///   cannot be moved into independently-owned tasks.
/// - [`into_split`](Self::into_split) moves the stream apart; use that form when
///   each half needs its own owner.
///
/// # Example
///
/// ```ignore
/// async fn handle(
///     mut stream: s2n_quic_dc::stream::Stream,
/// ) -> std::io::Result<()> {
///     stream.validate().await?;
///
///     let (mut reader, mut writer) = stream.into_split();
///     let recv = async move {
///         let mut body = Vec::new();
///         while !reader.read_to_end(&mut body).await?.is_complete() {}
///         std::io::Result::Ok(body)
///     };
///     let send = async move {
///         let mut bytes: &[u8] = b"ok";
///         writer.write_all_from_fin(&mut bytes).await
///     };
///
///     let (_body, _written) = tokio::try_join!(recv, send)?;
///     Ok(())
/// }
/// ```
pub struct Stream {
    read: Reader,
    write: Writer,
}

impl Stream {
    pub(crate) fn new(read: Reader, write: Writer) -> Self {
        Self { read, write }
    }

    /// Resets both halves of the stream and tries to notify the peer with a
    /// `FlowReset`.
    ///
    /// This transitions both the Reader and Writer to their terminal states so
    /// their Drop impls are no-ops. Reset notification is attempted from the
    /// Reader side; if the read half is already terminal or the flow is not yet
    /// fully established, this may become a local-only reset with no
    /// `FlowReset` emitted.
    ///
    /// Use this when the entire stream should fail immediately and any queued or
    /// future I/O should stop.
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

    /// Waits for the read half to become valid for application use.
    ///
    /// For streams that were already validated (confirmed non-duplicate), this is a no-op.
    /// For pending streams, this polls for the FlowValidated message from the pipeline.
    /// The application should wrap this in its own timeout.
    #[inline]
    pub async fn validate(&mut self) -> io::Result<()> {
        self.read.validate().await
    }

    /// Returns borrowed access to the read and write halves.
    ///
    /// This is convenient for running both halves in the same task.
    /// Use [`into_split`](Self::into_split) if each half needs separate ownership.
    #[inline]
    pub fn split(&mut self) -> (&mut Reader, &mut Writer) {
        (&mut self.read, &mut self.write)
    }

    /// Consumes the stream and returns owned reader and writer halves.
    ///
    /// This is the right choice when the halves need to be moved into separate
    /// tasks or stored independently.
    #[inline]
    pub fn into_split(self) -> (Reader, Writer) {
        (self.read, self.write)
    }

    /// Returns the handshake peer address used to identify this stream.
    ///
    /// This remains the stable peer identity even if data is exchanged across
    /// multiple data paths.
    #[inline]
    pub fn peer_addr(&self) -> SocketAddr {
        self.read.peer_addr()
    }

    /// Reads from the stream's receive half.
    ///
    /// See [`Reader::read_into`] for detailed semantics.
    #[inline]
    pub async fn read_into<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::writer::Storage,
    {
        self.read.read_into(buf).await
    }

    /// Writes to the stream's send half.
    ///
    /// See [`Writer::write_from`] for detailed semantics.
    #[inline]
    pub async fn write_from<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        self.write.write_from(buf).await
    }

    /// Writes until the provided buffer is empty.
    ///
    /// See [`Writer::write_all_from`] for detailed semantics.
    #[inline]
    pub async fn write_all_from<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        self.write.write_all_from(buf).await
    }

    /// Writes and marks the send half finished once the provided buffer is empty.
    ///
    /// See [`Writer::write_from_fin`] for detailed semantics.
    #[inline]
    pub async fn write_from_fin<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        self.write.write_from_fin(buf).await
    }

    /// Writes the full buffer and queues FIN on the final chunk.
    ///
    /// See [`Writer::write_all_from_fin`] for detailed semantics.
    #[inline]
    pub async fn write_all_from_fin<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        self.write.write_all_from_fin(buf).await
    }

    /// Half-closes the write side of the stream.
    ///
    /// See [`Writer::shutdown`] for the exact delivery semantics.
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
