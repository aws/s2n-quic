// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines the Stream objects that applications are interacting with

use crate::connection::ConnectionApi;
use bytes::Bytes;
use core::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
pub use s2n_quic_core::{
    application::ApplicationErrorCode,
    stream::{ops, StreamError, StreamId, StreamType},
};

#[derive(Clone)]
struct State {
    connection: ConnectionApi,
    stream_id: StreamId,
    is_rx_open: bool,
    is_tx_open: bool,
}

impl State {
    fn new(connection: ConnectionApi, stream_id: StreamId) -> Self {
        Self {
            connection,
            stream_id,
            is_rx_open: true,
            is_tx_open: true,
        }
    }

    fn poll_request(
        &mut self,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError> {
        let id = self.stream_id;
        self.connection.poll_request(id, request, context)
    }

    fn request(&mut self) -> Request {
        Request {
            state: self,
            request: ops::Request::default(),
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let is_rx_open = self.is_rx_open;
        let is_tx_open = self.is_tx_open;

        if is_rx_open || is_tx_open {
            let mut request = self.request();

            if is_tx_open {
                // Dropping a send stream will automatically finish the stream
                //
                // This is to stay consistent with std::net::TcpStream
                request.finish();
            }

            if is_rx_open {
                // Send a STOP_SENDING message on the receiving half of the `Stream`,
                // for the case the application did not consume all data.
                // If that already happened, this will be a noop.
                request.stop_sending(ApplicationErrorCode::UNKNOWN);
            }

            let _ = request.poll(None);
        }
    }
}

macro_rules! ready {
    ($e:expr $(,)?) => {
        match $e {
            core::task::Poll::Ready(t) => t,
            core::task::Poll::Pending => return core::task::Poll::Pending,
        }
    };
}

macro_rules! tx_stream_apis {
    () => {
        /// Enqueues a chunk of data for sending it towards the peer.
        ///
        /// The method will return:
        /// - `Poll::Ready(Ok(()))` if the data was enqueued for sending. The provided `Bytes` will
        ///   be replaced with an empty `Bytes`, in order to reduce needless ref count increases.
        /// - `Poll::Ready(Err(stream_error))` if the data could not be sent, because the stream
        ///   had previously entered an error state.
        /// - `Poll::Pending` if the send buffer capacity is currently exhausted. In this case, the
        ///   caller should retry sending after the `Waker` on the provided `Context` is notified.
        pub fn poll_send(
            &mut self,
            chunk: &mut Bytes,
            cx: &mut Context,
        ) -> Poll<Result<(), StreamError>> {
            if chunk.is_empty() {
                return Poll::Ready(Ok(()));
            }

            self.tx_request()?
                .send(core::slice::from_mut(chunk))
                .poll(Some(cx))?
                .into()
        }

        /// Enqueues a slice of chunks of data for sending it towards the peer.
        ///
        /// The method will return:
        /// - `Poll::Ready(Ok(count))` if part of the slice was enqueued for sending. Any of the
        ///   consumed `Bytes` will be replaced with an empty `Bytes`, in order to reduce needless
        ///   ref count increases.
        /// - `Poll::Ready(Err(stream_error))` if the data could not be sent, because the stream
        ///   had previously entered an error state.
        /// - `Poll::Pending` if the send buffer capacity is currently exhausted. In this case, the
        ///   caller should retry sending after the `Waker` on the provided `Context` is notified.
        pub fn poll_send_vectored(
            &mut self,
            chunks: &mut [Bytes],
            cx: &mut Context,
        ) -> Poll<Result<usize, StreamError>> {
            if chunks.is_empty() {
                return Poll::Ready(Ok(0));
            }

            let response = ready!(self.tx_request()?.send(chunks).poll(Some(cx))?.into_poll());

            Ok(response.tx().expect("invalid response").chunks.consumed).into()
        }

        /// Polls send readiness for the given stream.
        ///
        /// The method will return:
        /// - `Poll::Ready(Ok(available_bytes))` if the stream is ready to send data, where
        ///   `available_bytes` is how many bytes the stream can currently accept.
        /// - `Poll::Ready(Err(stream_error))` if the data could not be sent, because the stream
        ///   had previously entered an error state.
        /// - `Poll::Pending` if the send buffer capacity is currently exhausted. In this case, the
        ///   caller should retry sending after the `Waker` on the provided `Context` is notified.
        pub fn poll_send_ready(&mut self, cx: &mut Context) -> Poll<Result<usize, StreamError>> {
            let response = ready!(self
                .tx_request()?
                .send_readiness()
                .poll(Some(cx))?
                .into_poll());
            Ok(response.tx().expect("invalid response").bytes.available).into()
        }

        /// Enqueues a chunk of data for sending it towards the peer.
        ///
        /// This method should only be called after calling `poll_send_ready` first, as the stream
        /// may not have available send buffer capacity.
        ///
        /// The method will return:
        /// - `Ok(())` if the data was enqueued for sending.
        /// - `Err(stream_error)` if the data could not be sent, because the stream
        ///   had previously entered an error state, or the stream was not ready to send data.
        pub fn send_data(&mut self, chunk: Bytes) -> Result<(), StreamError> {
            if chunk.is_empty() {
                return Ok(());
            }

            match self.tx_request()?.send(&mut [chunk]).poll(None)? {
                response if response.tx().expect("invalid response").chunks.consumed == 1 => Ok(()),
                _ => Err(StreamError::SendingBlocked),
            }
        }

        /// Flushes the send buffer and waits for acknowledgement from the peer.
        ///
        /// The method will return:
        /// - `Poll::Ready(Ok(()))` if the send buffer was completely flushed and acknowledged.
        /// - `Poll::Ready(Err(stream_error))` if the stream could not be flushed, because the stream
        ///   had previously entered an error state.
        /// - `Poll::Pending` if the send buffer is still being flushed. In this case, the
        ///   caller should retry sending after the `Waker` on the provided `Context` is notified.
        pub fn poll_flush(&mut self, cx: &mut Context) -> Poll<Result<(), StreamError>> {
            self.tx_request()?.flush().poll(Some(cx))?.into()
        }

        /// Marks the stream as finished and flushes the send buffer.
        ///
        /// The method will return:
        /// - `Ok(())` if the stream was finished successfully
        /// - `Err(stream_error)` if the stream could not be finished, because the stream
        ///   had previously entered an error state.
        pub fn finish(&mut self) -> Result<(), StreamError> {
            self.tx_request()?.finish().poll(None)?;
            Ok(())
        }

        /// Initiates a `RESET` on the stream.
        ///
        /// This will close the stream and notify the peer of the provided `error_code`.
        pub fn reset(&mut self, error_code: ApplicationErrorCode) -> Result<(), StreamError> {
            self.tx_request()?.reset(error_code).poll(None)?;
            Ok(())
        }
    };
}

macro_rules! rx_stream_apis {
    () => {
        /// Receives a chunk of data from the stream.
        ///
        /// The method will return:
        /// - `Poll::Ready(Ok(Some(Bytes)))` if the stream is open and data was available
        /// - `Poll::Ready(Ok(None))` if the stream was finished and all of the data was consumed
        /// - `Poll::Ready(Err(stream_error))` if the stream could not be read, because the stream
        ///   had previously entered an error state.
        /// - `Poll::Pending` if the stream is waiting to receive data from the peer. In this case, the
        ///   caller should retry sending after the `Waker` on the provided `Context` is notified.
        pub fn poll_receive(
            &mut self,
            cx: &mut Context,
        ) -> Poll<Result<Option<Bytes>, StreamError>> {
            let mut chunk = Bytes::new();
            let response =
                ready!(self.poll_receive_vectored(core::slice::from_mut(&mut chunk), cx))?;

            Ok(match response {
                // return the chunk if it was consumed
                (consumed, _) if consumed > 0 => Some(chunk),
                // any other response means the stream has ended
                _ => None,
            })
            .into()
        }

        /// Receives a slice of chunks of data from the stream.
        ///
        /// The method will return:
        /// - `Poll::Ready(Ok((len, is_open)))` if the stream received data into the slice,
        ///   where `len` was the number of chunks received, and `is_open` indicating if the stream is
        ///   still open. If `is_open == false`, future calls to `poll_receive_vectored` will
        ///   always return `Poll::Ready(Ok((0, false)))`.
        /// - `Poll::Ready(Err(stream_error))` if the stream could not be read, because the stream
        ///   had previously entered an error state.
        /// - `Poll::Pending` if the stream is waiting to receive data from the peer. In this case, the
        ///   caller should retry sending after the `Waker` on the provided `Context` is notified.
        pub fn poll_receive_vectored(
            &mut self,
            chunks: &mut [Bytes],
            cx: &mut Context,
        ) -> Poll<Result<(usize, bool), StreamError>> {
            let response = ready!(self
                .rx_request()?
                .receive(chunks)
                .poll(Some(cx))?
                .into_poll());

            let rx = response.rx().expect("invalid response");
            let consumed = rx.chunks.consumed;
            debug_assert!(
                consumed <= chunks.len(),
                "consumed exceeded the number of chunks provided"
            );
            // return if the stream is still open to receiving more data
            let is_open = rx.status.is_open() || rx.status.is_finishing();
            Poll::Ready(Ok((consumed, is_open)))
        }

        /// Sends a `STOP_SENDING` message to the peer. This requests the peer to
        /// finish the `Stream` as soon as possible by issuing a `RESET` with the
        /// provided `error_code`.
        ///
        /// Since this is merely a request to the peer to `RESET` the `Stream`, the
        /// `Stream` will not immediately be in a `RESET` state after issuing this
        /// API call.
        ///
        /// If the `Stream` had been previously reset by the peer or if all data had
        /// already been received the API call will not trigger any action.
        pub fn stop_sending(
            &mut self,
            error_code: ApplicationErrorCode,
        ) -> Result<(), StreamError> {
            self.rx_request()?.stop_sending(error_code).poll(None)?;
            Ok(())
        }
    };
}

/// A readable and writeable QUIC stream
pub struct Stream(State);

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let is_alternate = f.alternate();

        let mut s = f.debug_struct("Stream");
        s.field("id", &self.id());

        // return additional information
        if is_alternate {
            // TODO query the shared state
        }

        s.finish()
    }
}

impl Stream {
    /// Creates a `Stream` instance, which represents a QUIC stream with the
    /// given ID. All interactions with the `Stream` will be performed through
    /// the provided [`SynchronizedSharedConnectionState`].
    pub(crate) fn new(shared_state: ConnectionApi, stream_id: StreamId) -> Self {
        Self(State::new(shared_state, stream_id))
    }

    pub fn id(&self) -> StreamId {
        self.0.stream_id
    }

    pub fn request(&mut self) -> Request {
        self.0.request()
    }

    pub fn tx_request(&mut self) -> Result<TxRequest, StreamError> {
        Ok(TxRequest {
            state: &mut self.0,
            request: ops::Request::default(),
        })
    }

    pub fn rx_request(&mut self) -> Result<RxRequest, StreamError> {
        Ok(RxRequest {
            state: &mut self.0,
            request: ops::Request::default(),
        })
    }

    tx_stream_apis!();
    rx_stream_apis!();

    /// Splits a bidirectional QUIC Stream in two halves.
    ///
    /// One half can be used to read data from the Stream.
    /// The other half can be used to send data.
    pub fn split(self) -> (ReceiveStream, SendStream) {
        let mut rx_state = self.0;
        let mut tx_state = rx_state.clone();

        // close the opposite sides
        rx_state.is_tx_open = false;
        tx_state.is_rx_open = false;

        (ReceiveStream(rx_state), SendStream(tx_state))
    }
}

/// A writeable QUIC stream
pub struct SendStream(State);

impl fmt::Debug for SendStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let is_alternate = f.alternate();

        let mut s = f.debug_struct("SendStream");
        s.field("id", &self.id());

        // return additional information
        if is_alternate {
            // TODO query the shared state
        }

        s.finish()
    }
}

impl SendStream {
    pub fn id(&self) -> StreamId {
        self.0.stream_id
    }

    pub fn tx_request(&mut self) -> Result<TxRequest, StreamError> {
        Ok(TxRequest {
            state: &mut self.0,
            request: ops::Request::default(),
        })
    }

    tx_stream_apis!();
}

impl From<Stream> for SendStream {
    fn from(stream: Stream) -> Self {
        Self(stream.0)
    }
}

/// A readable QUIC stream
pub struct ReceiveStream(State);

impl fmt::Debug for ReceiveStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let is_alternate = f.alternate();

        let mut s = f.debug_struct("ReceiveStream");
        s.field("id", &self.id());

        // return additional information
        if is_alternate {
            // TODO query the shared state
        }

        s.finish()
    }
}

impl ReceiveStream {
    pub fn id(&self) -> StreamId {
        self.0.stream_id
    }

    pub fn rx_request(&mut self) -> Result<RxRequest, StreamError> {
        Ok(RxRequest {
            state: &mut self.0,
            request: ops::Request::default(),
        })
    }

    rx_stream_apis!();
}

impl From<Stream> for ReceiveStream {
    fn from(stream: Stream) -> Self {
        Self(stream.0)
    }
}

macro_rules! tx_request_apis {
    () => {
        pub fn send(&mut self, chunks: &'chunks mut [Bytes]) -> &mut Self {
            self.request.send(chunks);
            self
        }

        pub fn send_readiness(&mut self) -> &mut Self {
            // express interest in tx
            if self.request.tx.is_none() {
                self.request.tx = Some(Default::default());
            }
            self
        }

        pub fn finish(&mut self) -> &mut Self {
            self.request.finish();
            self
        }

        pub fn reset(&mut self, error_code: ApplicationErrorCode) -> &mut Self {
            self.request.reset(error_code);
            self
        }

        pub fn flush(&mut self) -> &mut Self {
            self.request.flush();
            self
        }
    };
}

macro_rules! rx_request_apis {
    () => {
        pub fn receive(&mut self, chunks: &'chunks mut [Bytes]) -> &mut Self {
            self.request.receive(chunks);
            self
        }

        pub fn with_watermark(&mut self, low: usize, high: usize) -> &mut Self {
            self.request.with_watermark(low, high);
            self
        }

        pub fn with_low_watermark(&mut self, low: usize) -> &mut Self {
            self.request.with_low_watermark(low);
            self
        }

        pub fn with_high_watermark(&mut self, high: usize) -> &mut Self {
            self.request.with_high_watermark(high);
            self
        }

        pub fn stop_sending(&mut self, error_code: ApplicationErrorCode) -> &mut Self {
            self.request.stop_sending(error_code);
            self
        }
    };
}

pub struct Request<'state, 'chunks> {
    state: &'state mut State,
    request: ops::Request<'chunks>,
}

impl<'state, 'chunks> Request<'state, 'chunks> {
    tx_request_apis!();
    rx_request_apis!();

    pub fn poll(&mut self, context: Option<&Context>) -> Result<ops::Response, StreamError> {
        let response = self.state.poll_request(&mut self.request, context)?;

        if let Some(rx) = response.rx() {
            self.state.is_rx_open = rx.is_open();
        }

        if let Some(tx) = response.tx() {
            self.state.is_tx_open = tx.is_open();
        }

        Ok(response)
    }
}

impl<'state, 'chunks> Future for Request<'state, 'chunks> {
    type Output = Result<ops::Response, StreamError>;

    fn poll(
        mut self: Pin<&mut Self>,
        context: &mut Context,
    ) -> Poll<Result<ops::Response, StreamError>> {
        Self::poll(&mut self, Some(context))?.into()
    }
}

pub struct TxRequest<'state, 'chunks> {
    state: &'state mut State,
    request: ops::Request<'chunks>,
}

impl<'state, 'chunks> TxRequest<'state, 'chunks> {
    tx_request_apis!();

    pub fn poll(&mut self, context: Option<&Context>) -> Result<ops::tx::Response, StreamError> {
        let response = self
            .state
            .poll_request(&mut self.request, context)?
            .tx
            .expect("invalid response");

        self.state.is_tx_open = response.is_open();

        Ok(response)
    }
}

impl<'state, 'chunks> Future for TxRequest<'state, 'chunks> {
    type Output = Result<ops::tx::Response, StreamError>;

    fn poll(
        mut self: Pin<&mut Self>,
        context: &mut Context,
    ) -> Poll<Result<ops::tx::Response, StreamError>> {
        Self::poll(&mut self, Some(context))?.into()
    }
}

pub struct RxRequest<'state, 'chunks> {
    state: &'state mut State,
    request: ops::Request<'chunks>,
}

impl<'state, 'chunks> RxRequest<'state, 'chunks> {
    rx_request_apis!();

    pub fn poll(&mut self, context: Option<&Context>) -> Result<ops::rx::Response, StreamError> {
        let response = self
            .state
            .poll_request(&mut self.request, context)?
            .rx
            .expect("invalid response");

        self.state.is_rx_open = response.is_open();

        Ok(response)
    }
}

impl<'state, 'chunks> Future for RxRequest<'state, 'chunks> {
    type Output = Result<ops::rx::Response, StreamError>;

    fn poll(
        mut self: Pin<&mut Self>,
        context: &mut Context,
    ) -> Poll<Result<ops::rx::Response, StreamError>> {
        Self::poll(&mut self, Some(context))?.into()
    }
}
