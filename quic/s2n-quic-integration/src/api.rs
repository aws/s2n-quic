//! Abstractions over a QUIC connection and streams

use bytes::Bytes;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub trait Connection {
    type Acceptor: Acceptor;
    type Handle: Handle;

    fn split(self) -> (Self::Handle, Self::Acceptor);
}

pub trait Acceptor: Sized {
    type ReceiveStream: ReceiveStream;
    type BidiStream: SendStream + ReceiveStream;
    type Error;

    fn poll_accept_bidi(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::BidiStream>, Self::Error>>;

    fn poll_accept_receive(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Option<Self::ReceiveStream>, Self::Error>>;
}

pub trait Handle: Clone + Sized {
    type SendStream: SendStream;
    type BidiStream: SendStream + ReceiveStream;
    type Error;

    fn poll_open_send(&mut self, cx: &mut Context) -> Poll<Result<Self::SendStream, Self::Error>>;

    fn open_send(&mut self) -> OpenSendFuture<Self> {
        OpenSendFuture(self)
    }

    fn poll_open_bidi(&mut self, cx: &mut Context) -> Poll<Result<Self::BidiStream, Self::Error>>;

    fn open_bidi(&mut self) -> OpenBidiFuture<Self> {
        OpenBidiFuture(self)
    }
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
open_future!(OpenBidiFuture, BidiStream, poll_open_bidi);

pub trait SendStream: Sized {
    type Error;

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

pub trait ReceiveStream: Sized {
    type Error;

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
