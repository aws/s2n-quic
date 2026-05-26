// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    packet::{
        control::{RoutingInfo, Tag},
        storage, stream, WireVersion,
    },
};
use core::{fmt, ops::Deref};
use s2n_codec::{
    decoder_invariant, CheckedRange, DecoderBufferMut, DecoderBufferMutResult as R, DecoderError,
};
use s2n_quic_core::{assume, frame::FrameMut, varint::VarInt};

type PacketNumber = VarInt;

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
    source_queue_id: Option<VarInt>,
    binding_id: Option<stream::Id>,
    packet_number: PacketNumber,
    routing_info: RoutingInfo,
    header: CheckedRange,
    control_data: CheckedRange,
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
    pub fn source_queue_id(&self) -> Option<VarInt> {
        self.source_queue_id
    }

    #[inline]
    pub fn binding_id(&self) -> Option<&stream::Id> {
        self.binding_id.as_ref()
    }

    #[inline]
    pub fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    #[inline]
    pub fn routing_info(&self) -> RoutingInfo {
        self.routing_info
    }

    #[inline]
    pub fn total_len(&self) -> usize {
        self.header.len() + self.auth_tag.len()
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

        let (binding_id, buffer) = if tag.is_stream() {
            let (binding_id, buffer) = buffer.decode()?;
            (Some(binding_id), buffer)
        } else {
            (None, buffer)
        };

        let (source_queue_id, buffer) = if tag.has_source_queue_id() {
            let (v, buffer) = buffer.decode()?;
            (Some(v), buffer)
        } else {
            (None, buffer)
        };

        let (packet_number, buffer) = buffer.decode::<VarInt>()?;
        let (control_data_len, buffer) = buffer.decode::<VarInt>()?;

        // Decode routing info if present
        let (routing_info, buffer) = if tag.has_routing_info() {
            buffer.decode()?
        } else {
            (RoutingInfo::None, buffer)
        };

        let (control_data, buffer) = buffer.skip_into_range(*control_data_len as _, storage_buf)?;

        // compute the auth header range
        let total_header_len = start_len - buffer.len();
        let header = {
            let buffer = storage_buf.peek();
            let (header, _) = buffer.skip_into_range(total_header_len, storage_buf)?;
            header
        };

        let (auth_tag, _buffer) = buffer.skip_into_range(crypto_tag_len, storage_buf)?;

        Ok(Meta {
            tag,
            wire_version,
            credentials,
            source_queue_id,
            binding_id,
            packet_number,
            routing_info,
            header,
            control_data,
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
        let header = self.meta.header.get(&self.storage);

        let mut s = f.debug_struct("control::Packet");

        s.field("tag", &self.meta.tag)
            .field("wire_version", &self.meta.wire_version)
            .field("credentials", &self.meta.credentials)
            .field("source_queue_id", &self.meta.source_queue_id)
            .field("binding_id", &self.meta.binding_id)
            .field("packet_number", &self.meta.packet_number)
            .field("routing_info", &self.meta.routing_info);

        if !self.meta.control_data.is_empty() {
            s.field("control_data", &self.meta.control_data.get(header));
        }

        s.field("auth_tag", &self.meta.auth_tag.get(&self.storage))
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
    pub fn control_data(&self) -> &[u8] {
        let header = self.meta.header.get(&self.storage);
        self.meta.control_data.get(header)
    }

    #[inline]
    pub fn control_data_mut(&mut self) -> &mut [u8] {
        let header = self.meta.header.get_mut(&mut self.storage);
        self.meta.control_data.get_mut(header)
    }

    #[inline]
    pub fn control_frames_mut(&mut self) -> ControlFramesMut<'_> {
        let header = self.meta.header.get_mut(&mut self.storage);
        ControlFramesMut {
            buffer: self.meta.control_data.get_mut(header),
        }
    }

    #[inline]
    pub fn header(&self) -> &[u8] {
        self.meta.header.get(&self.storage)
    }

    #[inline]
    pub fn auth_tag(&self) -> &[u8] {
        self.meta.auth_tag.get(&self.storage)
    }

    /// Create a packet from metadata and storage, validating that the storage is compatible
    #[inline]
    pub fn from_parts(meta: Meta, storage: S) -> Result<Self, (Meta, S)> {
        // Validate storage by attempting to get the ranges
        // This will panic in debug mode if ranges are invalid
        let _ = meta.header.get(&storage);
        let _ = meta.auth_tag.get(&storage);

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

    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// Verify the authentication tag for this control packet using application crypto
    ///
    /// Control packets use KeyPhase::Zero for authentication
    #[inline]
    pub fn verify<O>(&self, opener: &O) -> Result<(), crate::crypto::open::Error>
    where
        O: crate::crypto::open::Application,
    {
        let packet_number = self.packet_number().as_u64();
        let header = self.meta.header.get(&self.storage);
        let tag = self.meta.auth_tag.get(&self.storage);

        // Control packets use KeyPhase::Zero
        use crate::crypto::KeyPhase;
        let key_phase = KeyPhase::Zero;

        // For control packets, the payload is empty (all data is in header)
        // We just need to verify the MAC over the header
        opener.decrypt(
            key_phase,
            packet_number,
            header,
            &[],
            tag,
            crate::crypto::UninitSlice::new(&mut []),
        )
    }

    /// Replace the storage with a new one, validating that it's still valid for the ranges
    #[inline]
    pub fn replace_storage<S2: storage::Bytes>(
        self,
        new_storage: S2,
    ) -> Result<Packet<S2>, (Meta, S2)> {
        // Validate new storage by attempting to get the ranges
        let _ = self.meta.header.get(&new_storage);
        let _ = self.meta.auth_tag.get(&new_storage);

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
        let packet_len = meta.header.len() + meta.auth_tag.len();

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

pub struct ControlFramesMut<'a> {
    buffer: &'a mut [u8],
}

impl<'a> ControlFramesMut<'a> {
    #[inline]
    pub(crate) fn new(buffer: &'a mut [u8]) -> Self {
        Self { buffer }
    }
}

impl<'a> Iterator for ControlFramesMut<'a> {
    type Item = Result<FrameMut<'a>, s2n_codec::DecoderError>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer.is_empty() {
            return None;
        }

        let buffer = unsafe {
            // extend the lifetime of the buffer
            core::mem::transmute::<&mut [u8], &mut [u8]>(self.buffer)
        };
        match DecoderBufferMut::new(buffer).decode::<FrameMut>() {
            Ok((frame, remaining)) => {
                self.buffer = remaining.into_less_safe_slice();
                Some(Ok(frame))
            }
            Err(err) => {
                // clear out the buffer and return an error
                self.buffer = &mut [];
                Some(Err(err))
            }
        }
    }
}
