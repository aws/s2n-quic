// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    crypto,
    packet::{
        control::decoder::ControlFramesMut,
        stream::{self, RelativeRetransmissionOffset, Tag},
        WireVersion,
    },
};
use core::{fmt, mem::size_of};
use s2n_codec::{
    decoder_invariant, CheckedRange, DecoderBufferMut, DecoderBufferMutResult as R, DecoderError,
};
use s2n_quic_core::{assume, ensure, varint::VarInt};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owned {
    pub tag: Tag,
    pub wire_version: WireVersion,
    pub credentials: Credentials,
    pub source_queue_id: Option<VarInt>,
    pub stream_id: stream::Id,
    pub original_packet_number: PacketNumber,
    pub packet_number: PacketNumber,
    pub retransmission_packet_number_offset: u8,
    pub next_expected_control_packet: PacketNumber,
    pub stream_offset: VarInt,
    pub final_offset: Option<VarInt>,
    pub application_header: Vec<u8>,
    pub control_data: Vec<u8>,
    pub payload: Vec<u8>,
    pub auth_tag: Vec<u8>,
}

impl<'a> From<Packet<'a>> for Owned {
    fn from(packet: Packet<'a>) -> Self {
        let application_header = packet.application_header().to_vec();
        let control_data = packet.control_data().to_vec();

        Self {
            tag: packet.tag,
            wire_version: packet.wire_version,
            credentials: packet.credentials,
            source_queue_id: packet.source_queue_id,
            stream_id: packet.stream_id,
            original_packet_number: packet.original_packet_number,
            packet_number: packet.packet_number,
            retransmission_packet_number_offset: packet.retransmission_packet_number_offset,
            next_expected_control_packet: packet.next_expected_control_packet,
            stream_offset: packet.stream_offset,
            final_offset: packet.final_offset,
            application_header,
            control_data,
            payload: packet.payload.to_vec(),
            auth_tag: packet.auth_tag.to_vec(),
        }
    }
}

pub struct Packet<'a> {
    tag: Tag,
    wire_version: WireVersion,
    credentials: Credentials,
    source_queue_id: Option<VarInt>,
    stream_id: stream::Id,
    original_packet_number: PacketNumber,
    packet_number: PacketNumber,
    retransmission_packet_number_offset: u8,
    next_expected_control_packet: PacketNumber,
    stream_offset: VarInt,
    final_offset: Option<VarInt>,
    header: &'a mut [u8],
    application_header: CheckedRange,
    control_data: CheckedRange,
    payload: &'a mut [u8],
    auth_tag: &'a mut [u8],
}

impl fmt::Debug for Packet<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("stream::Packet")
            .field("tag", &self.tag)
            .field("wire_version", &self.wire_version)
            .field("credentials", &self.credentials)
            .field("source_queue_id", &self.source_queue_id)
            .field("stream_id", &self.stream_id)
            .field("packet_number", &self.packet_number())
            .field("stream_offset", &self.stream_offset)
            .field("final_offset", &self.final_offset)
            .field("header_len", &self.header.len())
            .field("payload_len", &self.payload.len())
            .field("auth_tag_len", &self.auth_tag.len())
            .finish()
    }
}

impl Packet<'_> {
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
    pub fn stream_id(&self) -> &stream::Id {
        &self.stream_id
    }

    #[inline]
    pub fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    #[inline]
    pub fn is_retransmission(&self) -> bool {
        self.packet_number != self.original_packet_number
    }

    #[inline]
    pub fn next_expected_control_packet(&self) -> PacketNumber {
        self.next_expected_control_packet
    }

    #[inline]
    pub fn stream_offset(&self) -> VarInt {
        self.stream_offset
    }

    #[inline]
    pub fn final_offset(&self) -> Option<VarInt> {
        self.final_offset
    }

    #[inline]
    pub fn is_fin(&self) -> bool {
        self.final_offset()
            .and_then(|offset| offset.checked_sub(self.stream_offset))
            .and_then(|offset| {
                let len = VarInt::try_from(self.payload.len()).ok()?;
                offset.checked_sub(len)
            })
            .is_some_and(|v| *v == 0)
    }

    #[inline]
    pub fn application_header(&self) -> &[u8] {
        self.application_header.get(self.header)
    }

    #[inline]
    pub fn control_data(&self) -> &[u8] {
        self.control_data.get(self.header)
    }

    #[inline]
    pub fn control_frames_mut(&mut self) -> ControlFramesMut<'_> {
        ControlFramesMut::new(self.control_data.get_mut(self.header))
    }

    #[inline]
    pub fn header(&self) -> &[u8] {
        self.header
    }

    #[inline]
    pub fn payload(&self) -> &[u8] {
        self.payload
    }

    #[inline]
    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.payload
    }

    #[inline]
    pub fn total_len(&self) -> usize {
        self.header.len() + self.payload.len() + self.auth_tag.len()
    }

    #[inline]
    pub fn decrypt<D, C>(
        &mut self,
        d: &D,
        c: &C,
        payload_out: &mut crypto::UninitSlice,
    ) -> Result<(), crypto::open::Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
    {
        let key_phase = self.tag.key_phase();
        let space = self.remove_retransmit(c)?;

        let nonce = self.original_packet_number.as_u64();
        let header = &self.header;
        let payload = &self.payload;
        let auth_tag = &self.auth_tag;

        match space {
            stream::PacketSpace::Stream => {
                d.decrypt(key_phase, nonce, header, payload, auth_tag, payload_out)?;
            }
            stream::PacketSpace::Recovery => {
                // recovery/probe packets cannot have payloads
                ensure!(payload.is_empty(), Err(crypto::open::Error::MacOnly));
                c.verify(header, auth_tag)?;
            }
        }

        Ok(())
    }

    #[inline]
    pub fn decrypt_in_place<D, C>(&mut self, d: &D, c: &C) -> Result<(), crypto::open::Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
    {
        let key_phase = self.tag.key_phase();
        let space = self.remove_retransmit(c)?;

        let nonce = self.original_packet_number.as_u64();
        let header = &self.header;

        match space {
            stream::PacketSpace::Stream => {
                let payload_len = self.payload.len();
                let payload_ptr = self.payload.as_mut_ptr();
                let tag_len = self.auth_tag.len();
                let tag_ptr = self.auth_tag.as_mut_ptr();
                let payload_and_tag = unsafe {
                    debug_assert_eq!(payload_ptr.add(payload_len), tag_ptr);

                    core::slice::from_raw_parts_mut(payload_ptr, payload_len + tag_len)
                };
                d.decrypt_in_place(key_phase, nonce, header, payload_and_tag)?;
            }
            stream::PacketSpace::Recovery => {
                // recovery/probe packets cannot have payloads
                ensure!(self.payload.is_empty(), Err(crypto::open::Error::MacOnly));
                c.verify(header, self.auth_tag)?;
            }
        }

        Ok(())
    }

    #[inline]
    fn remove_retransmit<C>(&mut self, c: &C) -> Result<stream::PacketSpace, crypto::open::Error>
    where
        C: crypto::open::control::Stream,
    {
        let space = self.tag.packet_space();
        let original_packet_number = self.original_packet_number;
        let retransmission_packet_number = self.packet_number;

        if original_packet_number != retransmission_packet_number {
            c.retransmission_tag(
                original_packet_number.as_u64(),
                retransmission_packet_number.as_u64(),
                self.auth_tag,
            )?;
            // clear the recovery packet bit, since this is a retransmission
            self.header[0] &= !super::Tag::IS_RECOVERY_PACKET;

            // update the retransmission offset to the zero value
            let offset = self.retransmission_packet_number_offset as usize;
            let range = offset..offset + size_of::<RelativeRetransmissionOffset>();
            self.header[range].copy_from_slice(&[0; size_of::<RelativeRetransmissionOffset>()]);

            Ok(stream::PacketSpace::Stream)
        } else {
            Ok(space)
        }
    }

    #[inline]
    #[cfg(debug_assertions)]
    pub fn retransmit<K>(
        buffer: DecoderBufferMut,
        space: stream::PacketSpace,
        retransmission_packet_number: VarInt,
        key: &K,
    ) -> Result<(), DecoderError>
    where
        K: crypto::seal::control::Stream,
    {
        let buffer = buffer.into_less_safe_slice();

        let mut before = Self::snapshot(buffer, key.tag_len());
        // update the expected packet space with the new one
        before.tag.set_packet_space(space);
        // the auth tag will have changed so clear it
        before.auth_tag.clear();

        Self::retransmit_impl(
            DecoderBufferMut::new(buffer),
            space,
            retransmission_packet_number,
            key,
        )?;

        let mut after = Self::snapshot(buffer, key.tag_len());
        assert_eq!(after.packet_number, retransmission_packet_number);
        after.packet_number = before.packet_number;
        // the auth tag will have changed so clear it
        after.auth_tag.clear();

        assert_eq!(before, after);

        Ok(())
    }

    #[inline]
    #[cfg(not(debug_assertions))]
    pub fn retransmit<K>(
        buffer: DecoderBufferMut,
        space: stream::PacketSpace,
        retransmission_packet_number: VarInt,
        key: &K,
    ) -> Result<(), DecoderError>
    where
        K: crypto::seal::control::Stream,
    {
        Self::retransmit_impl(buffer, space, retransmission_packet_number, key)
    }

    #[inline]
    #[cfg(debug_assertions)]
    fn snapshot(buffer: &mut [u8], crypto_tag_len: usize) -> Owned {
        let buffer = DecoderBufferMut::new(buffer);
        let (packet, _buffer) = Self::decode(buffer, (), crypto_tag_len).unwrap();
        packet.into()
    }

    #[inline(always)]
    fn retransmit_impl<K>(
        buffer: DecoderBufferMut,
        space: stream::PacketSpace,
        retransmission_packet_number: VarInt,
        key: &K,
    ) -> Result<(), DecoderError>
    where
        K: crypto::seal::control::Stream,
    {
        unsafe {
            assume!(key.tag_len() >= 16, "tag len needs to be at least 16 bytes");
        }

        let (tag_slice, buffer) = buffer.decode_slice(1)?;

        let tag: super::Tag = {
            let tag_slice = tag_slice.into_less_safe_slice();

            match space {
                stream::PacketSpace::Stream => {
                    tag_slice[0] &= !super::Tag::IS_RECOVERY_PACKET;
                }
                stream::PacketSpace::Recovery => {
                    tag_slice[0] |= super::Tag::IS_RECOVERY_PACKET;
                }
            }

            let tag_slice = DecoderBufferMut::new(tag_slice);
            let (tag, _) = tag_slice.decode()?;
            tag
        };

        let (_credentials, buffer) = buffer.decode::<Credentials>()?;
        let (_wire_version, buffer) = buffer.decode::<WireVersion>()?;

        let (_source_control_port, buffer) = buffer.decode::<u16>()?;

        let (stream_id, buffer) = buffer.decode::<stream::Id>()?;

        decoder_invariant!(
            stream_id.is_reliable,
            "only reliable streams can be retransmitted"
        );

        let (_source_queue_id, buffer) = if tag.has_source_queue_id() {
            let (v, buffer) = buffer.decode::<VarInt>()?;
            (Some(v), buffer)
        } else {
            (None, buffer)
        };

        let (original_packet_number, buffer) = buffer.decode::<VarInt>()?;
        let (retransmission_packet_number_buffer, buffer) =
            buffer.decode_slice(size_of::<RelativeRetransmissionOffset>())?;
        let retransmission_packet_number_buffer =
            retransmission_packet_number_buffer.into_less_safe_slice();
        let retransmission_packet_number_buffer: &mut [u8;
                 size_of::<RelativeRetransmissionOffset>(
            )] = retransmission_packet_number_buffer.try_into().unwrap();

        let (_next_expected_control_packet, buffer) = buffer.decode::<VarInt>()?;
        let (_stream_offset, buffer) = buffer.decode::<VarInt>()?;

        let auth_tag_offset = buffer
            .len()
            .checked_sub(key.tag_len())
            .ok_or(DecoderError::InvariantViolation("missing auth tag"))?;
        let buffer = buffer.skip(auth_tag_offset)?;
        let auth_tag = buffer.into_less_safe_slice();

        let relative = retransmission_packet_number
            .checked_sub(original_packet_number)
            .ok_or(DecoderError::InvariantViolation(
                "invalid retransmission packet number",
            ))?;

        let relative: RelativeRetransmissionOffset = relative
            .as_u64()
            .try_into()
            .map_err(|_| DecoderError::InvariantViolation("packet is too old"))?;

        // undo the previous retransmission if needed
        let prev_value =
            RelativeRetransmissionOffset::from_be_bytes(*retransmission_packet_number_buffer);
        if prev_value != 0 {
            let retransmission_packet_number =
                original_packet_number + VarInt::from_u32(prev_value);
            key.retransmission_tag(
                original_packet_number.as_u64(),
                retransmission_packet_number.as_u64(),
                auth_tag,
            );
        }

        retransmission_packet_number_buffer.copy_from_slice(&relative.to_be_bytes());

        key.retransmission_tag(
            original_packet_number.as_u64(),
            retransmission_packet_number.as_u64(),
            auth_tag,
        );

        Ok(())
    }

    #[inline(always)]
    pub fn decode<V: Validator>(
        buffer: DecoderBufferMut,
        mut validator: V,
        crypto_tag_len: usize,
    ) -> R<Packet> {
        let (
            tag,
            wire_version,
            credentials,
            source_queue_id,
            stream_id,
            original_packet_number,
            packet_number,
            retransmission_packet_number_offset,
            next_expected_control_packet,
            stream_offset,
            final_offset,
            header_len,
            total_header_len,
            application_header_len,
            control_data_len,
            payload_len,
        ) = {
            let buffer = buffer.peek();

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

            // unused space - was source_control_port when we did port migration but that has
            // been replaced with `source_queue_id`, which is more flexible
            let (_source_control_port, buffer) = buffer.decode::<u16>()?;

            let (stream_id, buffer) = buffer.decode::<stream::Id>()?;

            let (source_queue_id, buffer) = if tag.has_source_queue_id() {
                let (v, buffer) = buffer.decode()?;
                (Some(v), buffer)
            } else {
                (None, buffer)
            };

            let (original_packet_number, buffer) = buffer.decode::<VarInt>()?;

            let retransmission_packet_number_offset = (start_len - buffer.len()) as u8;
            let (packet_number, buffer) = if stream_id.is_reliable {
                let (rel, buffer) = buffer.decode::<RelativeRetransmissionOffset>()?;
                let rel = VarInt::from_u32(rel);
                let pn = original_packet_number.checked_add(rel).ok_or(
                    DecoderError::InvariantViolation("retransmission packet number overflow"),
                )?;
                (pn, buffer)
            } else {
                (original_packet_number, buffer)
            };

            let (next_expected_control_packet, buffer) = buffer.decode()?;
            let (stream_offset, buffer) = buffer.decode()?;
            let (final_offset, buffer) = if tag.has_final_offset() {
                let (final_offset, buffer) = buffer.decode()?;
                (Some(final_offset), buffer)
            } else {
                (None, buffer)
            };
            let (control_data_len, buffer) = if tag.has_control_data() {
                buffer.decode()?
            } else {
                (VarInt::ZERO, buffer)
            };
            let (payload_len, buffer) = buffer.decode::<VarInt>()?;

            let (application_header_len, buffer) = if tag.has_application_header() {
                let (application_header_len, buffer) = buffer.decode::<VarInt>()?;
                ((*application_header_len) as usize, buffer)
            } else {
                (0, buffer)
            };

            let header_len = start_len - buffer.len();

            let buffer = buffer.skip(application_header_len)?;
            let buffer = buffer.skip(*control_data_len as _)?;

            let total_header_len = start_len - buffer.len();

            let buffer = buffer.skip(*payload_len as _)?;
            let buffer = buffer.skip(crypto_tag_len)?;

            let _ = buffer;

            (
                tag,
                wire_version,
                credentials,
                source_queue_id,
                stream_id,
                original_packet_number,
                packet_number,
                retransmission_packet_number_offset,
                next_expected_control_packet,
                stream_offset,
                final_offset,
                header_len,
                total_header_len,
                application_header_len,
                control_data_len,
                payload_len,
            )
        };

        unsafe {
            assume!(buffer.len() >= total_header_len);
        }
        let (header, buffer) = buffer.decode_slice(total_header_len)?;

        let (application_header, control_data) = {
            let buffer = header.peek();
            unsafe {
                assume!(buffer.len() >= header_len);
            }
            let buffer = buffer.skip(header_len)?;
            unsafe {
                assume!(buffer.len() >= application_header_len);
            }
            let (application_header, buffer) =
                buffer.skip_into_range(application_header_len, &header)?;
            unsafe {
                assume!(buffer.len() >= *control_data_len as usize);
            }
            let (control_data, _) = buffer.skip_into_range(*control_data_len as usize, &header)?;

            (application_header, control_data)
        };
        let header = header.into_less_safe_slice();

        let (payload, buffer) = buffer.decode_slice(*payload_len as usize)?;
        let payload = payload.into_less_safe_slice();

        let (auth_tag, buffer) = buffer.decode_slice(crypto_tag_len)?;
        let auth_tag = auth_tag.into_less_safe_slice();

        let packet = Packet {
            tag,
            wire_version,
            credentials,
            source_queue_id,
            stream_id,
            original_packet_number,
            packet_number,
            retransmission_packet_number_offset,
            next_expected_control_packet,
            stream_offset,
            final_offset,
            header,
            application_header,
            control_data,
            payload,
            auth_tag,
        };

        Ok((packet, buffer))
    }
}
