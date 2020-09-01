use crate::inet::{ExplicitCongestionNotification, SocketAddress};
use core::{fmt, marker::PhantomData};

/// A structure capable of queueing and receiving messages
pub trait Rx<'a>: Sized {
    type Queue: Queue;
    type Error: fmt::Display;

    /// Returns the reception queue
    fn queue(&'a mut self) -> Self::Queue;

    /// Returns number of items in the queue
    fn len(&self) -> usize;

    /// Returns true if the queue is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a future that receives messages into the queue and returns the number of messages
    /// received.
    fn receive(&mut self) -> Receive<'a, '_, Self> {
        Receive {
            rx: self,
            l: PhantomData,
        }
    }

    /// Polls receiving messages into the queue and returns the number of messages received.
    fn poll_receive(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<usize, Self::Error>>;
}

/// A Future for receiving data
pub struct Receive<'a, 'r, R: Rx<'a>> {
    /// Reference to the Rx implementation
    rx: &'r mut R,
    /// Stores the lifetime for the Rx trait parameter
    l: PhantomData<&'a ()>,
}

impl<'a, 'r, R: Rx<'a>> core::future::Future for Receive<'a, 'r, R> {
    type Output = Result<usize, R::Error>;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        self.rx.poll_receive(cx)
    }
}

/// A first-in, first-out queue of messages to be received
pub trait Queue {
    type Entry: Entry;

    /// Returns a slice of all of the entries in the queue
    fn as_slice_mut(&mut self) -> &mut [Self::Entry];

    /// Consumes `count` number of entries in the queue
    fn finish(&mut self, count: usize);
}

/// An entry in a Rx queue
pub trait Entry {
    /// Returns the remote address
    fn remote_address(&self) -> Option<SocketAddress>;

    /// Returns the ECN markings
    fn ecn(&self) -> ExplicitCongestionNotification;

    /// Returns the payload
    fn payload(&self) -> &[u8];

    /// Returns the length of the payload
    fn payload_len(&self) -> usize {
        self.payload().len()
    }

    /// Returns a mutable payload
    fn payload_mut(&mut self) -> &mut [u8];
}
