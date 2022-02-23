// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::Owner, helper::IdPrefixReader, Result};
use bytes::Bytes;
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;
use s2n_quic::{
    connection,
    stream::{LocalStream, PeerStream, SplittableStream},
};
use s2n_quic_core::stream::testing::Data;
use std::collections::{hash_map::Entry, HashMap};

pub struct Connection {
    conn: s2n_quic::Connection,
    streams: [HashMap<u64, Stream>; 2],
    opened_streams: HashMap<u64, (Bytes, LocalStream)>,
    unidentified_peer_stream: Option<(IdPrefixReader, PeerStream)>,
}

impl From<s2n_quic::Connection> for Connection {
    fn from(conn: s2n_quic::Connection) -> Self {
        Self::new(conn)
    }
}

impl Connection {
    pub fn new(connection: s2n_quic::Connection) -> Self {
        Self {
            conn: connection,
            streams: [HashMap::new(), HashMap::new()],
            opened_streams: HashMap::new(),
            unidentified_peer_stream: Default::default(),
        }
    }

    pub fn into_inner(self) -> s2n_quic::Connection {
        self.conn
    }

    fn open_local_stream<
        F: FnOnce(&mut s2n_quic::Connection, &mut Context) -> Poll<Result<S, connection::Error>>,
        S: Into<LocalStream>,
    >(
        &mut self,
        id: u64,
        open: F,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        // the stream has already been opened and is waiting to send the prefix
        if let Entry::Occupied(mut entry) = self.opened_streams.entry(id) {
            let (prefix, stream) = entry.get_mut();
            return match stream.poll_send(prefix, cx) {
                Poll::Ready(Ok(_)) => {
                    let (_, stream) = entry.remove();
                    let stream = Stream::new(stream);
                    self.streams[Owner::Local].insert(id, stream);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(err)) => {
                    entry.remove();
                    Poll::Ready(Err(err.into()))
                }
                Poll::Pending => Poll::Pending,
            };
        }

        let mut stream = ready!(open(&mut self.conn, cx))?.into();

        let mut prefix = Bytes::copy_from_slice(&id.to_be_bytes());

        match stream.poll_send(&mut prefix, cx) {
            Poll::Ready(Ok(_)) => {
                let stream = Stream::new(stream);
                self.streams[Owner::Local].insert(id, stream);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err.into())),
            Poll::Pending => {
                self.opened_streams.insert(id, (prefix, stream));
                Poll::Pending
            }
        }
    }
}

impl super::Connection for Connection {
    fn poll_open_bidirectional_stream(&mut self, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        self.open_local_stream(id, |conn, cx| conn.poll_open_bidirectional_stream(cx), cx)
    }

    fn poll_open_send_stream(&mut self, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        self.open_local_stream(id, |conn, cx| conn.poll_open_send_stream(cx), cx)
    }

    fn poll_accept_stream(&mut self, cx: &mut Context) -> Poll<Result<Option<u64>>> {
        loop {
            if let Some((id, stream)) = self.unidentified_peer_stream.as_mut() {
                let len = ready!(futures::io::AsyncRead::poll_read(
                    Pin::new(stream),
                    cx,
                    id.remaining()
                ))?;
                let id = ready!(id.on_read(len));

                let (_, stream) = self.unidentified_peer_stream.take().unwrap();
                let stream = Stream::new(stream);
                self.streams[Owner::Remote].insert(id, stream);
                return Poll::Ready(Ok(Some(id)));
            }

            let stream = ready!(self.conn.poll_accept(cx));

            if let Ok(Some(stream)) = stream {
                self.unidentified_peer_stream = Some((Default::default(), stream));
            } else {
                return Poll::Ready(Ok(None));
            };
        }
    }

    fn poll_send(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: u64,
        cx: &mut Context,
    ) -> Poll<Result<u64>> {
        self.streams[owner]
            .get_mut(&id)
            .unwrap()
            .tx
            .as_mut()
            .unwrap()
            .poll_send(bytes, cx)
    }

    fn poll_receive(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: u64,
        cx: &mut Context,
    ) -> Poll<Result<u64>> {
        self.streams[owner]
            .get_mut(&id)
            .unwrap()
            .rx
            .as_mut()
            .unwrap()
            .poll_receive(bytes, cx)
    }

    fn poll_send_finish(&mut self, owner: Owner, id: u64, _cx: &mut Context) -> Poll<Result<()>> {
        if let Entry::Occupied(mut entry) = self.streams[owner].entry(id) {
            let stream = entry.get_mut();
            if let Some(stream) = stream.tx.as_mut() {
                stream.inner.finish()?;
            }

            if stream.rx.is_none() {
                entry.remove();
            }
        }

        Poll::Ready(Ok(()))
    }

    fn poll_receive_finish(
        &mut self,
        owner: Owner,
        id: u64,
        _cx: &mut Context,
    ) -> Poll<Result<()>> {
        if let Entry::Occupied(mut entry) = self.streams[owner].entry(id) {
            let stream = entry.get_mut();
            if let Some(mut stream) = stream.rx.take() {
                let _ = stream.inner.stop_sending(0u8.into());
            }

            if stream.tx.is_none() {
                entry.remove();
            }
        }

        Poll::Ready(Ok(()))
    }
}

macro_rules! chunks {
    () => {
        [
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
            Bytes::new(),
        ]
    };
}

struct Stream {
    rx: Option<ReceiveStream>,
    tx: Option<SendStream>,
}

impl Stream {
    fn new(stream: impl SplittableStream) -> Self {
        let (rx, tx) = stream.split();
        let rx = rx.map(ReceiveStream::new);
        let tx = tx.map(SendStream::new);
        Self { rx, tx }
    }
}

struct ReceiveStream {
    inner: s2n_quic::stream::ReceiveStream,
    buffered: u64,
    is_open: bool,
}

impl ReceiveStream {
    fn new(inner: s2n_quic::stream::ReceiveStream) -> Self {
        Self {
            inner,
            buffered: 0,
            is_open: true,
        }
    }

    fn poll_receive(&mut self, bytes: u64, cx: &mut Context) -> Poll<Result<u64>> {
        if !self.is_open && self.buffered == 0 {
            return Ok(0).into();
        }

        if self.buffered <= bytes {
            let mut chunks = chunks!();

            if let Poll::Ready(res) = self.inner.poll_receive_vectored(&mut chunks, cx) {
                let (count, is_open) = res?;
                self.is_open &= is_open;

                for chunk in &chunks[..count] {
                    self.buffered += chunk.len() as u64;
                }
            }
        }

        let received_len = bytes.min(self.buffered);
        self.buffered -= received_len;

        if !self.is_open && received_len == 0 {
            return Ok(0).into();
        }

        if received_len == 0 {
            Poll::Pending
        } else {
            Ok(received_len).into()
        }
    }
}

struct SendStream {
    inner: s2n_quic::stream::SendStream,
    data: Data,
}

impl SendStream {
    fn new(inner: s2n_quic::stream::SendStream) -> Self {
        Self {
            inner,
            data: Data::new(u64::MAX),
        }
    }

    fn poll_send(&mut self, bytes: u64, cx: &mut Context) -> Poll<Result<u64>> {
        let mut chunks = chunks!();

        let count = self.data.clone().send(bytes as usize, &mut chunks).unwrap();
        let initial_len: u64 = chunks.iter().map(|chunk| chunk.len() as u64).sum();

        let count = ready!(self.inner.poll_send_vectored(&mut chunks[..count], cx))?;
        let remaining_len: u64 = chunks[count..].iter().map(|chunk| chunk.len() as u64).sum();

        let len = initial_len - remaining_len;

        self.data.seek_forward(len as usize);

        Poll::Ready(Ok(len))
    }
}
