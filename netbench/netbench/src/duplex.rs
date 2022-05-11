// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::Owner, Result};
use bytes::{Buf, Bytes};
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;
use std::collections::{hash_map::Entry, HashMap, HashSet, VecDeque};
use std::io::IoSlice;
use std::mem::MaybeUninit;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use s2n_quic_core::stream::testing::Data;

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

        let mut len = 0;

        if self.inner.as_ref().is_write_vectored() {
            let send_size = SEND_BUFFER_SIZE.min(self.to_send as usize);
            let to_send = IoSlice::new(&self.send_data[0..send_size]);

            match self.inner.as_mut().poll_write_vectored(cx, &[to_send]) {
                Poll::Ready(result) => {
                    len += result? as u64;
                    self.to_send -= len;
                    eprintln!("sent: {}", len);
                }
                _ => {}
            }
        } else {
            panic!("not write vectored");
        }

        if len > 0 {
            cx.waker().wake_by_ref();
        }

        Ok(())
    }

    fn read(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        let mut buf = ReadBuf::uninit(&mut self.read_buffer);
        let mut len = 0;
        match self.inner.as_mut().poll_read(cx, &mut buf) {
            Poll::Ready(result) => {
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

    fn poll_open_bidirectional_stream(&mut self, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        self.open_stream().into()
    }

    fn poll_open_send_stream(&mut self, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        self.open_stream().into()
    }

    fn poll_accept_stream(&mut self, cx: &mut Context) -> Poll<Result<Option<u64>>> {
        let id: u64 = 0;
        match self.open_stream().into() {
            Ok(()) => Ok(Some(id)).into(),
            Err(err) => Err(err).into(),
        }
    }

    fn poll_send(&mut self, owner: Owner, id: u64, bytes: u64, cx: &mut Context) -> Poll<Result<u64>> {
        eprintln!("poll send: {}", bytes);
        self.to_send += bytes;
        Ok(bytes).into()
    }

    fn poll_receive(&mut self, owner: Owner, id: u64, bytes: u64, cx: &mut Context) -> Poll<Result<u64>> {
        eprintln!("poll receive: {}", bytes);
        let len = (self.buffered_offset - self.received_offset).min(bytes);

        if len == 0 {
            return Poll::Pending;
        }

        self.received_offset += len;
        Ok(len).into()
    }

    fn poll_send_finish(&mut self, owner: Owner, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        Ok(()).into()
    }

    fn poll_receive_finish(&mut self, owner: Owner, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        Ok(()).into()
    }

    fn poll_progress(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        loop {
            self.write(cx);
            ready!(self.read(cx));
        }
    }

    fn poll_finish(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        if self.stream_opened {
            self.write(cx);
            if self.to_send > 0 {
                return Poll::Pending;
            }

            ready!(self.inner.as_mut().poll_shutdown(cx));
            self.close_stream();
        }

        loop {
            ready!(self.read(cx));
            return Ok(()).into();
        }
    }
}
