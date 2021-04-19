// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[macro_use]
mod macros;

#[cfg(s2n_quic_platform_socket_mmsg)]
pub mod mmsg;

#[cfg(s2n_quic_platform_socket_msg)]
pub mod msg;

pub mod queue;
pub mod simple;

use core::ffi::c_void;
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

/// An abstract message that can be sent and received on a network
pub trait Message {
    /// Returns the ECN values for the message
    fn ecn(&self) -> ExplicitCongestionNotification;

    /// Sets the ECN values for the message
    fn set_ecn(&mut self, _ecn: ExplicitCongestionNotification);

    /// Returns the `SocketAddress` for the message
    fn remote_address(&self) -> Option<SocketAddress>;

    /// Sets the `SocketAddress` for the message
    fn set_remote_address(&mut self, remote_address: &SocketAddress);

    /// Resets the `SocketAddress` for the message
    fn reset_remote_address(&mut self);

    /// Returns the length of the payload
    fn payload_len(&self) -> usize;

    /// Sets the payload length for the message
    ///
    /// # Safety
    /// This method should only set the payload less than or
    /// equal to its initially allocated size.
    unsafe fn set_payload_len(&mut self, payload_len: usize);

    /// Copies the relevant fields inside of one message into another.
    ///
    /// # Panics
    /// This should used in scenarios where the data pointers are the same.
    fn replicate_fields_from(&mut self, other: &Self);

    /// Returns a pointer for the message payload
    fn payload_ptr(&self) -> *const u8;

    /// Returns a mutable pointer for the message payload
    fn payload_ptr_mut(&mut self) -> *mut u8;

    /// Returns a slice for the message payload
    fn payload(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.payload_ptr(), self.payload_len()) }
    }

    /// Returns a mutable slice for the message payload
    fn payload_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.payload_ptr_mut(), self.payload_len()) }
    }

    /// Returns a pointer to the Message
    fn as_ptr(&self) -> *const c_void {
        self as *const _ as *const _
    }

    /// Returns a mutable pointer to the Message
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut _
    }
}

/// A message ring used to back a queue
pub trait Ring {
    /// The type of message that is stored in the ring
    type Message: Message;

    /// Returns the length of the ring
    ///
    /// This value should be half the length of the slice
    /// returned to ensure contiguous access.
    fn len(&self) -> usize;

    /// Returns true if the ring is empty
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the maximum transmission unit for the ring
    fn mtu(&self) -> usize;

    /// Returns all of the messages in the ring
    ///
    /// The first half of the slice should be duplicated into the second half
    fn as_slice(&self) -> &[Self::Message];

    /// Returns a mutable slice of the messages in the ring
    fn as_mut_slice(&mut self) -> &mut [Self::Message];
}
