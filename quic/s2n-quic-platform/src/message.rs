// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use alloc::boxed::Box;
use core::{cell::UnsafeCell, pin::Pin};
use s2n_quic_core::{inet::datagram, io::tx, path};

#[cfg(any(s2n_quic_platform_socket_msg, s2n_quic_platform_socket_mmsg))]
mod cmsg;
#[cfg(s2n_quic_platform_socket_mmsg)]
pub mod mmsg;
#[cfg(s2n_quic_platform_socket_msg)]
pub mod msg;
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

pub type Storage = Pin<Box<[UnsafeCell<u8>]>>;

/// An abstract message that can be sent and received on a network
pub trait Message: 'static + Copy {
    type Handle: path::Handle;

    const SUPPORTS_GSO: bool;
    const SUPPORTS_ECN: bool;
    const SUPPORTS_FLOW_LABELS: bool;

    /// Allocates `entries` messages, each with `payload_len` bytes
    fn alloc(entries: u32, payload_len: u32, offset: usize) -> Storage;

    /// Returns the length of the payload
    fn payload_len(&self) -> usize;

    /// Sets the payload length for the message
    ///
    /// # Safety
    /// This method should only set the payload less than or
    /// equal to its initially allocated size.
    unsafe fn set_payload_len(&mut self, payload_len: usize);

    /// Validates that the `source` message can be replicated to `dest`.
    ///
    /// # Panics
    ///
    /// This panics when the messages cannot be replicated
    fn validate_replication(source: &Self, dest: &Self);

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
