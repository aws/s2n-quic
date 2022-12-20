// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    connection::id::ConnectionInfo,
    crypto::ProtectedPayload,
    packet::{
        long::{
            validate_destination_connection_id_range, validate_source_connection_id_range,
            DestinationConnectionIdLen, SourceConnectionIdLen, Version,
        },
        number::ProtectedPacketNumber,
        Tag,
    },
    varint::VarInt,
};
use core::{convert::TryInto, mem::size_of};
use s2n_codec::{CheckedRange, DecoderBuffer, DecoderBufferMut, DecoderError, DecoderValue};

pub struct HeaderDecoder<'a> {
    initial_buffer_len: usize,
    peek: DecoderBuffer<'a>,
}

impl<'a> HeaderDecoder<'a> {
    pub fn new_long<'b>(buffer: &'a DecoderBufferMut<'b>) -> Self {
        let initial_buffer_len = buffer.len();
        let peek = buffer.peek();
        let peek = peek
            .skip(size_of::<Tag>() + size_of::<Version>())
            .expect("tag and version already verified");
        Self {
            initial_buffer_len,
            peek,
        }
    }

    pub fn new_short<'b>(buffer: &'a DecoderBufferMut<'b>) -> Self {
        let initial_buffer_len = buffer.len();
        let peek = buffer.peek();
        let peek = peek.skip(size_of::<Tag>()).expect("tag already verified");
        Self {
            initial_buffer_len,
            peek,
        }
    }

    pub fn decode_destination_connection_id(
        &mut self,
        buffer: &DecoderBufferMut<'_>,
    ) -> Result<CheckedRange, DecoderError> {
        let destination_connection_id =
            self.decode_checked_range::<DestinationConnectionIdLen>(buffer)?;
        validate_destination_connection_id_range(&destination_connection_id)?;
        Ok(destination_connection_id)
    }

    pub fn decode_short_destination_connection_id<Validator: connection::id::Validator>(
        &mut self,
        buffer: &DecoderBufferMut<'_>,
        connection_info: &ConnectionInfo,
        connection_id_validator: &Validator,
    ) -> Result<CheckedRange, DecoderError> {
        let destination_connection_id_len = if let Some(len) = connection_id_validator
            .validate(connection_info, self.peek.peek().into_less_safe_slice())
        {
            len
        } else {
            return Err(DecoderError::InvariantViolation("invalid connection id"));
        };

        let (destination_connection_id, peek) = self
            .peek
            .skip_into_range(destination_connection_id_len, buffer)?;
        self.peek = peek;
        validate_destination_connection_id_range(&destination_connection_id)?;
        Ok(destination_connection_id)
    }

    pub fn decode_source_connection_id(
        &mut self,
        buffer: &DecoderBufferMut<'_>,
    ) -> Result<CheckedRange, DecoderError> {
        let source_connection_id = self.decode_checked_range::<SourceConnectionIdLen>(buffer)?;
        validate_source_connection_id_range(&source_connection_id)?;
        Ok(source_connection_id)
    }

    pub fn decode_checked_range<Len: DecoderValue<'a> + TryInto<usize>>(
        &mut self,
        buffer: &DecoderBufferMut<'_>,
    ) -> Result<CheckedRange, DecoderError> {
        let (value, peek) = self.peek.skip_into_range_with_len_prefix::<Len>(buffer)?;
        self.peek = peek;
        Ok(value)
    }

    pub fn finish_long(mut self) -> Result<HeaderDecoderResult, DecoderError> {
        let (payload_len, peek) = self.peek.decode::<VarInt>()?;
        self.peek = peek;
        let header_len = self.decoded_len();

        self.peek = peek.skip(*payload_len as usize)?;
        let packet_len = self.decoded_len();

        Ok(HeaderDecoderResult {
            packet_len,
            header_len,
        })
    }

    pub fn finish_short(self) -> Result<HeaderDecoderResult, DecoderError> {
        let header_len = self.decoded_len();
        let packet_len = self.initial_buffer_len;

        Ok(HeaderDecoderResult {
            packet_len,
            header_len,
        })
    }

    pub fn decoded_len(&self) -> usize {
        self.initial_buffer_len - self.peek.len()
    }
}

#[derive(Debug)]
pub struct HeaderDecoderResult {
    pub packet_len: usize,
    pub header_len: usize,
}

impl HeaderDecoderResult {
    pub fn split_off_packet<'a>(
        &self,
        buffer: DecoderBufferMut<'a>,
    ) -> Result<
        (
            ProtectedPayload<'a>,
            ProtectedPacketNumber,
            DecoderBufferMut<'a>,
        ),
        DecoderError,
    > {
        let (payload, remaining) = buffer.decode_slice(self.packet_len)?;
        let packet_number = ProtectedPacketNumber::default();
        let payload = ProtectedPayload::new(self.header_len, payload.into_less_safe_slice());

        Ok((payload, packet_number, remaining))
    }
}
