use crate::inet::{ExplicitCongestionNotification, SocketAddress};
use core::{fmt, time::Duration};

/// A structure capable of queueing and transmitting messages
pub trait Tx<'a> {
    type Queue: Queue;
    type Error: fmt::Display;

    /// Set to true if the queue supports setting ECN markings
    const SUPPORTS_ECN: bool = false;

    /// Set to true if the queue supports pacing of sending messages
    const SUPPORTS_PACING: bool = false;

    /// Set to true if the queue supports setting IPv6 flow labels
    const SUPPORTS_FLOW_LABELS: bool = false;

    /// Returns the transmission queue
    fn queue(&'a mut self) -> Self::Queue;

    /// Returns number of items in the queue
    fn len(&self) -> usize;

    /// Returns true if the queue is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Transmits messages from the queue and returns the number
    /// of messages sent.
    fn transmit(&mut self) -> Result<usize, Self::Error>;
}

/// A first-in, first-out queue of messages to be transmitted
pub trait Queue {
    type Entry: Entry;

    /// Pushes a message into the transmission queue
    ///
    /// The index of the message is returned to enable further operations to be
    /// performed, e.g. encryption.
    fn push<M: Message>(&mut self, message: M) -> Result<usize, Error>;

    /// Returns the pending messages as a mutable slice
    fn as_slice_mut(&mut self) -> &mut [Self::Entry];

    /// Returns the number of remaining datagrams that can be transmitted
    fn capacity(&self) -> usize;

    /// Returns `true` if the queue will accept additional transmissions
    fn has_capacity(&self) -> bool {
        self.capacity() != 0
    }

    /// Returns the number of pending datagrams to be transmitted
    fn len(&self) -> usize;

    /// Returns `true` if there are no pending datagrams to be transmitted
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq)]
pub enum Error {
    /// The provided message did not write a payload
    EmptyPayload,

    /// The transmission queue is at capacity
    AtCapacity,
}

/// An entry in a Tx queue
pub trait Entry {
    /// Sets the message for the given entry
    fn set<M: Message>(&mut self, message: M) -> Result<usize, Error>;

    /// Returns the transmission payload as a slice of bytes
    fn payload(&self) -> &[u8];

    /// Returns the transmission payload as a mutable slice of bytes
    fn payload_mut(&mut self) -> &mut [u8];
}

/// Abstraction over a message to be sent on a socket
///
/// Instead of a concrete struct with eagerly evaluted fields,
/// using trait callbacks ensure messages only need to compute what
/// the actual transmission queue requires. For example, if the transmission
/// queue cannot set ECN markings, it will not call the [`ecn`] function.
pub trait Message {
    /// Returns the target remote address for the message
    fn remote_address(&mut self) -> SocketAddress;

    /// Returns the ECN markings for the message
    fn ecn(&mut self) -> ExplicitCongestionNotification;

    /// Returns the Duration for which the message will be delayed.
    ///
    /// This is used in scenarios where packets need to be paced.
    fn delay(&mut self) -> Duration;

    /// Returns the IPv6 flow label for the message
    fn ipv6_flow_label(&mut self) -> u32;

    /// Writes the payload of the message to an output buffer
    fn write_payload(&mut self, buffer: &mut [u8]) -> usize;
}
