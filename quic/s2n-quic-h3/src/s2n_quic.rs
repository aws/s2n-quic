// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::{Buf, Bytes};
use core::task::ready;
use h3::quic::{self, ConnectionErrorIncoming, StreamErrorIncoming, StreamId, WriteBuf};
use s2n_quic::{
    application,
    stream::{BidirectionalStream, ReceiveStream},
};
use std::{
    convert::TryInto,
    sync::Arc,
    task::{self, Poll},
};

#[cfg(feature = "tracing")]
use tracing::instrument;

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

impl<B> quic::Connection<B> for Connection
where
    B: Buf,
{
    type RecvStream = RecvStream;
    type OpenStreams = OpenStreams;

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn poll_accept_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::RecvStream, ConnectionErrorIncoming>> {
        let recv = match ready!(self.recv_acceptor.poll_accept_receive_stream(cx)) {
            Ok(Some(x)) => x,
            Ok(None) => {
                // This happens when the connection is closed without an error
                return Poll::Ready(Err(ConnectionErrorIncoming::InternalError(
                    "connection closed".to_string(),
                )));
            }
            Err(e) => {
                return Poll::Ready(Err(convert_connection_error(e)));
            }
        };

        Poll::Ready(Ok(Self::RecvStream::new(recv)))
    }

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn poll_accept_bidi(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::BidiStream, ConnectionErrorIncoming>> {
        let (recv, send) = match ready!(self.bidi_acceptor.poll_accept_bidirectional_stream(cx)) {
            Ok(Some(x)) => x.split(),
            Ok(None) => {
                // This happens when the connection is closed without an error
                return Poll::Ready(Err(ConnectionErrorIncoming::InternalError(
                    "connection closed".to_string(),
                )));
            }
            Err(e) => {
                return Poll::Ready(Err(convert_connection_error(e)));
            }
        };

        Poll::Ready(Ok(Self::BidiStream {
            send: Self::SendStream::new(send),
            recv: Self::RecvStream::new(recv),
        }))
    }

    fn opener(&self) -> Self::OpenStreams {
        OpenStreams {
            conn: self.conn.clone(),
        }
    }
}

fn convert_connection_error(e: s2n_quic::connection::Error) -> h3::quic::ConnectionErrorIncoming {
    match e {
        s2n_quic::connection::Error::Application { error, .. } => {
            ConnectionErrorIncoming::ApplicationClose {
                error_code: error.into(),
            }
        }
        s2n_quic::connection::Error::IdleTimerExpired { .. } => ConnectionErrorIncoming::Timeout,

        s2n_quic::connection::Error::Closed { .. } => {
            // This happens when the connection is closed without an error
            ConnectionErrorIncoming::InternalError("connection closed".to_string())
        }

        error => ConnectionErrorIncoming::Undefined(Arc::new(error)),
    }
}

impl<B> quic::OpenStreams<B> for Connection
where
    B: Buf,
{
    type BidiStream = BidiStream<B>;
    type SendStream = SendStream<B>;

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn poll_open_bidi(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::BidiStream, StreamErrorIncoming>> {
        self.conn
            .poll_open_bidirectional_stream(cx)
            .map_ok(|stream| stream.into())
            .map_err(|err| StreamErrorIncoming::ConnectionErrorIncoming {
                connection_error: convert_connection_error(err),
            })
    }

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn poll_open_send(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::SendStream, StreamErrorIncoming>> {
        self.conn
            .poll_open_send_stream(cx)
            .map_ok(|stream| stream.into())
            .map_err(|err| StreamErrorIncoming::ConnectionErrorIncoming {
                connection_error: convert_connection_error(err),
            })
    }

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
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

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn poll_open_bidi(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::BidiStream, StreamErrorIncoming>> {
        self.conn
            .poll_open_bidirectional_stream(cx)
            .map_ok(|stream| stream.into())
            .map_err(|err| StreamErrorIncoming::ConnectionErrorIncoming {
                connection_error: convert_connection_error(err),
            })
    }

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn poll_open_send(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::SendStream, StreamErrorIncoming>> {
        self.conn
            .poll_open_send_stream(cx)
            .map_ok(|stream| stream.into())
            .map_err(|err| StreamErrorIncoming::ConnectionErrorIncoming {
                connection_error: convert_connection_error(err),
            })
    }

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn close(&mut self, code: h3::error::Code, _reason: &[u8]) {
        self.conn.close(
            code.value()
                .try_into()
                .unwrap_or(application::Error::UNKNOWN),
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

    fn poll_data(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Option<Self::Buf>, StreamErrorIncoming>> {
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
    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        self.send.poll_ready(cx)
    }

    fn poll_finish(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        self.send.poll_finish(cx)
    }

    fn reset(&mut self, reset_code: u64) {
        self.send.reset(reset_code)
    }

    fn send_data<D: Into<WriteBuf<B>>>(&mut self, data: D) -> Result<(), StreamErrorIncoming> {
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

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn poll_data(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Option<Self::Buf>, StreamErrorIncoming>> {
        self.stream
            .poll_receive(cx)
            .map_err(convert_stream_error_to_h3_stream_error_incoming)
    }

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn stop_sending(&mut self, error_code: u64) {
        let _ = self.stream.stop_sending(
            s2n_quic::application::Error::new(error_code)
                .expect("s2n-quic supports error codes up to 2^62-1"),
        );
    }

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn recv_id(&self) -> StreamId {
        self.stream.id().try_into().expect("invalid stream id")
    }
}

impl From<ReceiveStream> for RecvStream {
    fn from(recv: ReceiveStream) -> Self {
        RecvStream::new(recv)
    }
}

fn convert_stream_error_to_h3_stream_error_incoming(
    error: s2n_quic::stream::Error,
) -> StreamErrorIncoming {
    match error {
        s2n_quic::stream::Error::StreamReset { error, .. } => {
            StreamErrorIncoming::StreamTerminated {
                error_code: error.into(),
            }
        }
        s2n_quic::stream::Error::ConnectionError { error, .. } => {
            StreamErrorIncoming::ConnectionErrorIncoming {
                connection_error: convert_connection_error(error),
            }
        }

        error => StreamErrorIncoming::Unknown(Box::new(error)),
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
    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        loop {
            // try to flush the current chunk if we have one
            if let Some(chunk) = self.chunk.as_mut() {
                ready!(self.stream.poll_send(chunk, cx))
                    .map_err(convert_stream_error_to_h3_stream_error_incoming)?;

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

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn send_data<D: Into<WriteBuf<B>>>(&mut self, data: D) -> Result<(), StreamErrorIncoming> {
        if self.buf.is_some() {
            return Err(StreamErrorIncoming::ConnectionErrorIncoming {
                connection_error: ConnectionErrorIncoming::InternalError(
                    "internal error in the http stack".to_string(),
                ),
            });
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

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn poll_finish(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        // ensure all chunks are flushed to the QUIC stream before finishing
        ready!(self.poll_ready(cx))?;
        self.stream
            .finish()
            .map_err(convert_stream_error_to_h3_stream_error_incoming)?;

        Ok(()).into()
    }

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
    fn reset(&mut self, reset_code: u64) {
        let _ = self
            .stream
            .reset(reset_code.try_into().unwrap_or(application::Error::UNKNOWN));
    }

    #[cfg_attr(feature = "tracing", instrument(skip_all, level = "trace"))]
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
