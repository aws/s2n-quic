// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{endpoint::error::Error, Reader, Writer};
use s2n_quic_core::buffer;
use std::{io, net::SocketAddr};

/// A stream accepted by the server that may still require validation.
///
/// This wrapper enforces server-side validation in the type system: until the
/// value is converted into a [`Stream`], application code can only use the
/// validation-specific methods on this type.
///
/// # Footguns
///
/// Validation has no built-in timeout. Applications should wrap
/// [`validate`](Self::validate) with their own timeout policy.
pub struct PendingValidation {
    stream: Stream,
}

impl PendingValidation {
    pub(crate) fn new(stream: Stream) -> Self {
        Self { stream }
    }

    /// Waits for the stream to become valid for application use and returns the
    /// unwrapped [`Stream`].
    ///
    /// This call has no built-in timeout. If validation is part of your request
    /// deadline, wrap it in your own timeout.
    pub async fn validate(mut self) -> io::Result<Stream> {
        self.stream.read.validate().await?;
        Ok(self.stream)
    }

    /// Attempts to unwrap immediately if validation already completed.
    ///
    /// Returns `Ok(Stream)` when validated, or `Err(Self)` if validation is
    /// still pending.
    pub fn try_validate(self) -> Result<Stream, Self> {
        if self.is_validated() {
            Ok(self.stream)
        } else {
            Err(self)
        }
    }

    /// Returns whether validation has already completed.
    #[inline]
    pub fn is_validated(&self) -> bool {
        self.stream.read.is_validated()
    }

    /// Returns the stream identifier.
    #[inline]
    pub fn stream_id(&self) -> u64 {
        self.stream.stream_id()
    }

    /// Returns the handshake peer address used to identify this stream.
    #[inline]
    pub fn peer_addr(&self) -> SocketAddr {
        self.stream.peer_addr()
    }

    /// Resets both halves of the stream.
    #[inline]
    pub fn reset(&mut self, error: Error) {
        self.stream.reset(error);
    }

    /// Disables both halves without emitting reset frames.
    ///
    /// Intended for rejection paths where the caller sends the reset frame via
    /// endpoint dispatch and only needs to suppress per-half Drop side effects.
    /// This is safe to call before validation completes because it only updates
    /// local stream-halves state and intentionally bypasses user-facing
    /// validation flow.
    #[inline]
    pub(crate) fn disable(&mut self) {
        self.stream.disable();
    }
}

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
    /// `QueueReset`.
    ///
    /// This transitions both the Reader and Writer to their terminal states so
    /// their Drop impls are no-ops. Reset notification is attempted from the
    /// Reader side; if the read half is already terminal or the flow is not yet
    /// fully established, this may become a local-only reset with no
    /// `QueueReset` emitted.
    ///
    /// Use this when the entire stream should fail immediately and any queued or
    /// future I/O should stop.
    pub fn reset(&mut self, error: Error) {
        self.read.send_reset(error.as_varint());
        self.write.force_shutdown();
    }

    /// Disables both halves without emitting reset frames.
    ///
    /// Intended for early rejection paths before normal stream ownership
    /// handoff, where reset signaling is emitted by the caller.
    pub(crate) fn disable(&mut self) {
        self.read.force_reset();
        self.write.force_shutdown();
    }

    /// Returns the stream identifier.
    ///
    /// This is the same ID that the client assigned when opening the stream,
    /// and is echoed by the server side once the stream is accepted.
    pub fn stream_id(&self) -> u64 {
        self.read.stream_id()
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
