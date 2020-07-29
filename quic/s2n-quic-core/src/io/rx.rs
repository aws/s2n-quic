use crate::inet::{ExplicitCongestionNotification, SocketAddress};
use core::fmt;

/// A structure capable of queueing and receiving messages
pub trait Rx<'a> {
    type Queue: Queue;
    type Error: fmt::Display;

    /// Returns the reception queue
    fn queue(&'a mut self) -> Self::Queue;

    /// Receives messages into the queue and returns the number
    /// of messages received.
    fn receive(&mut self) -> Result<usize, Self::Error>;
}

/// A first-in, first-out queue of messages to be received
pub trait Queue {
    type Entry: Entry;

    /// Returns a single entry in the queue
    fn pop(&mut self) -> Option<&mut Self::Entry>;

    /// Consumes and returns all of the entries in the queue
    fn take_all(&mut self) -> &mut [Self::Entry];
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
