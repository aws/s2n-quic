// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ffi::c_void;
use s2n_quic_core::{inet::datagram, io::tx, path};

#[cfg(any(s2n_quic_platform_socket_msg, s2n_quic_platform_socket_mmsg))]
pub mod cmsg;
#[cfg(s2n_quic_platform_socket_mmsg)]
pub mod mmsg;
#[cfg(s2n_quic_platform_socket_msg)]
pub mod msg;
pub mod queue;
pub mod simple;

pub mod default {
    cfg_if::cfg_if! {
        if #[cfg(s2n_quic_platform_socket_mmsg)] {
            pub use super::mmsg::*;
        } else if #[cfg(s2n_quic_platform_socket_msg)] {
            pub use super::msg::*;
        } else {
            pub use super::simple::*;
        }
    }
}

/// An abstract message that can be sent and received on a network
pub trait Message {
    type Handle: path::Handle;

    const SUPPORTS_GSO: bool;

    /// Returns the length of the payload
    fn payload_len(&self) -> usize;

    /// Sets the payload length for the message
    ///
    /// # Safety
    /// This method should only set the payload less than or
    /// equal to its initially allocated size.
    unsafe fn set_payload_len(&mut self, payload_len: usize);

    /// Returns true if this message can be included in the same GSO payload as the `other` message
    fn can_gso<M: tx::Message<Handle = Self::Handle>>(&self, other: &mut M) -> bool;

    /// Copies the relevant fields inside of one message into another.
    ///
    /// # Panics
    /// This should used in scenarios where the data pointers are the same.
    fn replicate_fields_from(&mut self, other: &Self);

    /// Returns a mutable pointer for the message payload
    fn payload_ptr_mut(&mut self) -> *mut u8;

    /// Returns a mutable slice for the message payload
    #[inline]
    fn payload_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.payload_ptr_mut(), self.payload_len()) }
    }

    /// Sets the segment size for the message payload
    fn set_segment_size(&mut self, _size: usize) {
        panic!("cannot use GSO on the current platform");
    }

    /// Resets the message for future use
    ///
    /// # Safety
    /// This method should only set the MTU to the original value
    unsafe fn reset(&mut self, mtu: usize);

    /// Returns a pointer to the Message
    fn as_ptr(&self) -> *const c_void {
        self as *const _ as *const _
    }

    /// Returns a mutable pointer to the Message
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut _
    }

    /// Reads the message as an RX packet
    fn rx_read(&mut self, local_address: &path::LocalAddress) -> Option<RxMessage<Self::Handle>>;

    /// Writes the message into the TX packet
    fn tx_write<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        message: M,
    ) -> Result<usize, tx::Error>;
}

pub struct RxMessage<'a, Handle: Copy> {
    /// The received header for the message
    pub header: datagram::Header<Handle>,
    /// The number of segments inside the message
    pub segment_size: usize,
    /// The full payload of the message
    pub payload: &'a mut [u8],
}

impl<'a, Handle: Copy> RxMessage<'a, Handle> {
    #[inline]
    pub fn for_each<F: FnMut(datagram::Header<Handle>, &mut [u8])>(self, mut on_packet: F) {
        debug_assert_ne!(self.segment_size, 0);

        for segment in self.payload.chunks_mut(self.segment_size) {
            on_packet(self.header, segment);
        }
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

    /// Returns the maximum number of GSO segments that can be used
    fn max_gso(&self) -> usize;

    /// Disables the ability for the ring to send GSO messages
    ///
    /// This will be called in case the runtime encounters an IO error and will
    /// try again with GSO disabled.
    fn disable_gso(&mut self);

    /// Returns all of the messages in the ring
    ///
    /// The first half of the slice should be duplicated into the second half
    fn as_slice(&self) -> &[Self::Message];

    /// Returns a mutable slice of the messages in the ring
    fn as_mut_slice(&mut self) -> &mut [Self::Message];
}
