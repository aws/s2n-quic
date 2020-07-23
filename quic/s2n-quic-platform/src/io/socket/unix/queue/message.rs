use super::msgname::{get_msgname, reset_msgname, set_msgname};
use core::{
    fmt,
    ops::{Deref, DerefMut},
};
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

/// Cross platform new type over network messages
#[repr(C)]
pub struct Message(platform::inner);

impl Message {
    /// Returns the ECN values for the message
    pub fn ecn(&self) -> ExplicitCongestionNotification {
        // TODO support ecn
        ExplicitCongestionNotification::default()
    }

    /// Sets the ECN values for the message
    pub fn set_ecn(&mut self, _ecn: ExplicitCongestionNotification) {
        // TODO support ecn
    }

    /// Returns the `SocketAddress` for the message
    pub fn remote_address(&self) -> Option<SocketAddress> {
        get_msgname(self.deref())
    }

    /// Sets the `SocketAddress` for the message
    pub fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        set_msgname(self.deref_mut(), remote_address);
    }

    /// Resets the `SocketAddress` for the message
    pub fn reset_remote_address(&mut self) {
        reset_msgname(self.deref_mut());
    }

    /// Returns a mutable slice for the message payload
    pub fn payload_mut(&mut self) -> &mut [u8] {
        unsafe {
            let iovec = &*self.deref().msg_iov;
            core::slice::from_raw_parts_mut(iovec.iov_base as *mut _, iovec.iov_len)
        }
    }

    /// Returns a pointer to the Message
    pub fn as_ptr(&self) -> *const Message {
        self as *const _
    }

    /// Returns a mutable pointer to the Message
    pub fn as_mut_ptr(&mut self) -> *mut Message {
        self as *mut _
    }

    /// Clones ownership of Message data into a new Message.
    ///
    /// # Safety
    /// This method will copy mutable pointers to iovecs which will
    /// result in multiple owners of buffer regions. Users will need
    /// to ensure the values are not borrowed concurrently.
    #[allow(dead_code)]
    pub(crate) unsafe fn create_multi_owner(&self) -> Self {
        Self(self.0)
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Message")
            .field("remote_address", &self.remote_address())
            .field("ecn", &self.ecn())
            .field("payload_len", &self.payload_len())
            .finish()
    }
}

#[cfg(s2n_quic_platform_socket_mmsg)]
mod platform {
    #![allow(dead_code)]
    use super::*;
    use core::ops::{Deref, DerefMut};
    use libc::{mmsghdr, msghdr};

    pub use libc::mmsghdr as inner;

    impl Message {
        /// Creates a new message for a given `msghdr`
        pub fn new(msg_hdr: msghdr) -> Self {
            Self(mmsghdr {
                msg_hdr,
                msg_len: 0,
            })
        }

        /// Returns the length of the payload
        pub fn payload_len(&self) -> usize {
            self.0.msg_len as usize
        }

        /// Sets the payload length for the message
        ///
        /// # Note
        /// Both the `msg_len` and `iov_len` are updated
        pub fn set_payload_len(&mut self, payload_len: usize) {
            unsafe {
                self.0.msg_len = payload_len as u32;
                (*self.deref_mut().msg_iov).iov_len = payload_len;
            }
        }

        /// Copy the lengths of the fields inside of one message into another.
        ///
        /// # Panics
        /// This should used in scenarios where the data pointers are the same.
        pub fn copy_field_lengths_from(&mut self, other: &Self) {
            debug_assert_eq!(
                self.deref().msg_name,
                other.deref().msg_name,
                "msg_name needs to point to the same data"
            );
            self.deref_mut().msg_namelen = other.deref().msg_namelen;

            debug_assert_eq!(
                self.deref().msg_iov,
                other.deref().msg_iov,
                "msg_iov needs to point to the same data"
            );
            self.0.msg_len = other.0.msg_len;
        }
    }

    impl Deref for Message {
        type Target = msghdr;

        fn deref(&self) -> &Self::Target {
            &self.0.msg_hdr
        }
    }

    impl DerefMut for Message {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0.msg_hdr
        }
    }
}

#[cfg(not(s2n_quic_platform_socket_mmsg))]
mod platform {
    #![allow(dead_code)]
    use super::*;
    use core::ops::{Deref, DerefMut};
    use libc::msghdr;

    pub use libc::msghdr as inner;

    impl Message {
        /// Creates a new message for a given `msghdr`
        pub fn new(msg_hdr: msghdr) -> Self {
            Self(msg_hdr)
        }

        /// Returns the length of the payload
        pub fn payload_len(&self) -> usize {
            unsafe { (*self.deref().msg_iov).iov_len }
        }

        /// Sets the payload length for the message
        pub fn set_payload_len(&mut self, payload_len: usize) {
            unsafe {
                (*self.deref_mut().msg_iov).iov_len = payload_len;
            }
        }

        /// Copy the lengths of the fields inside of one message into another.
        ///
        /// # Panics
        /// This should used in scenarios where the data pointers are the same.
        pub fn copy_field_lengths_from(&mut self, other: &Self) {
            debug_assert_eq!(
                self.deref().msg_name,
                other.deref().msg_name,
                "msg_name needs to point to the same data"
            );
            self.deref_mut().msg_namelen = other.deref().msg_namelen;
        }
    }

    impl Deref for Message {
        type Target = msghdr;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl DerefMut for Message {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }
}
