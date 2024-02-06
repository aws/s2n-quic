// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use bytes::{BufMut, Bytes, BytesMut};
use core::{mem::MaybeUninit, task::ready};
use futures::stream::Stream;
use std::{
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    fs,
    io::{AsyncRead, ReadBuf},
};

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
            let dst = unsafe { &mut *(bytes as *mut _ as *mut [MaybeUninit<u8>]) };
            let mut buf = ReadBuf::uninit(dst);
            ready!(AsyncRead::poll_read(
                Pin::new(&mut self.file),
                cx,
                &mut buf
            )?);
            buf.filled().len()
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

pub(crate) fn abs_path(path: &str, www_dir: &Path) -> PathBuf {
    let mut abs_path = www_dir.to_path_buf();
    abs_path.extend(
        path.split('/')
            .filter(|segment| !segment.starts_with('.'))
            .map(std::path::Path::new),
    );
    abs_path
}
