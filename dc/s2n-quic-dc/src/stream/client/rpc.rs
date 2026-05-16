// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::Stream;
use core::future::Future;
use s2n_quic_core::buffer::{self, writer::Storage as _};
use std::{future::poll_fn, io};

pub trait Request: 'static + Send + buffer::reader::storage::Infallible {}

impl<T: 'static + Send + buffer::reader::storage::Infallible> Request for T {}

pub trait Response {
    type Storage: buffer::writer::Storage;
    type Output;

    fn provide_storage(&mut self) -> impl Future<Output = io::Result<&mut Self::Storage>>;

    fn finish(self) -> impl Future<Output = io::Result<Self::Output>>;
}

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
