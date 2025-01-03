// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::{HeaderKey, Key as CryptoKey, ProtectedPayload},
    packet::{
        number::{PacketNumber, PacketNumberLen},
        stateless_reset,
    },
};
use s2n_codec::{encoder::scatter, Encoder, EncoderBuffer, EncoderLenEstimator, EncoderValue};

pub trait PacketPayloadLenCursor: EncoderValue {
    fn new() -> Self;
    fn update(&self, buffer: &mut EncoderBuffer, actual_len: usize);
}

/// used for short packets that don't use a payload len
impl PacketPayloadLenCursor for () {
    #[inline]
    fn new() {}

    #[inline]
    fn update(&self, _buffer: &mut EncoderBuffer, _actual_len: usize) {
        // noop
    }
}

pub trait PacketPayloadEncoder {
    /// Returns an estimate of the encoding size of the payload. This
    /// may be inaccurate from what actually is encoded. Estimates should
    /// be less than or equal to what is actually written.
    /// Implementations can return 0 to skip encoding.
    fn encoding_size_hint<E: Encoder>(&mut self, encoder: &E, minimum_len: usize) -> usize;

    /// Encodes the payload into the buffer. Implementations should ensure
    /// the encoding len is at least the minimum_len, otherwise the packet
    /// writing will panic.
    fn encode(
        &mut self,
        buffer: &mut scatter::Buffer,
        minimum_len: usize,
        header_len: usize,
        tag_len: usize,
    );
}

impl<T: EncoderValue> PacketPayloadEncoder for T {
    #[inline]
    fn encoding_size_hint<E: Encoder>(&mut self, encoder: &E, minimum_len: usize) -> usize {
        let len = self.encoding_size_for_encoder(encoder);
        if len < minimum_len {
            0
        } else {
            len
        }
    }

    #[inline]
    fn encode(
        &mut self,
        buffer: &mut scatter::Buffer,
        _minimum_len: usize,
        _header_len: usize,
        _tag_len: usize,
    ) {
        // the minimum len check is not needed, as it was already performed
        // in encoding_size_hint
        self.encode_mut(buffer);
    }
}

#[derive(Debug)]
pub enum PacketEncodingError<'a> {
    /// The packet number could not be truncated with the
    /// current largest_acknowledged_packet_number
    PacketNumberTruncationError(EncoderBuffer<'a>),

    /// The buffer does not have enough space to hold
    /// the packet encoding.
    InsufficientSpace(EncoderBuffer<'a>),

    /// The payload did not write anything
    EmptyPayload(EncoderBuffer<'a>),

    /// The key used to encrypt the buffer has exceeded the confidentiality limit
    AeadLimitReached(EncoderBuffer<'a>),
}

impl<'a> PacketEncodingError<'a> {
    /// Returns the buffer that experienced an encoding error
    pub fn take_buffer(self) -> EncoderBuffer<'a> {
        match self {
            Self::PacketNumberTruncationError(buffer) => buffer,
            Self::InsufficientSpace(buffer) => buffer,
            Self::EmptyPayload(buffer) => buffer,
            Self::AeadLimitReached(buffer) => buffer,
        }
    }
}

pub trait PacketEncoder<K: CryptoKey, H: HeaderKey, Payload: PacketPayloadEncoder>: Sized {
    type PayloadLenCursor: PacketPayloadLenCursor;

    /// Encodes the current packet's header into the provided encoder
    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E);

    /// Returns the payload for the current packet
    fn payload(&mut self) -> &mut Payload;

    /// Returns the packet number for the current packet
    fn packet_number(&self) -> PacketNumber;

    // Encodes, encrypts, and header-protects a packet into a buffer
    fn encode_packet<'a>(
        mut self,
        key: &mut K,
        header_key: &H,
        largest_acknowledged_packet_number: PacketNumber,
        min_packet_len: Option<usize>,
        mut buffer: EncoderBuffer<'a>,
    ) -> Result<(ProtectedPayload<'a>, EncoderBuffer<'a>), PacketEncodingError<'a>> {
        let packet_number = self.packet_number();

        // Truncate the packet number from the largest_acknowledged_packet_number.
        let truncated_packet_number =
            if let Some(tpn) = packet_number.truncate(largest_acknowledged_packet_number) {
                tpn
            } else {
                return Err(PacketEncodingError::PacketNumberTruncationError(buffer));
            };

        let packet_number_len = truncated_packet_number.len();

        // We need to build an estimate of how large this packet is before writing it
        let mut estimator = EncoderLenEstimator::new(buffer.remaining_capacity());

        // Start by measuring the header len
        self.encode_header(packet_number_len, &mut estimator);

        // Create a payload_len cursor so we can update it with the actual value
        // after writing the payload
        let mut payload_len_cursor = Self::PayloadLenCursor::new();

        // `encode_mut` is called here to initialize the len based on the
        // remaining buffer capacity
        payload_len_cursor.encode_mut(&mut estimator);

        // Save the header_len for later use
        let header_len = estimator.len();

        // Record the truncated_packet_number encoding size
        truncated_packet_number.encode(&mut estimator);

        // Make sure the crypto tag can be written.
        // We want to write this before the payload in the
        // estimator so the payload size hint has an accurate
        // view of remaining capacity.
        estimator.write_repeated(key.tag_len(), 0);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
        //# To achieve that end,
        //# the endpoint SHOULD ensure that all packets it sends are at least 22
        //# bytes longer than the minimum connection ID length that it requests
        //# the peer to include in its packets, adding PADDING frames as
        //# necessary.
        // One additional byte is added so that a stateless reset sent in response to this packet
        // (which is required to be smaller than this packet) is large enough to be
        // indistinguishable from a valid packet.
        let minimum_packet_len = min_packet_len
            .unwrap_or(0)
            .max(stateless_reset::min_indistinguishable_packet_len(key.tag_len()) + 1);

        // Compute how much the payload will need to write to satisfy the
        // minimum_packet_len
        let minimum_payload_len = minimum_packet_len.saturating_sub(estimator.len());

        //= https://www.rfc-editor.org/rfc/rfc9001#section-5.4.2
        //# in sampling packet ciphertext for header protection,
        //# the Packet Number field is assumed to be 4 bytes long

        // Header protection sampling assumes a packet number length of 4 bytes,
        // but the actual packet number may be smaller than that, so we need to ensure
        // there is still enough payload to sample from given the actual packet number length.
        let minimum_payload_len = minimum_payload_len.max(
            PacketNumberLen::MAX_LEN - truncated_packet_number.len().bytesize()
                + header_key.sealing_sample_len(),
        );

        // Try to estimate the payload size - it may be inaccurate
        // but this provides some checks to save writing the packet
        // header
        let estimated_payload_len = self
            .payload()
            .encoding_size_hint(&estimator, minimum_payload_len);

        // The payload is not interested in writing to this packet
        if estimated_payload_len == 0 {
            return Err(PacketEncodingError::EmptyPayload(buffer));
        }

        // Use the estimated_payload_len to check if we're
        // going to have enough room for it.
        estimator.write_repeated(estimated_payload_len, 0);

        // We don't have enough room to write this packet
        if estimator.overflowed() {
            return Err(PacketEncodingError::InsufficientSpace(buffer));
        }

        // After computing the minimum length we actually start writing to the buffer

        // Now we actually encode the header
        self.encode_header(packet_number_len, &mut buffer);

        // Write the estimated payload len. This will be updated
        // with the accurate value after everything is written.
        payload_len_cursor.encode(&mut buffer);

        // Write the packet number
        truncated_packet_number.encode(&mut buffer);

        let (payload_len, inline_len, extra) = {
            // Create a temporary buffer for writing the payload
            let (header_buffer, payload_buffer) = buffer.split_mut();

            // Payloads should not be able to write into the crypto tag space
            let payload_len = payload_buffer.len() - key.tag_len();
            let payload_buffer = EncoderBuffer::new(&mut payload_buffer[..payload_len]);
            let mut payload_buffer = scatter::Buffer::new(payload_buffer);

            // Try to encode the payload into the buffer
            self.payload().encode(
                &mut payload_buffer,
                minimum_payload_len,
                header_buffer.len(),
                key.tag_len(),
            );

            // read how much was written
            let payload_len = payload_buffer.len();

            let (inline_buffer, extra) = payload_buffer.into_inner();

            // record the number of bytes that were written to the inline buffer
            let inline_len = inline_buffer.len();

            (payload_len, inline_len, extra)
        };

        // The payload didn't have anything to write so rewind the cursor
        if payload_len == 0 {
            buffer.set_position(0);
            return Err(PacketEncodingError::EmptyPayload(buffer));
        }

        // Ideally we would check that the `payload_len >= minimum_payload_len`. However, the packet
        // interceptor may rewrite the packet into something smaller. Instead of preventing that
        // here, we will rely on the `crate::transmission::Transmission` logic to ensure the
        // padding is initially written to ensure the minimum is met before interception is applied.

        // Update the payload_len cursor with the actual payload len
        let actual_payload_len = buffer.len() + payload_len + key.tag_len() - header_len;
        payload_len_cursor.update(&mut buffer, actual_payload_len);

        // Advance the buffer cursor by what the payload wrote inline. We'll recreate the scatter
        // buffer with the option extra bytes at the end.
        buffer.advance_position(inline_len);
        let buffer = scatter::Buffer::new_with_extra(buffer, extra);

        // Encrypt the written payload. Note that the tag is appended to the
        // buffer in the `encrypt` function.
        let (encrypted_payload, remaining) =
            crate::crypto::encrypt(key, packet_number, packet_number_len, header_len, buffer)
                .expect("encryption should always work");

        // Protect the packet
        let protected_payload = crate::crypto::protect(header_key, encrypted_payload)
            .expect("header protection should always work");

        // SUCCESS!!!

        Ok((protected_payload, remaining))
    }
}
