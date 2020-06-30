use super::Message;
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

/// Handle for reading from and writing to one or two messages,
/// depending on the presence of the `mmsg` feature.
#[derive(Debug)]
pub struct Entry<'a> {
    /// Primary message reference for the entry
    pub(crate) primary: &'a mut Message,

    #[cfg(feature = "mmsg")]
    /// Secondary message reference, if `mmsg` is enabled
    pub(crate) secondary: &'a mut Message,
}

impl<'a> Entry<'a> {
    pub fn new(messages: &'a mut [Message], index: usize, capacity: usize) -> Self {
        #[cfg(feature = "mmsg")]
        {
            let (primary, secondary) = messages.split_at_mut(capacity);

            Entry {
                primary: &mut primary[index],
                secondary: &mut secondary[index],
            }
        }

        #[cfg(not(feature = "mmsg"))]
        {
            let _ = capacity;

            Entry {
                primary: &mut messages[index],
            }
        }
    }

    /// Returns the ECN values for the message
    ///
    /// # Panics
    /// If the values for all of the entries is not the same a
    /// panic will be triggered.
    pub fn ecn(&self) -> ExplicitCongestionNotification {
        #[cfg(feature = "mmsg")]
        debug_assert_eq!(self.primary.ecn(), self.secondary.ecn());

        self.primary.ecn()
    }

    /// Sets the ECN values for the message
    pub fn set_ecn(&mut self, ecn: ExplicitCongestionNotification) {
        self.primary.set_ecn(ecn);

        #[cfg(feature = "mmsg")]
        self.secondary.set_ecn(ecn);
    }

    /// Returns the `SocketAddress` for the message
    pub fn remote_address(&self) -> Option<SocketAddress> {
        self.primary.remote_address()
    }

    /// Sets the `SocketAddress` for the message
    pub fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        self.primary.set_remote_address(remote_address);

        #[cfg(feature = "mmsg")]
        self.secondary.set_remote_address(remote_address);
    }

    /// Resets the `SocketAddress` for the message
    pub fn reset_remote_address(&mut self) {
        self.primary.reset_remote_address();

        #[cfg(feature = "mmsg")]
        self.secondary.reset_remote_address();
    }

    /// Consumes the entry while returning the message payload
    pub fn take_payload(self) -> &'a mut [u8] {
        #[cfg(feature = "mmsg")]
        debug_assert_eq!(
            self.primary.payload_mut().as_ptr(),
            self.secondary.payload_mut().as_ptr()
        );

        // The iovec points to the same location
        self.primary.payload_mut()
    }

    /// Returns a mutable slice for the message payload
    pub fn payload_mut(&mut self) -> &mut [u8] {
        #[cfg(feature = "mmsg")]
        debug_assert_eq!(
            self.primary.payload_mut().as_ptr(),
            self.secondary.payload_mut().as_ptr()
        );

        // The iovec points to the same location
        self.primary.payload_mut()
    }

    /// Returns the length of the payload
    pub fn payload_len(&self) -> usize {
        self.primary.payload_len()
    }

    /// Sets the payload length for the message
    pub fn set_payload_len(&mut self, payload_len: usize) {
        self.primary.set_payload_len(payload_len);

        #[cfg(feature = "mmsg")]
        self.secondary.set_payload_len(payload_len);
    }

    /// Returns a pointer to the primary Message
    #[allow(dead_code)]
    pub fn as_ptr(&self) -> *const Message {
        self.primary.as_ptr()
    }

    /// Returns a mutable pointer to the primary Message
    #[allow(dead_code)]
    pub fn as_mut_ptr(&mut self) -> *mut Message {
        self.primary.as_mut_ptr()
    }
}
