// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Single-round-trip RPC helpers built on top of a [`Stream`].
//!
//! A "request/response" exchange is the most common pattern for `s2n-quic-dc` streams: the
//! client sends a request with FIN and then reads the server's response until EOF.  This
//! module captures that pattern in a pair of traits ([`Request`] and [`Response`]) and a
//! free function ([`from_stream`]) that drives both halves concurrently.

use crate::stream::Stream;
use core::future::Future;
use s2n_quic_core::buffer::{self, writer::Storage as _};
use std::{future::poll_fn, io};

/// A type that can be written as the request body of an RPC.
///
/// Any type that already implements [`buffer::reader::storage::Infallible`] — including
/// `&[u8]`, [`bytes::Bytes`], and [`Vec<u8>`] — automatically satisfies this trait.
pub trait Request: 'static + Send + buffer::reader::storage::Infallible {}

impl<T: 'static + Send + buffer::reader::storage::Infallible> Request for T {}

/// A type that accumulates the response body of an RPC and produces a final output.
///
/// Implementations are called in a loop: each iteration calls [`provide_storage`] to
/// obtain a buffer for the next chunk of response data and, once EOF is reached, calls
/// [`finish`] to produce the final value.
///
/// The built-in implementation is [`InMemoryResponse`], which collects all bytes into an
/// in-memory buffer before returning.
pub trait Response {
    /// The write destination for incoming response bytes.
    type Storage: buffer::writer::Storage;

    /// The value produced after the response is fully received.
    type Output;

    /// Returns a mutable reference to the buffer that should receive the next chunk.
    ///
    /// This is called before every [`read_into`](crate::stream::Reader::read_into). The
    /// returned storage must have remaining capacity; returning a full buffer causes the
    /// RPC to fail with an error.
    fn provide_storage(&mut self) -> impl Future<Output = io::Result<&mut Self::Storage>>;

    /// Consumes the response and produces the final output after EOF is reached.
    fn finish(self) -> impl Future<Output = io::Result<Self::Output>>;
}

/// A [`Response`] that accumulates all bytes into a single in-memory buffer `S`.
///
/// `S` is any type that implements [`buffer::writer::Storage`], such as [`Vec<u8>`] or
/// [`bytes::BytesMut`].  The buffer is returned verbatim from [`finish`](Response::finish).
///
/// # Example
///
/// ```ignore
/// use s2n_quic_dc::stream::client::rpc::InMemoryResponse;
///
/// let response: InMemoryResponse<Vec<u8>> = Vec::new().into();
/// ```
pub struct InMemoryResponse<S>(S);

impl<S> From<S> for InMemoryResponse<S>
where
    S: buffer::writer::Storage,
{
    fn from(value: S) -> Self {
        InMemoryResponse(value)
    }
}

impl<S> Response for InMemoryResponse<S>
where
    S: buffer::writer::Storage,
{
    type Storage = S;
    type Output = S;

    async fn provide_storage(&mut self) -> io::Result<&mut Self::Storage> {
        Ok(&mut self.0)
    }

    async fn finish(self) -> io::Result<Self::Output> {
        Ok(self.0)
    }
}

/// Drives a single request/response RPC over an existing [`Stream`].
///
/// Writes the full `request` payload with FIN on the write half, and concurrently reads the
/// full response on the read half until EOF.  Once both halves are done, returns
/// `response.finish().await`.
///
/// The write and read halves run concurrently in the same task: writes make progress
/// whenever the write half has flow credit, while reads drain incoming data in parallel.
///
/// # Errors
///
/// - Any write error is propagated immediately and the stream is dropped.
/// - Any read error is propagated immediately.
/// - If [`Response::provide_storage`] returns a full buffer that cannot accept more bytes,
///   an error is returned.
pub async fn from_stream<Req, Res>(
    stream: Stream,
    request: Req,
    response: Res,
) -> io::Result<Res::Output>
where
    Req: Request,
    Res: Response,
{
    let (mut reader, mut writer) = stream.into_split();

    let writer = async move {
        let mut request = request;
        while !request.buffer_is_empty() {
            writer.write_from_fin(&mut request).await?;
        }
        writer.shutdown()?;
        <io::Result<_>>::Ok(())
    };
    let mut writer = core::pin::pin!(writer);
    let mut writer_finished = false;

    let reader = async move {
        let mut response = response;
        loop {
            let storage = response.provide_storage().await?;

            if !storage.has_remaining_capacity() {
                return Err(io::Error::other(
                    "the provided response buffer failed to provide enough capacity for the peer's response",
                ));
            }

            let len = reader.read_into(storage).await?;

            if len == 0 {
                let out = response.finish().await?;
                return Ok(out);
            }
        }
    };
    let mut reader = core::pin::pin!(reader);

    poll_fn(|cx| {
        if !writer_finished {
            writer_finished = writer.as_mut().poll(cx)?.is_ready();
        }

        reader.as_mut().poll(cx)
    })
    .await
}
