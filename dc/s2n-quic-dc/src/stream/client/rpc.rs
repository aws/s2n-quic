// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event, stream::application::Stream};
use s2n_quic_core::buffer::{self, writer::Storage};
use std::{future::Future, io};

pub async fn from_stream<Sub, Req, Res>(
    stream: Stream<Sub>,
    mut request: Req,
    mut response: Res,
) -> io::Result<Res::Output>
where
    Sub: event::Subscriber,
    Req: Request,
    Res: Response,
{
    let (mut reader, mut writer) = stream.into_split();

    // TODO if the request is large enough, should we spawn a task for it?
    let writer = async move {
        while !request.buffer_is_empty() {
            writer.write_from_fin(&mut request).await?;
        }

        writer.shutdown()?;

        Ok(())
    };

    let reader = async move {
        loop {
            let storage = response.provide_storage().await?;

            if !storage.has_remaining_capacity() {
                return Err(io::Error::new(io::ErrorKind::Other, "the provided response buffer failed to provide enough capacity for the peer's response"));
            }

            let len = reader.read_into(storage).await?;

            if len == 0 {
                let out = response.finish().await?;
                return Ok(out);
            }
        }
    };

    let (_, out) = ::tokio::try_join!(writer, reader)?;

    Ok(out)
}

pub trait Request: 'static + Send + buffer::reader::storage::Infallible {}

impl<T: 'static + Send + buffer::reader::storage::Infallible> Request for T {}

pub trait Response {
    type Storage: buffer::writer::Storage;
    type Output;

    /// Provides storage space for the response from the peer
    ///
    /// The storage should have a capacity of at least 1. Otherwise the operation will be aborted.
    fn provide_storage(&mut self) -> impl Future<Output = io::Result<&mut Self::Storage>>;

    /// Indicates the peer has transmitted the entire response
    fn finish(self) -> impl Future<Output = io::Result<Self::Output>>;
}

/// Writes the response into the provided [`buffer::writer::Storage`] implementation.
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
