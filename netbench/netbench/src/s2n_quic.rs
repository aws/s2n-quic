// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::Owner, helper::IdPrefixReader, scenario, Result};
use bytes::Bytes;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;
use s2n_quic::{
    connection,
    stream::{LocalStream, PeerStream, SplittableStream},
};
use s2n_quic_core::stream::testing::Data;
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};

fn stream_error(err: s2n_quic::stream::Error) -> Result<()> {
    if let s2n_quic::stream::Error::StreamReset { error, .. } = err {
        if *error == 0 {
            return Ok(());
        }
    }

    if let s2n_quic::stream::Error::ConnectionError { error, .. } = err {
        return conn_error(error);
    }

    Err(err.into())
}

fn conn_error(err: s2n_quic::connection::Error) -> Result<()> {
    if let s2n_quic::connection::Error::Application { error, .. } = err {
        if *error == 0 {
            return Ok(());
        }
    }

    Err(err.into())
}

impl<'a> crate::client::Client<'a> for s2n_quic::Client {
    type Connect = Connect<'a>;
    type Connection = crate::Driver<'a, Connection>;

    fn connect(
        &mut self,
        addr: std::net::SocketAddr,
        server_name: &str,
        _server_conn_id: u64,
        scenario: &'a Arc<scenario::Connection>,
    ) -> Self::Connect {
        let connect = s2n_quic::client::Connect::new(addr).with_server_name(server_name);
        let attempt = s2n_quic::Client::connect(self, connect);
        Connect { attempt, scenario }
    }
}

pub struct Connect<'a> {
    attempt: s2n_quic::client::ConnectionAttempt,
    scenario: &'a scenario::Connection,
}

impl<'a> Future for Connect<'a> {
    type Output = Result<crate::Driver<'a, Connection>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let conn = ready!(Pin::new(&mut self.attempt).poll(cx))?;
        let conn = Connection::new(conn);
        let conn = crate::Driver::new(self.scenario, conn);
        Ok(conn).into()
    }
}

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
                    Poll::Ready(stream_error(err))
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
            Poll::Ready(Err(err)) => Poll::Ready(stream_error(err)),
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
                stream.inner.finish().or_else(stream_error)?;
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

        while self.buffered <= bytes && self.is_open {
            let mut chunks = chunks!();

            if let Poll::Ready(res) = self.inner.poll_receive_vectored(&mut chunks, cx) {
                let (count, is_open) = res?;
                self.is_open &= is_open;

                for chunk in &chunks[..count] {
                    self.buffered += chunk.len() as u64;
                }
            } else {
                break;
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

    fn poll_send(&mut self, mut bytes: u64, cx: &mut Context) -> Poll<Result<u64>> {
        if bytes == 0 {
            return Ok(0).into();
        }

        let mut len = 0;
        let mut data = self.data;

        while bytes > 0 {
            let mut chunks = chunks!();

            let count = data.send(bytes as usize, &mut chunks).unwrap();
            let initial_len: u64 = chunks.iter().map(|chunk| chunk.len() as u64).sum();

            let count = if let Poll::Ready(count) =
                self.inner.poll_send_vectored(&mut chunks[..count], cx)?
            {
                count
            } else {
                break;
            };

            if count == chunks.len() {
                len += initial_len;
                bytes -= initial_len;
                continue;
            }

            let remaining_len: u64 = chunks[count..].iter().map(|chunk| chunk.len() as u64).sum();

            len += initial_len - remaining_len;

            break;
        }

        if len == 0 {
            return Poll::Pending;
        }

        self.data.seek_forward(len as usize);

        Poll::Ready(Ok(len))
    }
}
