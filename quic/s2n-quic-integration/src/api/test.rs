//! API implementation used to test the integration framework itself

use crate::api;
use bytes::Bytes;
use core::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};
use futures::{ready, stream::Stream};

#[derive(Clone, Copy, Debug)]
pub enum Error {
    Closed,
    Reset(u64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for Error {}

impl<T> From<flume::SendError<T>> for Error {
    fn from(_error: flume::SendError<T>) -> Self {
        Self::Closed
    }
}

#[derive(Clone)]
pub struct Connection {
    handle: Handle,
    acceptor: Acceptor,
}

impl Connection {
    pub fn pair() -> (Self, Self) {
        let (handle_a, acceptor_a) = Self::side();
        let (handle_b, acceptor_b) = Self::side();
        let a = Self {
            handle: handle_a,
            acceptor: acceptor_b,
        };
        let b = Self {
            handle: handle_b,
            acceptor: acceptor_a,
        };
        (a, b)
    }

    fn side() -> (Handle, Acceptor) {
        let (send_recv, recv_recv) = flume::unbounded();
        let (send_bidi, recv_bidi) = flume::unbounded();

        let handle = Handle::new(send_bidi, send_recv);
        let acceptor = Acceptor {
            receive: ReceiveStreamAcceptor {
                streams: recv_recv.into_stream(),
            },
            bidi: BidirectionalStreamAcceptor {
                streams: recv_bidi.into_stream(),
            },
        };
        (handle, acceptor)
    }

    pub fn split_all(self) -> (Handle, BidirectionalStreamAcceptor, ReceiveStreamAcceptor) {
        use api::*;
        let (handle, acceptor) = self.split();
        let (bidi, recv) = acceptor.split();
        (handle, bidi, recv)
    }
}

impl api::Connection for Connection {
    type Acceptor = Acceptor;
    type Handle = Handle;

    fn split(self) -> (Self::Handle, Self::Acceptor) {
        let Self { handle, acceptor } = self;
        (handle, acceptor)
    }
}

#[derive(Clone)]
pub struct Acceptor {
    receive: ReceiveStreamAcceptor,
    bidi: BidirectionalStreamAcceptor,
}

impl api::Acceptor for Acceptor {
    type ReceiveStreamAcceptor = ReceiveStreamAcceptor;
    type BidirectionalStreamAcceptor = BidirectionalStreamAcceptor;

    fn split(
        self,
    ) -> (
        Self::BidirectionalStreamAcceptor,
        Self::ReceiveStreamAcceptor,
    ) {
        let Self { bidi, receive } = self;
        (bidi, receive)
    }
}

#[derive(Debug)]
enum AcceptorMessage<Stream> {
    Stream(Stream),
    Close,
    Error(Error),
}

#[derive(Clone)]
pub struct ReceiveStreamAcceptor {
    streams: flume::r#async::RecvStream<'static, AcceptorMessage<ReceiveStream>>,
}

impl fmt::Debug for ReceiveStreamAcceptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ReceiveStreamAcceptor")
            .field("pending", &self.streams.size_hint().0)
            .finish()
    }
}

impl api::ReceiveStreamAcceptor for ReceiveStreamAcceptor {
    type ReceiveStream = ReceiveStream;
    type Error = Error;

    fn poll_accept_receive(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::ReceiveStream>, Self::Error>> {
        let msg = ready!(Pin::new(&mut self.streams).poll_next(cx));

        match msg {
            Some(AcceptorMessage::Stream(stream)) => Ok(Some(stream)),
            Some(AcceptorMessage::Error(err)) => Err(err),
            Some(AcceptorMessage::Close) | None => Ok(None),
        }
        .into()
    }
}

#[derive(Clone)]
pub struct BidirectionalStreamAcceptor {
    streams: flume::r#async::RecvStream<'static, AcceptorMessage<BidirectionalStream>>,
}

impl fmt::Debug for BidirectionalStreamAcceptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BidirectionalStreamAcceptor")
            .field("pending", &self.streams.size_hint().0)
            .finish()
    }
}

impl api::BidirectionalStreamAcceptor for BidirectionalStreamAcceptor {
    type BidirectionalStream = BidirectionalStream;
    type Error = Error;

    fn poll_accept_bidirectional(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::BidirectionalStream>, Self::Error>> {
        let msg = ready!(Pin::new(&mut self.streams).poll_next(cx));

        match msg {
            Some(AcceptorMessage::Stream(stream)) => Ok(Some(stream)),
            Some(AcceptorMessage::Error(err)) => Err(err),
            Some(AcceptorMessage::Close) | None => Ok(None),
        }
        .into()
    }
}

#[derive(Clone)]
pub struct Handle {
    recv_streams: flume::Sender<AcceptorMessage<ReceiveStream>>,
    bidi_streams: flume::Sender<AcceptorMessage<BidirectionalStream>>,
}

impl Handle {
    fn new(
        bidi_streams: flume::Sender<AcceptorMessage<BidirectionalStream>>,
        recv_streams: flume::Sender<AcceptorMessage<ReceiveStream>>,
    ) -> Self {
        Self {
            bidi_streams,
            recv_streams,
        }
    }

    #[allow(dead_code)]
    pub fn close(&mut self, error: Error) {
        let _ = self.recv_streams.send(AcceptorMessage::Error(error));
        let _ = self.bidi_streams.send(AcceptorMessage::Error(error));
    }
}

impl api::Handle for Handle {
    type SendStream = SendStream;
    type BidirectionalStream = BidirectionalStream;
    type Error = Error;

    fn poll_open_send(&mut self, _cx: &mut Context) -> Poll<Result<Self::SendStream, Self::Error>> {
        let (send, recv) = pair();

        self.recv_streams.send(AcceptorMessage::Stream(recv))?;

        Ok(send).into()
    }

    fn poll_open_bidirectional(
        &mut self,
        _cx: &mut Context,
    ) -> Poll<Result<Self::BidirectionalStream, Self::Error>> {
        let (send_a, recv_a) = pair();
        let (send_b, recv_b) = pair();

        let a = BidirectionalStream {
            send: send_a,
            recv: recv_b,
        };
        let b = BidirectionalStream {
            send: send_b,
            recv: recv_a,
        };

        self.bidi_streams.send(AcceptorMessage::Stream(a))?;

        Ok(b).into()
    }

    fn close(&mut self) {
        let _ = self.recv_streams.send(AcceptorMessage::Close);
        let _ = self.bidi_streams.send(AcceptorMessage::Close);
    }
}

#[derive(Debug)]
enum StreamMessage {
    Data(Bytes),
    Finish,
    Reset(u64),
}

pub struct SendStream {
    stream: flume::Sender<StreamMessage>,
}

impl api::SendStream for SendStream {
    type Error = Error;

    fn poll_send(&mut self, data: &mut Bytes, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let data = core::mem::replace(data, Bytes::new());
        self.stream.send(StreamMessage::Data(data))?;
        Ok(()).into()
    }

    fn poll_finish(&mut self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.stream.send(StreamMessage::Finish)?;
        Ok(()).into()
    }

    fn reset(&mut self, code: u64) {
        let _ = self.stream.send(StreamMessage::Reset(code));
    }
}

pub struct ReceiveStream {
    stream: flume::r#async::RecvStream<'static, StreamMessage>,
}

impl api::ReceiveStream for ReceiveStream {
    type Error = Error;

    fn poll_receive(&mut self, cx: &mut Context) -> Poll<Result<Option<Bytes>, Self::Error>> {
        let message =
            ready!(Stream::poll_next(Pin::new(&mut self.stream), cx)).ok_or(Error::Closed)?;

        match message {
            StreamMessage::Data(chunk) => Ok(Some(chunk)),
            StreamMessage::Finish => Ok(None),
            StreamMessage::Reset(code) => Err(Error::Reset(code)),
        }
        .into()
    }

    fn stop_sending(&mut self, _code: u64) {
        todo!()
    }
}

pub struct BidirectionalStream {
    send: SendStream,
    recv: ReceiveStream,
}

impl api::BidirectionalStream for BidirectionalStream {
    type SendStream = SendStream;
    type ReceiveStream = ReceiveStream;

    fn split(self) -> (Self::ReceiveStream, Self::SendStream) {
        let Self { send, recv } = self;
        (recv, send)
    }
}

fn pair() -> (SendStream, ReceiveStream) {
    let (send, recv) = flume::unbounded();

    let send = SendStream { stream: send };

    let recv = ReceiveStream {
        stream: recv.into_stream(),
    };

    (send, recv)
}

#[cfg(test)]
mod tests {
    use crate::{api::*, rt::spawn};

    #[tokio::test]
    async fn accept_test() {
        let (client, server) = super::Connection::pair();

        let (mut client_handle, _client_bidi_acceptor, _client_recv_acceptor) = client.split_all();
        let (_server_handle, mut server_bidi_acceptor, mut server_recv_acceptor) =
            server.split_all();

        let server_recv_task = spawn(async move { server_recv_acceptor.accept_receive().await });

        let mut client_send = client_handle.open_send().await.unwrap();

        let mut server_recv = server_recv_task.await.unwrap().unwrap().unwrap();

        client_send
            .send(Bytes::from_static(&[1, 2, 3]))
            .await
            .unwrap();

        assert_eq!(
            server_recv.receive().await.unwrap().unwrap(),
            Bytes::from_static(&[1, 2, 3])
        );

        let server_bidi_task =
            spawn(async move { server_bidi_acceptor.accept_bidirectional().await });

        let _client_bidi = client_handle.open_bidirectional().await.unwrap();

        server_bidi_task.await.unwrap().unwrap();
    }
}
