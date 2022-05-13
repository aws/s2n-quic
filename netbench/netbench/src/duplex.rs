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
    send_data: Vec<u8>,
    to_send: u64,
    read_buffer: [MaybeUninit<u8>; READ_BUFFER_SIZE],
    buffered_offset: u64,
    received_offset: u64,
}

impl<T: AsyncRead + AsyncWrite> Connection<T> {
    pub fn new(id: u64, inner: Pin<Box<T>>) -> Self {
        Self {
            id,
            inner,
            stream_opened: false,
            send_data: vec![1; SEND_BUFFER_SIZE],
            to_send: 0,
            read_buffer: unsafe { MaybeUninit::uninit().assume_init() },
            buffered_offset: 0,
            received_offset: 0,
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

    fn write(&mut self, cx: &mut Context) -> Result<()> {
        if self.to_send == 0 {
            return Ok(());
        }

        let send_size = SEND_BUFFER_SIZE.min(self.to_send as usize);
        let mut len = 0;

        if self.inner.as_ref().is_write_vectored() {
            let to_send = IoSlice::new(&self.send_data[0..send_size]);
            if let Poll::Ready(result) = self.inner.as_mut().poll_write_vectored(cx, &[to_send]) {
                len += result? as u64;
                self.to_send -= len;
            }
        } else {
            let to_send = &self.send_data[0..send_size];
            if let Poll::Ready(result) = self.inner.as_mut().poll_write(cx, to_send) {
                len += result? as u64;
                self.to_send -= len;
            }
        }

        if len > 0 {
            cx.waker().wake_by_ref();
        }

        if let Poll::Ready(res) = self.inner.as_mut().poll_flush(cx) {
            res?;
        }

        Ok(())
    }

    fn read(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        let mut buf = ReadBuf::uninit(&mut self.read_buffer);
        let mut len = 0;
        match self.inner.as_mut().poll_read(cx, &mut buf) {
            Poll::Ready(_) => {
                if buf.filled().is_empty() {
                    if self.stream_opened {
                        self.close_stream()?;
                    }
                    return Ok(()).into();
                }

                len += buf.filled().len() as u64;
                self.buffered_offset += len;
            }
            Poll::Pending => {
                return Poll::Pending;
            }
        }

        if len > 0 {
            cx.waker().wake_by_ref();
        }

        Ok(()).into()
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

    fn poll_send(&mut self, _: Owner, _: u64, bytes: u64, _: &mut Context) -> Poll<Result<u64>> {
        let to_add = bytes.min(SEND_BUFFER_SIZE as u64 - self.to_send);
        if to_add == 0 {
            return Poll::Pending;
        }

        self.to_send += to_add;
        Ok(to_add).into()
    }

    fn poll_receive(&mut self, _: Owner, _: u64, bytes: u64, _: &mut Context) -> Poll<Result<u64>> {
        let len = (self.buffered_offset - self.received_offset).min(bytes);

        if len == 0 {
            return Poll::Pending;
        }

        self.received_offset += len;
        Ok(len).into()
    }

    fn poll_send_finish(&mut self, _: Owner, _: u64, _: &mut Context) -> Poll<Result<()>> {
        Ok(()).into()
    }

    fn poll_receive_finish(&mut self, _: Owner, _: u64, _: &mut Context) -> Poll<Result<()>> {
        Ok(()).into()
    }

    fn poll_progress(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        loop {
            self.write(cx)?;
            ready!(self.read(cx))?;

            if !self.stream_opened {
                return Ok(()).into();
            }
        }
    }

    fn poll_finish(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        if self.stream_opened {
            self.write(cx)?;
            if self.to_send > 0 {
                return Poll::Pending;
            }

            ready!(self.inner.as_mut().poll_shutdown(cx))?;
            self.close_stream()?;
        }

        loop {
            ready!(self.read(cx))?;
            if !self.stream_opened {
                return Ok(()).into();
            }
        }
    }
}
