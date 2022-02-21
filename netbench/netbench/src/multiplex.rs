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

mod buffer;
mod frame;
mod stream;

use buffer::{ReadBuffer, WriteBuffer};
use frame::Frame;
use stream::{ReceiveStream, SendStream, Stream};

const DEFUALT_STREAM_WINDOW: u64 = 256000;
const DEFAULT_MAX_STREAMS: u64 = 100;

#[derive(Clone, Debug)]
pub struct Config {
    pub stream_window: u64,
    pub max_streams: u64,
    pub max_stream_data_thresh: u64,
    pub max_stream_frame_len: u32,
    pub max_write_queue_len: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stream_window: DEFUALT_STREAM_WINDOW,
            max_streams: DEFAULT_MAX_STREAMS,
            max_stream_data_thresh: DEFUALT_STREAM_WINDOW / 2,
            max_stream_frame_len: (1 << 15),
            max_write_queue_len: 250,
        }
    }
}

pub struct Connection<T: AsyncRead + AsyncWrite> {
    inner: Pin<Box<T>>,
    rx_open: bool,
    tx_open: bool,
    frame: Option<frame::Frame>,
    read_buf: ReadBuffer,
    decoder: frame::Decoder,
    write_buf: WriteBuffer,
    stream_controllers: [stream::Controller; 2],
    streams: [HashMap<u64, Stream>; 2],
    max_stream_data: HashSet<(Owner, u64)>,
    pending_accept: VecDeque<u64>,
    peer_initial_max_stream_data: u64,
    config: Config,
}

impl<T: AsyncRead + AsyncWrite> Connection<T> {
    pub fn new(inner: Pin<Box<T>>, config: Config) -> Self {
        let mut write_buf = WriteBuffer::default();

        if config.stream_window != DEFUALT_STREAM_WINDOW {
            write_buf.push(Frame::InitialMaxStreamData {
                up_to: config.stream_window,
            });
        }

        Self {
            inner,
            rx_open: true,
            tx_open: true,
            frame: None,
            read_buf: Default::default(),
            decoder: Default::default(),
            write_buf,
            stream_controllers: [
                stream::Controller::new(config.max_streams),
                stream::Controller::new(100),
            ],
            streams: [Default::default(), Default::default()],
            max_stream_data: Default::default(),
            pending_accept: Default::default(),
            peer_initial_max_stream_data: DEFUALT_STREAM_WINDOW,
            config,
        }
    }

    fn flush_write_buffer(&mut self, cx: &mut Context) -> Result<()> {
        if !self.tx_open {
            return if self.write_buf.is_empty() {
                Ok(())
            } else {
                Err("stream was closed with pending data".into())
            };
        }

        if self.write_buf.is_empty() {
            return Ok(());
        }

        if self.inner.as_ref().is_write_vectored() {
            let chunks = self.write_buf.chunks();

            return match self.inner.as_mut().poll_write_vectored(cx, &chunks) {
                Poll::Ready(result) => {
                    let len = result?;
                    self.write_buf.advance(len);
                    self.write_buf.notify(cx);
                    Ok(())
                }
                Poll::Pending => Ok(()),
            };
        }

        while let Some(mut chunk) = self.write_buf.pop_front() {
            match self.inner.as_mut().poll_write(cx, &chunk) {
                Poll::Ready(result) => {
                    let len = result?;
                    chunk.advance(len);

                    if !chunk.is_empty() {
                        self.write_buf.push_front(chunk);
                    }

                    if len == 0 {
                        return if self.write_buf.is_empty() {
                            self.tx_open = false;
                            Ok(())
                        } else {
                            Err("stream was closed with pending data".into())
                        };
                    }
                }
                Poll::Pending => {
                    self.write_buf.push_front(chunk);
                    break;
                }
            }
        }

        self.write_buf.notify(cx);

        Ok(())
    }

    fn flush_read_buffer(&mut self, cx: &mut Context) -> Result<()> {
        loop {
            if self.frame.is_none() {
                if let Some(frame) = self.decoder.decode(&mut self.read_buf)? {
                    self.frame = Some(frame);
                }
            }

            match self.dispatch_frame(cx) {
                Poll::Ready(result) => result?,
                Poll::Pending => return Ok(()),
            }
        }
    }

    fn dispatch_frame(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        use frame::Frame::*;

        match self.frame.take() {
            Some(StreamOpen { id, bidirectional }) => {
                // TODO make sure the peer hasn't opened too many
                let mut stream = Stream {
                    rx: Some(ReceiveStream::new(self.config.stream_window)),
                    tx: None,
                };
                if bidirectional {
                    stream.tx = Some(SendStream::new(self.peer_initial_max_stream_data));
                };
                self.streams[Owner::Remote].insert(id, stream);
                self.pending_accept.push_back(id);
                cx.waker().wake_by_ref();
            }
            Some(StreamData {
                id,
                owner,
                mut data,
            }) => {
                if let Some(rx) = self.streams[owner]
                    .get_mut(&id)
                    .and_then(|stream| stream.rx.as_mut())
                {
                    let len = data.len() as u64;
                    let len = rx.buffer(len, cx)?;
                    data.advance(len as _);

                    if !data.is_empty() {
                        self.frame = Some(StreamData { id, owner, data });
                        return Poll::Pending;
                    }
                }
            }
            Some(MaxStreams { up_to }) => {
                self.stream_controllers[Owner::Remote].max_streams(up_to, cx);
            }
            Some(MaxStreamData { id, owner, up_to }) => {
                if let Some(tx) = self.streams[owner]
                    .get_mut(&id)
                    .and_then(|stream| stream.tx.as_mut())
                {
                    tx.max_data(up_to, cx);
                }
            }
            Some(StreamFinish { id, owner }) => {
                if let Some(rx) = self.streams[owner]
                    .get_mut(&id)
                    .ok_or("invalid stream")?
                    .rx
                    .as_mut()
                {
                    rx.finish(cx);
                }
            }
            Some(InitialMaxStreamData { up_to }) => {
                self.peer_initial_max_stream_data = up_to;
            }
            None => return Poll::Pending,
        }

        Ok(()).into()
    }
}

impl<T: AsyncRead + AsyncWrite> super::Connection for Connection<T> {
    fn poll_open_bidirectional_stream(&mut self, id: u64, _cx: &mut Context) -> Poll<Result<()>> {
        ready!(self.stream_controllers[Owner::Local].open());

        self.write_buf.push(Frame::StreamOpen {
            id,
            bidirectional: true,
        });

        let stream = Stream {
            rx: Some(ReceiveStream::new(self.config.stream_window)),
            tx: Some(SendStream::new(self.peer_initial_max_stream_data)),
        };
        self.streams[Owner::Local].insert(id, stream);

        Ok(()).into()
    }

    fn poll_open_send_stream(&mut self, id: u64, _cx: &mut Context) -> Poll<Result<()>> {
        ready!(self.stream_controllers[Owner::Local].open());

        self.write_buf.push(Frame::StreamOpen {
            id,
            bidirectional: false,
        });

        let stream = Stream {
            rx: None,
            tx: Some(SendStream::new(self.peer_initial_max_stream_data)),
        };
        self.streams[Owner::Local].insert(id, stream);

        Ok(()).into()
    }

    fn poll_accept_stream(&mut self, _cx: &mut Context) -> Poll<Result<Option<u64>>> {
        if !self.rx_open {
            return Ok(None).into();
        }

        if let Some(id) = self.pending_accept.pop_front() {
            Ok(Some(id)).into()
        } else {
            Poll::Pending
        }
    }

    fn poll_send(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: u64,
        _cx: &mut Context,
    ) -> Poll<Result<u64>> {
        if !self.write_buf.request_push(self.config.max_write_queue_len) {
            return Poll::Pending;
        }

        let stream = self.streams[owner]
            .get_mut(&id)
            .ok_or("missing stream")?
            .tx
            .as_mut()
            .ok_or("missing tx stream")?;

        let allowed_bytes = bytes.min(self.config.max_stream_frame_len as _);

        if let Some(data) = stream.send(allowed_bytes) {
            let len = data.len() as u64;
            self.write_buf.push(frame::Frame::StreamData {
                id,
                owner: !owner,
                data,
            });
            Ok(len).into()
        } else {
            Poll::Pending
        }
    }

    fn poll_receive(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: u64,
        _cx: &mut Context,
    ) -> Poll<Result<u64>> {
        let stream = self.streams[owner]
            .get_mut(&id)
            .ok_or("missing stream")?
            .rx
            .as_mut()
            .ok_or("missing rx stream")?;

        let len = ready!(stream.receive(bytes))?;

        if stream.receive_window() < self.config.stream_window / 2 {
            self.max_stream_data.insert((owner, id));
        }

        Ok(len).into()
    }

    fn poll_send_finish(&mut self, owner: Owner, id: u64, _cx: &mut Context) -> Poll<Result<()>> {
        if !self.write_buf.request_push(self.config.max_write_queue_len) {
            return Poll::Pending;
        }

        if let Entry::Occupied(mut entry) = self.streams[owner].entry(id) {
            let stream = entry.get_mut();

            if stream.tx.take().is_some() {
                self.write_buf
                    .push(frame::Frame::StreamFinish { id, owner: !owner });
            }

            if stream.rx.is_none() {
                entry.remove();
            }
        }

        Ok(()).into()
    }

    fn poll_receive_finish(
        &mut self,
        owner: Owner,
        id: u64,
        _cx: &mut Context,
    ) -> Poll<Result<()>> {
        if let Entry::Occupied(mut entry) = self.streams[owner].entry(id) {
            let stream = entry.get_mut();
            stream.rx = None;
            if stream.tx.is_none() {
                entry.remove();
            }
        }

        Ok(()).into()
    }

    fn poll_progress(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        loop {
            for (owner, id) in self.max_stream_data.drain() {
                let stream = self.streams[owner]
                    .get_mut(&id)
                    .unwrap()
                    .rx
                    .as_mut()
                    .unwrap();
                let up_to = stream.credit(self.config.stream_window);
                self.write_buf.push_priority(Frame::MaxStreamData {
                    id,
                    owner: !owner,
                    up_to,
                });
            }

            self.flush_write_buffer(cx)?;

            self.flush_read_buffer(cx)?;

            // only read from the socket if it's open and we don't have a pending frame
            if !(self.rx_open && self.frame.is_none()) {
                return Ok(()).into();
            }

            let rx_open = &mut self.rx_open;
            let inner = self.inner.as_mut();

            ready!(self.read_buf.read(|buf| {
                ready!(inner.poll_read(cx, buf))?;

                // the socket returned a 0 write
                if buf.filled().is_empty() {
                    *rx_open = false;
                }

                Ok(()).into()
            }))?;
        }
    }

    fn poll_finish(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        self.flush_write_buffer(cx)?;

        if !self.write_buf.is_empty() {
            return Poll::Pending;
        }

        ready!(self.inner.as_mut().poll_flush(cx))?;
        Ok(()).into()
    }
}

#[derive(Debug, Default)]
struct Blocked(bool);

impl Blocked {
    pub fn block(&mut self) {
        self.0 = true;
    }

    pub fn unblock(&mut self, cx: &mut Context) {
        if self.0 {
            self.0 = false;
            cx.waker().wake_by_ref();
        }
    }
}
