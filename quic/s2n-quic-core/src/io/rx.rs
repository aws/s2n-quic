// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event, inet::datagram, path};
use core::task::{Context, Poll};

pub mod pair;

/// Handle to a receive IO provider
pub trait Rx: Sized {
    type PathHandle;
    // TODO make this generic over lifetime
    // See https://github.com/aws/s2n-quic/issues/1742
    type Queue: Queue<Handle = Self::PathHandle>;
    type Error: Send;

    /// Returns a future that yields after a packet is ready to be received
    #[inline]
    fn ready(&mut self) -> RxReady<Self> {
        RxReady(self)
    }

    /// Polls the IO provider for a packet that is ready to be received
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>>;

    /// Calls the provided callback with the IO provider queue
    fn queue<F: FnOnce(&mut Self::Queue)>(&mut self, f: F);

    /// Handles the queue error and potentially publishes an event
    fn handle_error<E: event::EndpointPublisher>(self, error: Self::Error, event: &mut E);
}

impl_ready_future!(Rx, RxReady, Result<(), T::Error>);

/// Extension traits for Rx channels
pub trait RxExt: Rx {
    /// Creates single channel from a pair of channels
    #[inline]
    fn with_pair<Other>(self, other: Other) -> pair::Channel<Self, Other>
    where
        Other: Rx<PathHandle = Self::PathHandle>,
    {
        pair::Channel { a: self, b: other }
    }
}

/// Implement the extension traits for all Rx queues
impl<R: Rx> RxExt for R {}

/// A structure capable of queueing and receiving messages
pub trait Queue {
    type Handle: path::Handle;

    /// Iterates over all of the packets in the receive queue and processes them
    fn for_each<F: FnMut(datagram::Header<Self::Handle>, &mut [u8])>(&mut self, on_packet: F);

    /// Returns if there are items in the queue or not
    fn is_empty(&self) -> bool;
}
