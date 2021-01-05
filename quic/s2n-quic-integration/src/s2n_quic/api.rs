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
    type BidiStream = stream::BidirectionalStream;
    type ReceiveStream = stream::ReceiveStream;
    type Error = connection::Error;

    fn poll_accept_bidi(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::BidiStream>, Self::Error>> {
        StreamAcceptor::poll_accept_bidirectional_stream(self, cx)
    }

    fn poll_accept_receive(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::ReceiveStream>, Self::Error>> {
        StreamAcceptor::poll_accept_receive_stream(self, cx)
    }
}

impl crate::api::Handle for Handle {
    type BidiStream = stream::BidirectionalStream;
    type SendStream = stream::SendStream;
    type Error = connection::Error;

    fn poll_open_send(&mut self, cx: &mut Context) -> Poll<Result<Self::SendStream, Self::Error>> {
        Handle::poll_open_send_stream(self, cx)
    }

    fn poll_open_bidi(&mut self, cx: &mut Context) -> Poll<Result<Self::BidiStream, Self::Error>> {
        Handle::poll_open_bidirectional_stream(self, cx)
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
