// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::recv;
use crate::stream::socket::Socket;
use crate::{event, stream::TransportFeatures};
use core::task::{Context, Poll};
use std::io;

pub mod channel;
mod dispatch;
mod local;

pub use channel::Channel;
pub use dispatch::Dispatch;
pub use local::Local;

pub trait Buffer {
    fn is_empty(&self) -> bool;

    fn poll_fill<S, Pub>(
        &mut self,
        cx: &mut Context,
        socket: &S,
        publisher: &mut Pub,
    ) -> Poll<io::Result<usize>>
    where
        S: ?Sized + Socket,
        Pub: event::ConnectionPublisher;

    fn process<R>(
        &mut self,
        features: TransportFeatures,
        router: &mut R,
    ) -> Result<(), recv::Error>
    where
        R: Dispatch;
}

pub enum Either<A, B> {
    A(A),
    B(B),
}

impl<A, B> Buffer for Either<A, B>
where
    A: Buffer,
    B: Buffer,
{
    #[inline]
    fn is_empty(&self) -> bool {
        match self {
            Self::A(a) => a.is_empty(),
            Self::B(b) => b.is_empty(),
        }
    }

    #[inline]
    fn poll_fill<S, Pub>(
        &mut self,
        cx: &mut Context,
        socket: &S,
        publisher: &mut Pub,
    ) -> Poll<io::Result<usize>>
    where
        S: ?Sized + Socket,
        Pub: event::ConnectionPublisher,
    {
        match self {
            Self::A(a) => a.poll_fill(cx, socket, publisher),
            Self::B(b) => b.poll_fill(cx, socket, publisher),
        }
    }

    #[inline]
    fn process<R>(&mut self, features: TransportFeatures, router: &mut R) -> Result<(), recv::Error>
    where
        R: Dispatch,
    {
        match self {
            Self::A(a) => a.process(features, router),
            Self::B(b) => b.process(features, router),
        }
    }
}
