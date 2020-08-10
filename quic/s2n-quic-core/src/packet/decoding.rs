use crate::{
    crypto::ProtectedPayload,
    packet::{
        long::{
            validate_destination_connection_id_range, validate_source_connection_id_range,
            validate_token, DestinationConnectionIDLen, SourceConnectionIDLen, Version,
        },
        number::ProtectedPacketNumber,
        DestinationConnectionIDDecoder, Tag, TokenDecoder,
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

    pub fn decode_destination_connection_id<'b>(
        &mut self,
        buffer: &DecoderBufferMut<'b>,
    ) -> Result<CheckedRange, DecoderError> {
        let destination_connection_id =
            self.decode_checked_range::<DestinationConnectionIDLen>(buffer)?;
        validate_destination_connection_id_range(&destination_connection_id)?;
        Ok(destination_connection_id)
    }

    pub fn decode_short_destination_connection_id<'b, DCID: DestinationConnectionIDDecoder>(
        &mut self,
        buffer: &DecoderBufferMut<'b>,
        destination_connection_id_decoder: DCID,
    ) -> Result<CheckedRange, DecoderError> {
        let (destination_connection_id_len, _) =
            destination_connection_id_decoder.len(self.peek.peek())?;

        let (destination_connection_id, peek) = self
            .peek
            .skip_into_range(destination_connection_id_len, buffer)?;
        self.peek = peek;
        validate_destination_connection_id_range(&destination_connection_id)?;
        Ok(destination_connection_id)
    }

    pub fn decode_source_connection_id<'b>(
        &mut self,
        buffer: &DecoderBufferMut<'b>,
    ) -> Result<CheckedRange, DecoderError> {
        let source_connection_id = self.decode_checked_range::<SourceConnectionIDLen>(buffer)?;
        validate_source_connection_id_range(&source_connection_id)?;
        Ok(source_connection_id)
    }

    pub fn decode_token<'b, TD: TokenDecoder>(
        &mut self,
        buffer: &DecoderBufferMut<'b>,
        token_decoder: TD,
    ) -> Result<CheckedRange, DecoderError> {
        let (token_len, _) = token_decoder.len(self.peek.peek())?;

        let (token, peek) = self.peek.skip_into_range(token_len, buffer)?;
        self.peek = peek;
        validate_token(&token)?;
        Ok(token)
    }

    pub fn decode_checked_range<'b, Len: DecoderValue<'a> + TryInto<usize>>(
        &mut self,
        buffer: &DecoderBufferMut<'b>,
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
            header_len,
            packet_len,
        })
    }

    pub fn finish_short(self) -> Result<HeaderDecoderResult, DecoderError> {
        let header_len = self.decoded_len();
        let packet_len = self.initial_buffer_len;

        Ok(HeaderDecoderResult {
            header_len,
            packet_len,
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
