#[cfg(s2n_quic_platform_socket_mmsg)]
pub mod mmsg;

#[cfg(s2n_quic_platform_socket_msg)]
pub mod msg;

#[cfg(feature = "std")]
pub mod std;

pub mod queue;

use libc::c_void;
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

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

    /// Returns a mutable slice for the message payload
    fn payload_ptr_mut(&mut self) -> *mut u8;

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

pub trait Ring {
    type Message: Message;

    fn len(&self) -> usize;
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn mtu(&self) -> usize;
    fn as_slice(&self) -> &[Self::Message];
    fn as_mut_slice(&mut self) -> &mut [Self::Message];
}
