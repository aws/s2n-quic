// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::Owner, Result};
use bytes::Buf;
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;
use std::collections::{hash_map::Entry, HashMap, HashSet, VecDeque};
use tokio::io::{AsyncRead, AsyncWrite};
use s2n_quic_core::stream::testing::Data;

#[derive(Debug)]
pub struct Connection<T: AsyncRead + AsyncWrite> {
    id: u64,
    inner: Pin<Box<T>>,
    stream_opened: bool,
    to_send: u64,
    to_receive: u64,
}

impl<T: AsyncRead + AsyncWrite> Connection<T> {
    pub fn new(id: u64, inner: Pin<Box<T>>) -> Self {
        Self {
            id,
            inner,
            stream_opened: false,
            to_send: 0,
            to_receive: 0,
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
        if self.inner.as_ref().is_write_vectored() {
            eprintln!("is write vectored");
        } else {
            eprintln!("is not write vectored");
        }
        Ok(())
    }

    fn read(&mut self, cx: &mut Context) -> Result<()> {
        Ok(())
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
        self.to_send += bytes;
        Ok(bytes).into()
    }

    fn poll_receive(&mut self, owner: Owner, id: u64, bytes: u64, cx: &mut Context) -> Poll<Result<u64>> {
        self.to_receive += bytes;
        Ok(bytes).into()
    }

    fn poll_send_finish(&mut self, owner: Owner, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        todo!()
    }

    fn poll_receive_finish(&mut self, owner: Owner, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        todo!()
    }

    fn poll_progress(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        loop {
            self.write(cx);
        }
    }

    fn poll_finish(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        todo!()
    }
}
