// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    packet::{datagram::Tag, WireVersion},
};
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

pub struct Packet<'a> {
    tag: Tag,
    wire_version: WireVersion,
    credentials: Credentials,
    source_control_port: u16,
    packet_number: PacketNumber,
    next_expected_control_packet: Option<PacketNumber>,
    header: &'a mut [u8],
    application_header: CheckedRange,
    control_data: CheckedRange,
    payload: &'a mut [u8],
    auth_tag: &'a mut [u8],
}

impl std::fmt::Debug for Packet<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Packet")
            .field("tag", &self.tag)
            .field("wire_version", &self.wire_version)
            .field("credentials", &self.credentials)
            .field("source_control_port", &self.source_control_port)
            .field("packet_number", &self.packet_number)
            .field(
                "next_expected_control_packet",
                &self.next_expected_control_packet,
            )
            .field("header", &self.header)
            .field("application_header", &self.application_header)
            .field("control_data", &self.control_data)
            .field("payload_len", &self.payload.len())
            .field("auth_tag", &self.auth_tag)
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
    pub fn source_control_port(&self) -> u16 {
        self.source_control_port
    }

    #[inline]
    pub fn crypto_nonce(&self) -> u64 {
        self.packet_number.as_u64()
    }

    #[inline]
    pub fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    #[inline]
    pub fn next_expected_control_packet(&self) -> Option<PacketNumber> {
        self.next_expected_control_packet
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
    pub fn auth_tag(&self) -> &[u8] {
        self.auth_tag
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
            source_control_port,
            packet_number,
            next_expected_control_packet,
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

            let (source_control_port, buffer) = buffer.decode()?;

            let (packet_number, buffer) = if tag.is_connected() || tag.ack_eliciting() {
                buffer.decode()?
            } else {
                (VarInt::ZERO, buffer)
            };

            let (payload_len, buffer) = buffer.decode::<VarInt>()?;
            let payload_len = (*payload_len) as usize;

            let (next_expected_control_packet, control_data_len, buffer) = if tag.ack_eliciting() {
                let (packet_number, buffer) = buffer.decode::<VarInt>()?;
                let (control_data_len, buffer) = buffer.decode::<VarInt>()?;
                (Some(packet_number), (*control_data_len) as usize, buffer)
            } else {
                (None, 0usize, buffer)
            };

            let (application_header_len, buffer) = if tag.has_application_header() {
                let (application_header_len, buffer) = buffer.decode::<VarInt>()?;
                ((*application_header_len) as usize, buffer)
            } else {
                (0, buffer)
            };

            let header_len = start_len - buffer.len();

            let buffer = buffer.skip(application_header_len)?;

            let buffer = buffer.skip(control_data_len)?;

            let total_header_len = start_len - buffer.len();

            (
                tag,
                wire_version,
                credentials,
                source_control_port,
                packet_number,
                next_expected_control_packet,
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
                assume!(buffer.len() >= control_data_len);
            }
            let (control_data, _) = buffer.skip_into_range(control_data_len, &header)?;

            (application_header, control_data)
        };
        let header = header.into_less_safe_slice();

        let (payload, buffer) = buffer.decode_slice(payload_len)?;
        let payload = payload.into_less_safe_slice();

        let (auth_tag, buffer) = buffer.decode_slice(crypto_tag_len)?;
        let auth_tag = auth_tag.into_less_safe_slice();

        let packet = Packet {
            tag,
            wire_version,
            credentials,
            source_control_port,
            packet_number,
            next_expected_control_packet,
            header,
            application_header,
            control_data,
            payload,
            auth_tag,
        };

        Ok((packet, buffer))
    }
}
