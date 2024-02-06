// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::{Buf, Bytes};
use core::task::ready;
use h3::quic::{self, Error, StreamId, WriteBuf};
use s2n_quic::stream::{BidirectionalStream, ReceiveStream};
use s2n_quic_core::varint::VarInt;
use std::{
    convert::TryInto,
    fmt::{self, Display},
    sync::Arc,
    task::{self, Poll},
};

pub struct Connection {
    conn: s2n_quic::connection::Handle,
    bidi_acceptor: s2n_quic::connection::BidirectionalStreamAcceptor,
    recv_acceptor: s2n_quic::connection::ReceiveStreamAcceptor,
}

impl Connection {
    pub fn new(new_conn: s2n_quic::Connection) -> Self {
        let (handle, acceptor) = new_conn.split();
        let (bidi, recv) = acceptor.split();

        Self {
            conn: handle,
            bidi_acceptor: bidi,
            recv_acceptor: recv,
        }
    }
}

#[derive(Debug)]
pub struct ConnectionError(s2n_quic::connection::Error);

impl std::error::Error for ConnectionError {}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Error for ConnectionError {
    fn is_timeout(&self) -> bool {
        matches!(self.0, s2n_quic::connection::Error::IdleTimerExpired { .. })
    }

    fn err_code(&self) -> Option<u64> {
        match self.0 {
            s2n_quic::connection::Error::Application { error, .. } => Some(error.into()),
            _ => None,
        }
    }
}

impl From<s2n_quic::connection::Error> for ConnectionError {
    fn from(e: s2n_quic::connection::Error) -> Self {
        Self(e)
    }
}

impl<B> quic::Connection<B> for Connection
where
    B: Buf,
{
    type BidiStream = BidiStream<B>;
    type SendStream = SendStream<B>;
    type RecvStream = RecvStream;
    type OpenStreams = OpenStreams;
    type Error = ConnectionError;

    fn poll_accept_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Option<Self::RecvStream>, Self::Error>> {
        let recv = match ready!(self.recv_acceptor.poll_accept_receive_stream(cx))? {
            Some(x) => x,
            None => return Poll::Ready(Ok(None)),
        };
        Poll::Ready(Ok(Some(Self::RecvStream::new(recv))))
    }

    fn poll_accept_bidi(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Option<Self::BidiStream>, Self::Error>> {
        let (recv, send) = match ready!(self.bidi_acceptor.poll_accept_bidirectional_stream(cx))? {
            Some(x) => x.split(),
            None => return Poll::Ready(Ok(None)),
        };
        Poll::Ready(Ok(Some(Self::BidiStream {
            send: Self::SendStream::new(send),
            recv: Self::RecvStream::new(recv),
        })))
    }

    fn poll_open_bidi(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::BidiStream, Self::Error>> {
        let stream = ready!(self.conn.poll_open_bidirectional_stream(cx))?;
        Ok(stream.into()).into()
    }

    fn poll_open_send(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::SendStream, Self::Error>> {
        let stream = ready!(self.conn.poll_open_send_stream(cx))?;
        Ok(stream.into()).into()
    }

    fn opener(&self) -> Self::OpenStreams {
        OpenStreams {
            conn: self.conn.clone(),
        }
    }

    fn close(&mut self, code: h3::error::Code, _reason: &[u8]) {
        self.conn.close(
            code.value()
                .try_into()
                .expect("s2n-quic supports error codes up to 2^62-1"),
        );
    }
}

pub struct OpenStreams {
    conn: s2n_quic::connection::Handle,
}

impl<B> quic::OpenStreams<B> for OpenStreams
where
    B: Buf,
{
    type BidiStream = BidiStream<B>;
    type SendStream = SendStream<B>;
    type RecvStream = RecvStream;
    type Error = ConnectionError;

    fn poll_open_bidi(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::BidiStream, Self::Error>> {
        let stream = ready!(self.conn.poll_open_bidirectional_stream(cx))?;
        Ok(stream.into()).into()
    }

    fn poll_open_send(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::SendStream, Self::Error>> {
        let stream = ready!(self.conn.poll_open_send_stream(cx))?;
        Ok(stream.into()).into()
    }

    fn close(&mut self, code: h3::error::Code, _reason: &[u8]) {
        self.conn.close(
            code.value()
                .try_into()
                .unwrap_or_else(|_| VarInt::MAX.into()),
        );
    }
}

impl Clone for OpenStreams {
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
        }
    }
}

pub struct BidiStream<B>
where
    B: Buf,
{
    send: SendStream<B>,
    recv: RecvStream,
}

impl<B> quic::BidiStream<B> for BidiStream<B>
where
    B: Buf,
{
    type SendStream = SendStream<B>;
    type RecvStream = RecvStream;

    fn split(self) -> (Self::SendStream, Self::RecvStream) {
        (self.send, self.recv)
    }
}

impl<B> quic::RecvStream for BidiStream<B>
where
    B: Buf,
{
    type Buf = Bytes;
    type Error = ReadError;

    fn poll_data(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Option<Self::Buf>, Self::Error>> {
        self.recv.poll_data(cx)
    }

    fn stop_sending(&mut self, error_code: u64) {
        self.recv.stop_sending(error_code)
    }

    fn recv_id(&self) -> StreamId {
        self.recv.recv_id()
    }
}

impl<B> quic::SendStream<B> for BidiStream<B>
where
    B: Buf,
{
    type Error = SendStreamError;

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.send.poll_ready(cx)
    }

    fn poll_finish(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.send.poll_finish(cx)
    }

    fn reset(&mut self, reset_code: u64) {
        self.send.reset(reset_code)
    }

    fn send_data<D: Into<WriteBuf<B>>>(&mut self, data: D) -> Result<(), Self::Error> {
        self.send.send_data(data)
    }

    fn send_id(&self) -> StreamId {
        self.send.send_id()
    }
}

impl<B> From<BidirectionalStream> for BidiStream<B>
where
    B: Buf,
{
    fn from(bidi: BidirectionalStream) -> Self {
        let (recv, send) = bidi.split();
        BidiStream {
            send: send.into(),
            recv: recv.into(),
        }
    }
}

pub struct RecvStream {
    stream: s2n_quic::stream::ReceiveStream,
}

impl RecvStream {
    fn new(stream: s2n_quic::stream::ReceiveStream) -> Self {
        Self { stream }
    }
}

impl quic::RecvStream for RecvStream {
    type Buf = Bytes;
    type Error = ReadError;

    fn poll_data(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Option<Self::Buf>, Self::Error>> {
        let buf = ready!(self.stream.poll_receive(cx))?;
        Ok(buf).into()
    }

    fn stop_sending(&mut self, error_code: u64) {
        let _ = self.stream.stop_sending(
            s2n_quic::application::Error::new(error_code)
                .expect("s2n-quic supports error codes up to 2^62-1"),
        );
    }

    fn recv_id(&self) -> StreamId {
        self.stream.id().try_into().expect("invalid stream id")
    }
}

impl From<ReceiveStream> for RecvStream {
    fn from(recv: ReceiveStream) -> Self {
        RecvStream::new(recv)
    }
}

#[derive(Debug)]
pub struct ReadError(s2n_quic::stream::Error);

impl std::error::Error for ReadError {}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<ReadError> for Arc<dyn Error> {
    fn from(e: ReadError) -> Self {
        Arc::new(e)
    }
}

impl From<s2n_quic::stream::Error> for ReadError {
    fn from(e: s2n_quic::stream::Error) -> Self {
        Self(e)
    }
}

impl Error for ReadError {
    fn is_timeout(&self) -> bool {
        matches!(
            self.0,
            s2n_quic::stream::Error::ConnectionError {
                error: s2n_quic::connection::Error::IdleTimerExpired { .. },
                ..
            }
        )
    }

    fn err_code(&self) -> Option<u64> {
        match self.0 {
            s2n_quic::stream::Error::ConnectionError {
                error: s2n_quic::connection::Error::Application { error, .. },
                ..
            } => Some(error.into()),
            s2n_quic::stream::Error::StreamReset { error, .. } => Some(error.into()),
            _ => None,
        }
    }
}

pub struct SendStream<B: Buf> {
    stream: s2n_quic::stream::SendStream,
    chunk: Option<Bytes>,
    buf: Option<WriteBuf<B>>, // TODO: Replace with buf: PhantomData<B>
                              //       after https://github.com/hyperium/h3/issues/78 is resolved
}

impl<B> SendStream<B>
where
    B: Buf,
{
    fn new(stream: s2n_quic::stream::SendStream) -> SendStream<B> {
        Self {
            stream,
            chunk: None,
            buf: Default::default(),
        }
    }
}

impl<B> quic::SendStream<B> for SendStream<B>
where
    B: Buf,
{
    type Error = SendStreamError;

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        loop {
            // try to flush the current chunk if we have one
            if let Some(chunk) = self.chunk.as_mut() {
                ready!(self.stream.poll_send(chunk, cx))?;

                // s2n-quic will take the whole chunk on send, even if it exceeds the limits
                debug_assert!(chunk.is_empty());
                self.chunk = None;
            }

            // try to take the next chunk from the WriteBuf
            if let Some(ref mut data) = self.buf {
                let len = data.chunk().len();

                // if the write buf is empty, then clear it and break
                if len == 0 {
                    self.buf = None;
                    break;
                }

                // copy the first chunk from WriteBuf and prepare it to flush
                let chunk = data.copy_to_bytes(len);
                self.chunk = Some(chunk);

                // loop back around to flush the chunk
                continue;
            }

            // if we didn't have either a chunk or WriteBuf, then we're ready
            break;
        }

        Poll::Ready(Ok(()))

        // TODO: Replace with following after https://github.com/hyperium/h3/issues/78 is resolved
        // self.available_bytes = ready!(self.stream.poll_send_ready(cx))?;
        // Poll::Ready(Ok(()))
    }

    fn send_data<D: Into<WriteBuf<B>>>(&mut self, data: D) -> Result<(), Self::Error> {
        if self.buf.is_some() {
            return Err(Self::Error::NotReady);
        }
        self.buf = Some(data.into());
        Ok(())

        // TODO: Replace with following after https://github.com/hyperium/h3/issues/78 is resolved
        // let mut data = data.into();
        // while self.available_bytes > 0 && data.has_remaining() {
        //     let len = data.chunk().len();
        //     let chunk = data.copy_to_bytes(len);
        //     self.stream.send_data(chunk)?;
        //     self.available_bytes = self.available_bytes.saturating_sub(len);
        // }
        // Ok(())
    }

    fn poll_finish(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        // ensure all chunks are flushed to the QUIC stream before finishing
        ready!(self.poll_ready(cx))?;
        self.stream.finish()?;
        Ok(()).into()
    }

    fn reset(&mut self, reset_code: u64) {
        let _ = self
            .stream
            .reset(reset_code.try_into().unwrap_or_else(|_| VarInt::MAX.into()));
    }

    fn send_id(&self) -> StreamId {
        self.stream.id().try_into().expect("invalid stream id")
    }
}

impl<B> From<s2n_quic::stream::SendStream> for SendStream<B>
where
    B: Buf,
{
    fn from(send: s2n_quic::stream::SendStream) -> Self {
        SendStream::new(send)
    }
}

#[derive(Debug)]
pub enum SendStreamError {
    Write(s2n_quic::stream::Error),
    NotReady,
}

impl std::error::Error for SendStreamError {}

impl Display for SendStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<s2n_quic::stream::Error> for SendStreamError {
    fn from(e: s2n_quic::stream::Error) -> Self {
        Self::Write(e)
    }
}

impl Error for SendStreamError {
    fn is_timeout(&self) -> bool {
        matches!(
            self,
            Self::Write(s2n_quic::stream::Error::ConnectionError {
                error: s2n_quic::connection::Error::IdleTimerExpired { .. },
                ..
            })
        )
    }

    fn err_code(&self) -> Option<u64> {
        match self {
            Self::Write(s2n_quic::stream::Error::StreamReset { error, .. }) => {
                Some((*error).into())
            }
            Self::Write(s2n_quic::stream::Error::ConnectionError {
                error: s2n_quic::connection::Error::Application { error, .. },
                ..
            }) => Some((*error).into()),
            _ => None,
        }
    }
}

impl From<SendStreamError> for Arc<dyn Error> {
    fn from(e: SendStreamError) -> Self {
        Arc::new(e)
    }
}
