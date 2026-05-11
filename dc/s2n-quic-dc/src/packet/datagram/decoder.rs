// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    packet::{
        datagram::{RoutingInfo, Tag},
        storage, WireVersion,
    },
};
use core::{fmt, ops::Deref};
use s2n_codec::{
    decoder_invariant, CheckedRange, DecoderBufferMut, DecoderBufferMutResult as R, DecoderError,
};
use s2n_quic_core::{assume, varint::VarInt};

pub type PacketNumber = VarInt;

pub trait Validator {
    fn validate_tag(&mut self, tag: Tag) -> Result<(), DecoderError>;
}

impl Validator for () {
    #[inline]
    fn validate_tag(&mut self, _tag: Tag) -> Result<(), DecoderError> {
        Ok(())
    }
}

impl Validator for Tag {
    #[inline]
    fn validate_tag(&mut self, actual: Tag) -> Result<(), DecoderError> {
        decoder_invariant!(*self == actual, "unexpected packet type");
        Ok(())
    }
}

impl<A, B> Validator for (A, B)
where
    A: Validator,
    B: Validator,
{
    #[inline]
    fn validate_tag(&mut self, tag: Tag) -> Result<(), DecoderError> {
        self.0.validate_tag(tag)?;
        self.1.validate_tag(tag)?;
        Ok(())
    }
}

/// Packet metadata without any storage - all the parsed fields and ranges
#[derive(Clone, Copy, Debug)]
pub struct Meta {
    tag: Tag,
    wire_version: WireVersion,
    credentials: Credentials,
    source_control_port: u16,
    routing_info: RoutingInfo,
    packet_number: PacketNumber,
    header: CheckedRange,
    application_header: CheckedRange,
    payload: CheckedRange,
    auth_tag: CheckedRange,
}

impl Meta {
    #[inline]
    pub fn tag(&self) -> Tag {
        self.tag
    }

    #[inline]
    pub fn wire_version(&self) -> WireVersion {
        self.wire_version
    }

    #[inline]
    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    #[inline]
    pub fn source_control_port(&self) -> u16 {
        self.source_control_port
    }

    #[inline]
    pub fn routing_info(&self) -> RoutingInfo {
        self.routing_info
    }

    #[inline]
    pub fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    #[inline]
    pub fn crypto_nonce(&self) -> u64 {
        self.packet_number.as_u64()
    }

    /// Returns the length of the outer packet header (everything before the application header).
    ///
    /// The `application_header` range is always decoded from *within* the `header` buffer
    /// (see `Meta::decode`), so `application_header.len() <= header.len()` is a structural
    /// invariant of every successfully decoded packet.
    #[inline]
    pub fn outer_header_len(&self) -> usize {
        debug_assert!(
            self.application_header.len() <= self.header.len(),
            "application_header must be a sub-range of header"
        );
        self.header.len() - self.application_header.len()
    }

    /// Combine this metadata with storage to create a Packet
    #[inline]
    pub fn with_storage<S: storage::Bytes>(self, storage: S) -> Result<Packet<S>, (Self, S)> {
        Packet::from_parts(self, storage)
    }

    /// Decode packet metadata and create CheckedRanges relative to a storage buffer
    #[inline(always)]
    pub fn decode<V: Validator>(
        storage_buf: &DecoderBufferMut,
        mut validator: V,
        crypto_tag_len: usize,
    ) -> Result<Self, DecoderError> {
        let buffer = storage_buf.peek();

        unsafe {
            assume!(
                crypto_tag_len >= 16,
                "tag len needs to be at least 16 bytes"
            );
        }

        let start_len = buffer.len();

        let (tag, buffer) = buffer.decode()?;
        validator.validate_tag(tag)?;

        let (credentials, buffer) = buffer.decode()?;
        let (wire_version, buffer) = buffer.decode()?;
        let (source_control_port, buffer) = buffer.decode()?;

        let (packet_number, buffer) = if tag.has_packet_number() {
            buffer.decode::<VarInt>()?
        } else {
            (VarInt::ZERO, buffer)
        };

        // Decode routing info if present
        let (routing_info, buffer) = if tag.has_routing_info() {
            buffer.decode()?
        } else {
            (RoutingInfo::None, buffer)
        };

        let (payload_len, buffer) = buffer.decode::<VarInt>()?;
        let payload_len = (*payload_len) as usize;

        let (application_header_len, buffer) = if tag.payload_encrypted() {
            let (application_header_len, buffer) = buffer.decode::<VarInt>()?;
            ((*application_header_len) as usize, buffer)
        } else {
            (0, buffer)
        };

        // Use skip_into_range for application_header
        let (application_header, buffer) =
            buffer.skip_into_range(application_header_len, storage_buf)?;

        // compute the total header range
        let total_header_len = start_len - buffer.len();
        let header = {
            let buffer = storage_buf.peek();
            let (header, _) = buffer.skip_into_range(total_header_len, storage_buf)?;
            header
        };

        // Use skip_into_range for payload and auth_tag
        let (payload, buffer) = buffer.skip_into_range(payload_len, storage_buf)?;
        let (auth_tag, _buffer) = buffer.skip_into_range(crypto_tag_len, storage_buf)?;

        Ok(Meta {
            tag,
            wire_version,
            credentials,
            source_control_port,
            routing_info,
            packet_number,
            header,
            application_header,
            payload,
            auth_tag,
        })
    }
}

pub struct Packet<S: storage::Bytes> {
    meta: Meta,
    storage: S,
}

impl<S: storage::Bytes> fmt::Debug for Packet<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("datagram::Packet")
            .field("tag", &self.meta.tag)
            .field("wire_version", &self.meta.wire_version)
            .field("credentials", &self.meta.credentials)
            .field("source_control_port", &self.meta.source_control_port)
            .field("routing_info", &self.meta.routing_info)
            .field("packet_number", &self.meta.packet_number)
            .field("payload_len", &self.meta.payload.len())
            .finish()
    }
}

impl<S: storage::Bytes> Deref for Packet<S> {
    type Target = Meta;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.meta
    }
}

impl<S: storage::Bytes> Packet<S> {
    #[inline]
    pub fn application_header(&self) -> &[u8] {
        let header = self.meta.header.get(&*self.storage);
        self.meta.application_header.get(header)
    }

    #[inline]
    pub fn header(&self) -> &[u8] {
        self.meta.header.get(&*self.storage)
    }

    #[inline]
    pub fn payload(&self) -> &[u8] {
        self.meta.payload.get(&*self.storage)
    }

    #[inline]
    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.meta.payload.get_mut(&mut *self.storage)
    }

    #[inline]
    pub fn auth_tag(&self) -> &[u8] {
        self.meta.auth_tag.get(&*self.storage)
    }

    /// Decrypt the packet payload in place using the provided opener.
    ///
    /// This authenticates the packet and decrypts the payload in a single operation.
    #[inline]
    pub fn decrypt_in_place<O>(&mut self, opener: &O) -> Result<(), crate::crypto::open::Error>
    where
        O: crate::crypto::open::Application,
    {
        let key_phase = self.tag().key_phase();
        let packet_number = self.packet_number().as_u64();
        let header_len = self.meta.header.len();
        let payload_len = self.meta.payload.len();
        let auth_tag_len = self.meta.auth_tag.len();

        let (header, payload_and_tag) = unsafe {
            // SAFETY: bounds were validated during packet decoding in Meta::decode
            let ptr = self.storage.as_mut_ptr();
            let header = core::slice::from_raw_parts_mut(ptr, header_len);
            let payload_and_tag =
                core::slice::from_raw_parts_mut(ptr.add(header_len), payload_len + auth_tag_len);
            (header, payload_and_tag)
        };

        opener.decrypt_in_place(key_phase, packet_number, header, payload_and_tag)
    }

    /// Returns the required buffer size for `decrypt_into`
    ///
    /// This is the payload length.
    #[inline]
    pub fn decrypt_into_len(&self) -> usize {
        self.meta.payload.len()
    }

    /// Decrypt the packet payload into a destination buffer.
    ///
    /// The destination buffer will contain only the decrypted application payload.
    /// The application header remains available via [`Self::application_header`].
    ///
    /// Returns the total payload length written (`payload.len()`).
    ///
    /// Use `decrypt_into_len()` to get the required buffer size.
    #[inline]
    pub fn decrypt_into<O>(
        &self,
        opener: &O,
        dest: &mut bytes::buf::UninitSlice,
    ) -> Result<usize, crate::crypto::open::Error>
    where
        O: crate::crypto::open::Application,
    {
        let key_phase = self.tag().key_phase();
        let packet_number = self.packet_number().as_u64();

        let payload_len = self.meta.payload.len();
        let auth_tag_len = self.meta.auth_tag.len();

        // Ensure destination buffer is large enough
        if dest.len() < payload_len {
            return Err(crate::crypto::open::Error::UnsupportedOperation);
        }

        let header_len = self.meta.header.len();

        // Get slices using same pattern as decrypt_in_place
        let (header, payload_in, tag) = unsafe {
            // SAFETY: bounds were validated during packet decoding in Meta::decode
            let ptr = self.storage.as_ptr();
            let header = core::slice::from_raw_parts(ptr, header_len);
            let payload_in = core::slice::from_raw_parts(ptr.add(header_len), payload_len);
            let tag = core::slice::from_raw_parts(ptr.add(header_len + payload_len), auth_tag_len);
            (header, payload_in, tag)
        };

        // Decrypt payload directly into destination buffer.
        opener.decrypt(
            key_phase,
            packet_number,
            header,
            payload_in,
            tag,
            &mut dest[..payload_len],
        )?;
        Ok(payload_len)
    }

    /// Create a packet from metadata and storage, validating that the storage is compatible
    #[inline]
    pub fn from_parts(meta: Meta, storage: S) -> Result<Self, (Meta, S)> {
        // Validate storage by attempting to get the ranges
        let _ = meta.header.get(&*storage);
        let _ = meta.payload.get(&*storage);
        let _ = meta.auth_tag.get(&*storage);

        Ok(Self { meta, storage })
    }

    /// Extract the metadata, consuming the packet and returning both metadata and storage
    #[inline]
    pub fn into_parts(self) -> (Meta, S) {
        (self.meta, self.storage)
    }

    /// Get a copy of the metadata without consuming the packet
    #[inline]
    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    /// Get a reference to the storage
    #[inline]
    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// Replace the storage with a new one, validating that it's still valid for the ranges
    #[inline]
    pub fn replace_storage<S2: storage::Bytes>(
        self,
        new_storage: S2,
    ) -> Result<Packet<S2>, (Meta, S2)> {
        // Validate new storage by attempting to get the ranges
        let _ = self.meta.header.get(&*new_storage);
        let _ = self.meta.payload.get(&*new_storage);
        let _ = self.meta.auth_tag.get(&*new_storage);

        Ok(Packet {
            meta: self.meta,
            storage: new_storage,
        })
    }
}

impl Packet<&mut [u8]> {
    #[inline(always)]
    pub fn decode<V: Validator>(
        buffer: DecoderBufferMut,
        validator: V,
        crypto_tag_len: usize,
    ) -> R<Self> {
        // First, figure out how long the packet is by peeking
        let meta = Meta::decode(&buffer, validator, crypto_tag_len)?;
        let packet_len = meta.header.len() + meta.payload.len() + meta.auth_tag.len();

        // Now decode the full packet storage
        let (storage_buf, buffer) = buffer.decode_slice(packet_len)?;

        let storage = unsafe {
            // SAFETY: extend the lifetime of the buffer
            let slice = storage_buf.into_less_safe_slice();
            core::mem::transmute::<&mut [u8], &mut [u8]>(slice)
        };

        let packet = Packet { meta, storage };

        Ok((packet, buffer))
    }
}
