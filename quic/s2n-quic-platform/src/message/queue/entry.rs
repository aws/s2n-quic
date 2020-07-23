use crate::message::Message;
use libc::c_void;
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

/// Handle for reading from and writing to one or two messages,
/// depending on the presence of the `mmsg` feature.
#[derive(Debug)]
pub struct Entry<'a, Message> {
    /// Primary message reference for the entry
    pub(crate) primary: &'a mut Message,

    /// Secondary message reference, if `mmsg` is enabled
    pub(crate) secondary: &'a mut Message,
}

impl<'a, Message> Entry<'a, Message> {
    pub fn new(messages: &'a mut [Message], index: usize, capacity: usize) -> Self {
        let (primary, secondary) = messages.split_at_mut(capacity);

        Entry {
            primary: &mut primary[index],
            secondary: &mut secondary[index],
        }
    }
}

impl<'a, Msg: Message> Message for Entry<'a, Msg> {
    /// Returns the ECN values for the message
    ///
    /// # Panics
    /// If the values for all of the entries is not the same a
    /// panic will be triggered.
    fn ecn(&self) -> ExplicitCongestionNotification {
        debug_assert_eq!(self.primary.ecn(), self.secondary.ecn());
        self.primary.ecn()
    }

    /// Sets the ECN values for the message
    fn set_ecn(&mut self, ecn: ExplicitCongestionNotification) {
        self.primary.set_ecn(ecn);
        self.secondary.set_ecn(ecn);
    }

    /// Returns the `SocketAddress` for the message
    fn remote_address(&self) -> Option<SocketAddress> {
        self.primary.remote_address()
    }

    /// Sets the `SocketAddress` for the message
    fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        self.primary.set_remote_address(remote_address);
        self.secondary.set_remote_address(remote_address);
    }

    /// Resets the `SocketAddress` for the message
    fn reset_remote_address(&mut self) {
        self.primary.reset_remote_address();
        self.secondary.reset_remote_address();
    }

    fn payload_ptr_mut(&mut self) -> *mut u8 {
        debug_assert_eq!(
            self.primary.payload_ptr_mut(),
            self.secondary.payload_ptr_mut()
        );

        self.primary.payload_ptr_mut()
    }

    /// Returns the length of the payload
    fn payload_len(&self) -> usize {
        self.primary.payload_len()
    }

    /// Sets the payload length for the message
    unsafe fn set_payload_len(&mut self, payload_len: usize) {
        self.primary.set_payload_len(payload_len);
        self.secondary.set_payload_len(payload_len);
    }

    fn replicate_fields_from(&mut self, other: &Self) {
        self.primary.replicate_fields_from(&other.primary);
        self.secondary.replicate_fields_from(&other.secondary);
    }

    /// Returns a pointer to the primary Message
    #[allow(dead_code)]
    fn as_ptr(&self) -> *const c_void {
        self.primary.as_ptr()
    }

    /// Returns a mutable pointer to the primary Message
    #[allow(dead_code)]
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self.primary.as_mut_ptr()
    }
}
