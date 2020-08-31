use crate::inet::{ExplicitCongestionNotification, SocketAddress};
use core::{fmt, marker::PhantomData, time::Duration};

/// A structure capable of queueing and transmitting messages
pub trait Tx<'a>: Sized {
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

    /// Returns a future that transmits messages from the queue and returns the number of messages
    /// sent.
    fn transmit(&mut self) -> Transmit<'a, '_, Self> {
        Transmit {
            tx: self,
            l: PhantomData,
        }
    }

    /// Polls transmitting messages from the queue and returns the number of messages sent.
    fn poll_transmit(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<usize, Self::Error>>;
}

#[pin_project::pin_project]
pub struct Transmit<'a, 't, T: Tx<'a>> {
    #[pin]
    tx: &'t mut T,
    l: PhantomData<&'a ()>,
}

impl<'a, 't, T: Tx<'a>> core::future::Future for Transmit<'a, 't, T> {
    type Output = Result<usize, T::Error>;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let tx = &mut self.project().tx;

        if tx.is_empty() {
            return core::task::Poll::Ready(Ok(0));
        }

        tx.poll_transmit(cx)
    }
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

impl<Payload: AsRef<[u8]>> Message for (SocketAddress, Payload) {
    fn remote_address(&mut self) -> SocketAddress {
        self.0
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        Default::default()
    }

    fn delay(&mut self) -> Duration {
        Default::default()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        0
    }

    fn write_payload(&mut self, buffer: &mut [u8]) -> usize {
        let payload = self.1.as_ref();
        let len = payload.len();
        if let Some(buffer) = buffer.get_mut(..len) {
            buffer.copy_from_slice(payload);
            len
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inet::SocketAddressV4;

    #[test]
    fn message_tuple_test() {
        let address: SocketAddress = SocketAddressV4::new([127, 0, 0, 1], 80).into();
        let mut message = (address, [1u8, 2, 3]);

        let mut buffer = [0u8; 10];

        assert_eq!(message.remote_address(), address);
        assert_eq!(message.ecn(), Default::default());
        assert_eq!(message.delay(), Default::default());
        assert_eq!(message.ipv6_flow_label(), 0);
        assert_eq!(message.write_payload(&mut buffer[..]), 3);

        // assert an empty buffer doesn't panic
        assert_eq!(message.write_payload(&mut [][..]), 0);
    }
}
