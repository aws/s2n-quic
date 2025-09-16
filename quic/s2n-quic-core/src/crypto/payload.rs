// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::packet::number::PacketNumberLen;
use s2n_codec::{CheckedRange, DecoderBuffer, DecoderBufferMut, DecoderError};

/// Type which restricts access to protected and encrypted payloads.
///
/// The `ProtectedPayload` is an `EncryptedPayload` that has had
/// header protection applied. So to get to the cleartext payload,
/// first you remove header protection, and then you decrypt the packet
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtectedPayload<'a> {
    pub(crate) header_len: usize,
    pub(crate) buffer: DecoderBufferMut<'a>,
}

impl core::fmt::Debug for ProtectedPayload<'_> {
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
    pub fn get_checked_range(&self, range: &CheckedRange) -> DecoderBuffer<'_> {
        self.buffer.get_checked_range(range)
    }

    pub(crate) fn header_protection_sample(
        &self,
        sample_len: usize,
    ) -> Result<&[u8], DecoderError> {
        header_protection_sample(self.buffer.peek(), self.header_len, sample_len)
    }

    /// Returns the length of the payload, including the header
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns `true` if the payload is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

/// Type which restricts access to encrypted payloads
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EncryptedPayload<'a> {
    pub(crate) header_len: usize,
    pub(crate) packet_number_len: PacketNumberLen,
    pub(crate) buffer: DecoderBufferMut<'a>,
}

impl core::fmt::Debug for EncryptedPayload<'_> {
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
    pub fn get_checked_range(&self, range: &CheckedRange) -> DecoderBuffer<'_> {
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
        header_protection_sample(self.buffer.peek(), self.header_len, sample_len)
    }
}

fn header_protection_sample(
    buffer: DecoderBuffer<'_>,
    header_len: usize,
    sample_len: usize,
) -> Result<&[u8], DecoderError> {
    let buffer = buffer.skip(header_len)?;

    //= https://www.rfc-editor.org/rfc/rfc9001#section-5.4.2
    //# in sampling packet ciphertext for header protection, the Packet Number field is
    //# assumed to be 4 bytes long
    let buffer = buffer.skip(PacketNumberLen::MAX_LEN)?;

    //= https://www.rfc-editor.org/rfc/rfc9001#section-5.4.2
    //# An endpoint MUST discard packets that are not long enough to contain
    //# a complete sample.
    let (sample, _) = buffer.decode_slice(sample_len)?;

    Ok(sample.into_less_safe_slice())
}
