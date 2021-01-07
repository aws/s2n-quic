//! Abstractions over a QUIC connection and streams

use bytes::Bytes;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

#[cfg(test)]
pub(crate) mod test;

pub trait Connection {
    type Acceptor: Acceptor;
    type Handle: Handle;

    fn split(self) -> (Self::Handle, Self::Acceptor);
}

pub trait Acceptor: Sized {
    type ReceiveStreamAcceptor: ReceiveStreamAcceptor;
    type BidirectionalStreamAcceptor: BidirectionalStreamAcceptor;

    fn split(
        self,
    ) -> (
        Self::BidirectionalStreamAcceptor,
        Self::ReceiveStreamAcceptor,
    );
}

pub trait ReceiveStreamAcceptor: Sized + Send {
    type ReceiveStream: ReceiveStream;
    type Error: Error;

    fn poll_accept_receive(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::ReceiveStream>, Self::Error>>;

    fn accept_receive(&mut self) -> AcceptReceiveFuture<Self> {
        AcceptReceiveFuture(self)
    }
}

pub trait BidirectionalStreamAcceptor: Sized + Send {
    type BidirectionalStream: BidirectionalStream;
    type Error: Error;

    fn poll_accept_bidirectional(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::BidirectionalStream>, Self::Error>>;

    fn accept_bidirectional(&mut self) -> AcceptBidirectionalFuture<Self> {
        AcceptBidirectionalFuture(self)
    }
}

macro_rules! accept_future {
    ($name:ident, $handle:ident, $ty:ident, $call:ident) => {
        #[must_use]
        pub struct $name<'a, H: $handle>(&'a mut H);

        impl<'a, H: $handle> Future for $name<'a, H> {
            type Output = Result<Option<H::$ty>, H::Error>;

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
                (self.0).$call(cx)
            }
        }
    };
}

accept_future!(
    AcceptReceiveFuture,
    ReceiveStreamAcceptor,
    ReceiveStream,
    poll_accept_receive
);

accept_future!(
    AcceptBidirectionalFuture,
    BidirectionalStreamAcceptor,
    BidirectionalStream,
    poll_accept_bidirectional
);

pub trait Handle: Clone + Sized + Send {
    type SendStream: SendStream;
    type BidirectionalStream: BidirectionalStream;
    type Error: Error;

    fn poll_open_send(&mut self, cx: &mut Context) -> Poll<Result<Self::SendStream, Self::Error>>;

    fn open_send(&mut self) -> OpenSendFuture<Self> {
        OpenSendFuture(self)
    }

    fn poll_open_bidirectional(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Self::BidirectionalStream, Self::Error>>;

    fn open_bidirectional(&mut self) -> OpenBidirectionalFuture<Self> {
        OpenBidirectionalFuture(self)
    }

    fn close(&mut self);
}

macro_rules! open_future {
    ($name:ident, $ty:ident, $call:ident) => {
        #[must_use]
        pub struct $name<'a, H: Handle>(&'a mut H);

        impl<'a, H: Handle> Future for $name<'a, H> {
            type Output = Result<H::$ty, H::Error>;

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
                (self.0).$call(cx)
            }
        }
    };
}

open_future!(OpenSendFuture, SendStream, poll_open_send);
open_future!(
    OpenBidirectionalFuture,
    BidirectionalStream,
    poll_open_bidirectional
);

pub trait SendStream: Sized + Send {
    type Error: Error;

    fn send(&mut self, data: Bytes) -> SendFuture<Self> {
        SendFuture(self, data)
    }

    fn poll_send(&mut self, data: &mut Bytes, cx: &mut Context) -> Poll<Result<(), Self::Error>>;

    fn finish(&mut self) -> FinishFuture<Self> {
        FinishFuture(self)
    }

    fn poll_finish(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>>;

    fn reset(&mut self, code: u64);
}

#[must_use]
pub struct SendFuture<'a, S: SendStream>(&'a mut S, Bytes);

impl<'a, S: SendStream> Future for SendFuture<'a, S> {
    type Output = Result<(), S::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let Self(a, b) = &mut *self;
        a.poll_send(b, cx)
    }
}

#[must_use]
pub struct FinishFuture<'a, S: SendStream>(&'a mut S);

impl<'a, S: SendStream> Future for FinishFuture<'a, S> {
    type Output = Result<(), S::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.0.poll_finish(cx)
    }
}

pub trait ReceiveStream: Sized + Send {
    type Error: Error;

    fn receive(&mut self) -> ReceiveFuture<Self> {
        ReceiveFuture(self)
    }

    fn poll_receive(&mut self, cx: &mut Context) -> Poll<Result<Option<Bytes>, Self::Error>>;

    fn stop_sending(&mut self, error_code: u64);
}

#[must_use]
pub struct ReceiveFuture<'a, S: ReceiveStream>(&'a mut S);

impl<'a, S: ReceiveStream> Future for ReceiveFuture<'a, S> {
    type Output = Result<Option<Bytes>, S::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.0.poll_receive(cx)
    }
}

pub trait BidirectionalStream: Send {
    type SendStream: SendStream;
    type ReceiveStream: ReceiveStream;

    fn split(self) -> (Self::ReceiveStream, Self::SendStream);
}

pub trait Error: 'static + Send + Sync + std::error::Error {}

impl<T: 'static + Send + Sync + std::error::Error> Error for T {}
