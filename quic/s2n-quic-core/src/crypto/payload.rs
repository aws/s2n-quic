use crate::packet::number::PacketNumberLen;
use s2n_codec::{CheckedRange, DecoderBuffer, DecoderBufferMut, DecoderError};

/// Type which restricts access to protected and encrypted payloads
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtectedPayload<'a> {
    pub(crate) header_len: usize,
    pub(crate) buffer: DecoderBufferMut<'a>,
}

impl<'a> core::fmt::Debug for ProtectedPayload<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        // Since the protected payload is not very helpful for debugging purposes,
        // we just print the length of the protected payload as long as we are not in
        // pretty-printing mode.
        // Snapshot tests use the pretty-printing mode, therefore we can't change the Debug behavior
        // for those.
        let print_buffer_content = f.alternate();

        let mut debug_struct = f.debug_struct("ProtectedPayload");
        let mut debug_struct = debug_struct.field("header_len", &self.header_len);

        if !print_buffer_content {
            debug_struct = debug_struct.field("buffer_len", &(self.buffer.len() - self.header_len))
        } else {
            debug_struct = debug_struct.field("buffer", &self.buffer)
        }
        debug_struct.finish()
    }
}

impl<'a> ProtectedPayload<'a> {
    /// Creates a new protected payload with a header_len
    pub fn new(header_len: usize, buffer: &'a mut [u8]) -> Self {
        debug_assert!(buffer.len() >= header_len, "header_len is too large");

        Self {
            header_len,
            buffer: DecoderBufferMut::new(buffer),
        }
    }

    /// Reads data from a `CheckedRange`
    pub fn get_checked_range(&self, range: &CheckedRange) -> DecoderBuffer {
        self.buffer.get_checked_range(range)
    }

    pub(crate) fn header_protection_sample(
        &self,
        sample_len: usize,
    ) -> Result<&[u8], DecoderError> {
        self.buffer
            .peek()
            .skip(self.header_len)?
            .skip(PacketNumberLen::MAX_LEN)?
            .decode_slice(sample_len)
            .map(|(sample, _)| sample.into_less_safe_slice())
    }
}

/// Type which restricts access to encrypted payloads
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EncryptedPayload<'a> {
    pub(crate) header_len: usize,
    pub(crate) packet_number_len: PacketNumberLen,
    pub(crate) buffer: DecoderBufferMut<'a>,
}

impl<'a> core::fmt::Debug for EncryptedPayload<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        // Since the protected payload is not very helpful for debugging purposes,
        // we just print the length of the protected payload as long as we are not in
        // pretty-printing mode.
        // Snapshot tests use the pretty-printing mode, therefore we can't change the Debug behavior
        // for those.
        let print_buffer_content = f.alternate();

        let mut debug_struct = f.debug_struct("EncryptedPayload");
        let mut debug_struct = debug_struct
            .field("header_len", &self.header_len)
            .field("packet_number_len", &self.packet_number_len);

        if !print_buffer_content {
            debug_struct = debug_struct.field("buffer_len", &(self.buffer.len() - self.header_len))
        } else {
            debug_struct = debug_struct.field("buffer", &self.buffer)
        }
        debug_struct.finish()
    }
}

impl<'a> EncryptedPayload<'a> {
    pub(crate) fn new(
        header_len: usize,
        packet_number_len: PacketNumberLen,
        buffer: &'a mut [u8],
    ) -> Self {
        debug_assert!(
            buffer.len() >= header_len + packet_number_len.bytesize(),
            "header_len is too large"
        );

        Self {
            header_len,
            packet_number_len,
            buffer: DecoderBufferMut::new(buffer),
        }
    }

    /// Reads the packet tag in the payload
    pub fn get_tag(&self) -> u8 {
        self.buffer.as_less_safe_slice()[0]
    }

    /// Reads data from a `CheckedRange`
    pub fn get_checked_range(&self, range: &CheckedRange) -> DecoderBuffer {
        self.buffer.get_checked_range(range)
    }

    pub(crate) fn split_mut(self) -> (&'a mut [u8], &'a mut [u8]) {
        let (header, payload) = self
            .buffer
            .decode_slice(self.header_len + self.packet_number_len.bytesize())
            .expect("header_len already checked");
        (
            header.into_less_safe_slice(),
            payload.into_less_safe_slice(),
        )
    }

    pub(crate) fn header_protection_sample(
        &self,
        sample_len: usize,
    ) -> Result<&[u8], DecoderError> {
        self.buffer
            .peek()
            .skip(self.header_len)?
            .skip(PacketNumberLen::MAX_LEN)?
            .decode_slice(sample_len)
            .map(|(sample, _)| sample.into_less_safe_slice())
    }
}
