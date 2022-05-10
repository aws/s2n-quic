// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{inet::ExplicitCongestionNotification, path};
use core::time::Duration;

/// A structure capable of queueing and transmitting messages
pub trait Queue {
    type Entry: Entry<Handle = Self::Handle>;
    type Handle: path::Handle;

    /// Set to true if the queue supports setting ECN markings
    const SUPPORTS_ECN: bool = false;

    /// Set to true if the queue supports pacing of sending messages
    const SUPPORTS_PACING: bool = false;

    /// Set to true if the queue supports setting IPv6 flow labels
    const SUPPORTS_FLOW_LABELS: bool = false;

    /// Pushes a message into the transmission queue
    ///
    /// The index of the message is returned to enable further operations to be
    /// performed, e.g. encryption.
    fn push<M: Message<Handle = Self::Handle>>(&mut self, message: M) -> Result<Outcome, Error>;

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

pub struct Outcome {
    pub len: usize,
    pub index: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq)]
pub enum Error {
    /// The provided message did not write a payload
    EmptyPayload,

    /// The provided buffer was too small for the desired payload
    UndersizedBuffer,

    /// The transmission queue is at capacity
    AtCapacity,
}

/// An entry in a Tx queue
pub trait Entry {
    type Handle: path::Handle;

    /// Sets the message for the given entry
    fn set<M: Message<Handle = Self::Handle>>(&mut self, message: M) -> Result<usize, Error>;

    /// Returns the transmission payload as a slice of bytes
    fn payload(&self) -> &[u8];

    /// Returns the transmission payload as a mutable slice of bytes
    fn payload_mut(&mut self) -> &mut [u8];
}

/// Abstraction over a message to be sent on a socket
///
/// Instead of a concrete struct with eagerly evaluated fields,
/// using trait callbacks ensure messages only need to compute what
/// the actual transmission queue requires. For example, if the transmission
/// queue cannot set ECN markings, it will not call the [`Message::ecn`] function.
pub trait Message {
    type Handle: path::Handle;

    /// Returns the path handle on which this message should be sent
    fn path_handle(&self) -> &Self::Handle;

    /// Returns the ECN markings for the message
    fn ecn(&mut self) -> ExplicitCongestionNotification;

    /// Returns the Duration for which the message will be delayed.
    ///
    /// This is used in scenarios where packets need to be paced.
    fn delay(&mut self) -> Duration;

    /// Returns the IPv6 flow label for the message
    fn ipv6_flow_label(&mut self) -> u32;

    /// Returns true if the packet can be used in a GSO packet
    fn can_gso(&self, segment_len: usize, segment_count: usize) -> bool;

    /// Writes the payload of the message to an output buffer
    fn write_payload(&mut self, buffer: PayloadBuffer, gso_offset: usize) -> Result<usize, Error>;
}

#[derive(Debug)]
pub struct PayloadBuffer<'a>(&'a mut [u8]);

impl<'a> PayloadBuffer<'a> {
    #[inline]
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self(bytes)
    }

    /// # Safety
    ///
    /// This function should only be used in the case that the writer has its own safety checks in place
    #[inline]
    pub unsafe fn into_mut_slice(self) -> &'a mut [u8] {
        self.0
    }

    #[track_caller]
    #[inline]
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        if bytes.is_empty() {
            return Err(Error::EmptyPayload);
        }

        if let Some(buffer) = self.0.get_mut(0..bytes.len()) {
            buffer.copy_from_slice(bytes);
            Ok(bytes.len())
        } else {
            debug_assert!(
                false,
                "tried to write more bytes than was available in the buffer"
            );
            Err(Error::UndersizedBuffer)
        }
    }
}

impl<Handle: path::Handle, Payload: AsRef<[u8]>> Message for (Handle, Payload) {
    type Handle = Handle;

    fn path_handle(&self) -> &Self::Handle {
        &self.0
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

    fn can_gso(&self, segment_len: usize, _segment_count: usize) -> bool {
        segment_len >= self.1.as_ref().len()
    }

    fn write_payload(
        &mut self,
        mut buffer: PayloadBuffer,
        _gso_offset: usize,
    ) -> Result<usize, Error> {
        buffer.write(self.1.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inet::SocketAddressV4;

    #[test]
    fn message_tuple_test() {
        let remote_address = SocketAddressV4::new([127, 0, 0, 1], 80).into();
        let local_address = SocketAddressV4::new([192, 168, 0, 1], 3000).into();
        let tuple = path::Tuple {
            remote_address,
            local_address,
        };
        let mut message = (tuple, [1u8, 2, 3]);

        let mut buffer = [0u8; 10];

        assert_eq!(*message.path_handle(), tuple);
        assert_eq!(message.ecn(), Default::default());
        assert_eq!(message.delay(), Default::default());
        assert_eq!(message.ipv6_flow_label(), 0);
        assert_eq!(
            message.write_payload(PayloadBuffer::new(&mut buffer[..]), 0),
            Ok(3)
        );
    }

    #[test]
    #[should_panic]
    fn message_tuple_undersized_test() {
        let remote_address = SocketAddressV4::new([127, 0, 0, 1], 80).into();
        let local_address = SocketAddressV4::new([192, 168, 0, 1], 3000).into();
        let tuple = path::Tuple {
            remote_address,
            local_address,
        };
        let mut message = (tuple, [1u8, 2, 3]);

        // assert an undersized buffer panics in debug
        let _ = message.write_payload(PayloadBuffer::new(&mut [][..]), 0);
    }
}
