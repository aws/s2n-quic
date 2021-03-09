// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use bytes::{BufMut, Bytes, BytesMut};
use futures::{ready, stream::Stream};
use std::{
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{fs, io::AsyncRead};

const CAPACITY: usize = 4096;

pub struct File {
    file: fs::File,
    buf: BytesMut,
}

impl File {
    pub async fn open<P: AsRef<Path>>(p: P) -> Result<Self> {
        let file = fs::File::open(p).await?;
        Ok(Self {
            file,
            buf: BytesMut::with_capacity(CAPACITY),
        })
    }
}

impl Stream for File {
    type Item = std::io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.buf.capacity() == 0 {
            self.buf.reserve(CAPACITY);
        }

        let n = {
            let bytes = self.buf.chunk_mut();
            let dst = unsafe { core::slice::from_raw_parts_mut(bytes.as_mut_ptr(), bytes.len()) };
            ready!(AsyncRead::poll_read(Pin::new(&mut self.file), cx, dst)?)
        };

        if n == 0 {
            return Poll::Ready(None);
        }

        // Safety: This is guaranteed to be the number of initialized (and read)
        // bytes due to the invariants provided by `ReadBuf::filled`.
        unsafe {
            self.buf.advance_mut(n);
        }

        let chunk = self.buf.split();
        Poll::Ready(Some(Ok(chunk.freeze())))
    }
}
