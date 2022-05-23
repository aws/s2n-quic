// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::Owner, Result};
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;
use std::{io::IoSlice, mem::MaybeUninit};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const READ_BUFFER_SIZE: usize = 100_000;
const SEND_BUFFER_SIZE: usize = 100_000_000;

#[derive(Debug)]
pub struct Connection<T: AsyncRead + AsyncWrite> {
    id: u64,
    inner: Pin<Box<T>>,
    stream_opened: bool,
    send_buffer: Vec<u8>,
    read_buffer: [MaybeUninit<u8>; READ_BUFFER_SIZE],
}

impl<T: AsyncRead + AsyncWrite> Connection<T> {
    pub fn new(id: u64, inner: Pin<Box<T>>) -> Self {
        Self {
            id,
            inner,
            stream_opened: false,
            send_buffer: vec![1; SEND_BUFFER_SIZE],
            read_buffer: unsafe { MaybeUninit::uninit().assume_init() },
        }
    }

    fn open_stream(&mut self) -> Result<()> {
        if self.stream_opened {
            return Err("cannot open more than one duplex stream at a time".into());
        }

        self.stream_opened = true;
        Ok(())
    }

    fn close_stream(&mut self) -> Result<()> {
        if !self.stream_opened {
            return Err("attempted to close the stream which wasn't opened".into());
        }

        self.stream_opened = false;
        Ok(())
    }
}

impl<T: AsyncRead + AsyncWrite> super::Connection for Connection<T> {
    fn id(&self) -> u64 {
        self.id
    }

    fn poll_open_bidirectional_stream(&mut self, _: u64, _: &mut Context) -> Poll<Result<()>> {
        self.open_stream().into()
    }

    fn poll_open_send_stream(&mut self, _: u64, _: &mut Context) -> Poll<Result<()>> {
        self.open_stream().into()
    }

    fn poll_accept_stream(&mut self, _: &mut Context) -> Poll<Result<Option<u64>>> {
        let id: u64 = 0;
        match self.open_stream() {
            Ok(()) => Ok(Some(id)).into(),
            Err(err) => Err(err).into(),
        }
    }

    fn poll_send(&mut self, _owner: Owner, _id: u64, bytes: u64, cx: &mut Context) -> Poll<Result<u64>> {
        let mut sent: u64 = 0;
        while sent < bytes {
            let to_send = (bytes - sent) as usize;
            let to_send = to_send.min(SEND_BUFFER_SIZE);
            let to_send = &self.send_buffer[0..to_send];
            match self.inner.as_mut().poll_write(cx, to_send) {
                Poll::Ready(result) => {
                    sent += result? as u64;
                }
                Poll::Pending => {
                    break;
                }
            }
        }

        if sent == 0 {
            return Poll::Pending;
        }

        cx.waker().wake_by_ref();
        if let Poll::Ready(res) = self.inner.as_mut().poll_flush(cx) {
            res?;
        }

        Ok(sent).into()
    }

    fn poll_receive(&mut self, _owner: Owner, _id: u64, bytes: u64, cx: &mut Context) -> Poll<Result<u64>> {
        let mut received: u64 = 0;
        while received < bytes {
            let mut buf = ReadBuf::uninit(&mut self.read_buffer);
            match self.inner.as_mut().poll_read(cx, &mut buf) {
                Poll::Ready(_) => {
                    if buf.filled().is_empty() && self.stream_opened {
                        self.close_stream()?;
                        return Ok(0).into();
                    }
                    received += buf.filled().len() as u64;
                }
                Poll::Pending => {
                    break;
                }
            }
        }

        if received == 0 {
            return Poll::Pending;
        }

        cx.waker().wake_by_ref();
        Ok(received).into()
    }

    fn poll_send_finish(&mut self, _: Owner, _: u64, _: &mut Context) -> Poll<Result<()>> {
        Ok(()).into()
    }

    fn poll_receive_finish(&mut self, _: Owner, _: u64, _: &mut Context) -> Poll<Result<()>> {
        Ok(()).into()
    }

    fn poll_progress(&mut self, _: &mut Context) -> Poll<Result<()>> {
        Ok(()).into()
    }

    fn poll_finish(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        if self.stream_opened {
            ready!(self.inner.as_mut().poll_shutdown(cx))?;
            self.close_stream()?;
        }
        Ok(()).into()
    }
}
