use crate::api;
use bytes::Bytes;
use core::{
    convert::TryInto,
    task::{Context, Poll},
};
use s2n_quic::{
    connection::{self, Handle, StreamAcceptor},
    stream, Connection,
};

impl api::Connection for Connection {
    type Acceptor = StreamAcceptor;
    type Handle = Handle;

    fn split(self) -> (Self::Handle, Self::Acceptor) {
        Connection::split(self)
    }
}

impl api::Acceptor for StreamAcceptor {
    type ReceiveStreamAcceptor = connection::ReceiveStreamAcceptor;
    type BidirectionalStreamAcceptor = connection::BidirectionalStreamAcceptor;

    fn split(
        self,
    ) -> (
        Self::BidirectionalStreamAcceptor,
        Self::ReceiveStreamAcceptor,
    ) {
        Self::split(self)
    }
}

impl api::ReceiveStreamAcceptor for connection::ReceiveStreamAcceptor {
    type ReceiveStream = stream::ReceiveStream;
    type Error = connection::Error;

    fn poll_accept_receive(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::ReceiveStream>, Self::Error>> {
        Self::poll_accept_receive_stream(self, cx)
    }
}

impl api::BidirectionalStreamAcceptor for connection::BidirectionalStreamAcceptor {
    type BidirectionalStream = stream::BidirectionalStream;
    type Error = connection::Error;

    fn poll_accept_bidirectional(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::BidirectionalStream>, Self::Error>> {
        Self::poll_accept_bidirectional_stream(self, cx)
    }
}

impl crate::api::Handle for Handle {
    type BidirectionalStream = stream::BidirectionalStream;
    type SendStream = stream::SendStream;
    type Error = connection::Error;

    fn poll_open_send(&mut self, cx: &mut Context) -> Poll<Result<Self::SendStream, Self::Error>> {
        Handle::poll_open_send_stream(self, cx)
    }

    fn poll_open_bidirectional(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Self::BidirectionalStream, Self::Error>> {
        Handle::poll_open_bidirectional_stream(self, cx)
    }

    fn close(&mut self) {
        // TODO
    }
}

macro_rules! send_stream {
    ($ty:ident) => {
        impl api::SendStream for stream::$ty {
            type Error = stream::Error;

            fn poll_send(
                &mut self,
                chunk: &mut Bytes,
                cx: &mut Context,
            ) -> Poll<Result<(), Self::Error>> {
                stream::$ty::poll_send(self, chunk, cx)
            }

            fn poll_finish(&mut self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
                stream::$ty::finish(self).into()
            }

            fn reset(&mut self, code: u64) {
                let _ = stream::$ty::reset(self, code.try_into().unwrap());
            }
        }
    };
}

send_stream!(SendStream);
send_stream!(BidirectionalStream);

macro_rules! receive_stream {
    ($ty:ident) => {
        impl api::ReceiveStream for stream::$ty {
            type Error = stream::Error;

            fn poll_receive(
                &mut self,
                cx: &mut Context,
            ) -> Poll<Result<Option<Bytes>, Self::Error>> {
                stream::$ty::poll_receive(self, cx)
            }

            fn stop_sending(&mut self, code: u64) {
                let _ = stream::$ty::stop_sending(self, code.try_into().unwrap());
            }
        }
    };
}

receive_stream!(ReceiveStream);
receive_stream!(BidirectionalStream);

impl api::BidirectionalStream for stream::BidirectionalStream {
    type SendStream = stream::SendStream;
    type ReceiveStream = stream::ReceiveStream;

    fn split(self) -> (Self::ReceiveStream, Self::SendStream) {
        stream::BidirectionalStream::split(self)
    }
}
