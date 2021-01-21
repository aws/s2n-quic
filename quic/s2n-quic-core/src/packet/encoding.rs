use crate::{
    connection,
    crypto::{HeaderCrypto, Key as CryptoKey, ProtectedPayload},
    packet::number::{PacketNumber, PacketNumberLen},
};
use s2n_codec::{Encoder, EncoderBuffer, EncoderLenEstimator, EncoderValue};

pub trait PacketPayloadLenCursor: EncoderValue {
    fn new() -> Self;
    fn update(&self, buffer: &mut EncoderBuffer, actual_len: usize);
}

/// used for short packets that don't use a payload len
impl PacketPayloadLenCursor for () {
    fn new() {}

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
    fn encode(&mut self, buffer: &mut EncoderBuffer, minimum_len: usize, overhead_len: usize);
}

impl<T: EncoderValue> PacketPayloadEncoder for T {
    fn encoding_size_hint<E: Encoder>(&mut self, encoder: &E, minimum_len: usize) -> usize {
        let len = self.encoding_size_for_encoder(encoder);
        if len < minimum_len {
            0
        } else {
            len
        }
    }

    fn encode(&mut self, buffer: &mut EncoderBuffer, _minimum_len: usize, _overhead_len: usize) {
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
}

impl<'a> PacketEncodingError<'a> {
    /// Returns the buffer that experienced an encoding error
    pub fn take_buffer(self) -> EncoderBuffer<'a> {
        match self {
            Self::PacketNumberTruncationError(buffer) => buffer,
            Self::InsufficientSpace(buffer) => buffer,
            Self::EmptyPayload(buffer) => buffer,
        }
    }
}

pub trait PacketEncoder<Crypto: HeaderCrypto + CryptoKey, Payload: PacketPayloadEncoder>:
    Sized
{
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
        crypto: &Crypto,
        largest_acknowledged_packet_number: PacketNumber,
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
        estimator.write_repeated(crypto.tag_len(), 0);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
        //# To achieve that end,
        //# the endpoint SHOULD ensure that all packets it sends are at least 22
        //# bytes longer than the minimum connection ID length that it requests
        //# the peer to include in its packets, adding PADDING frames as
        //# necessary.
        // This is derived from the requirements of packet protection sampling and stateless reset.
        // Since the connection ID length is determined by a provider, connection::id::MAX_LEN is
        // used to ensure all packets are large enough such that a stateless reset sent in response
        // is indistinguishable from a valid packet regardless of the connection ID length the
        // provider uses. Two additional bytes are added so that a stateless reset sent in response
        // is large enough to be indistinguishable from a packet with the minimum payload size of
        // 1 byte.
        let minimum_packet_len = header_len
            + PacketNumberLen::MAX_LEN
            + crypto.sealing_sample_len()
            + connection::id::MAX_LEN
            + 2;

        // Compute how much the payload will need to write to satisfy the
        // minimum_packet_len
        let minimum_payload_len = minimum_packet_len.saturating_sub(buffer.len());

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

        let payload_len = {
            // Create a temporary buffer for writing the payload
            let (header_buffer, payload_buffer) = buffer.split_mut();

            let overhead_len = header_buffer.len() + crypto.tag_len();

            // Payloads should not be able to write into the crypto tag space
            let payload_len = payload_buffer.len() - crypto.tag_len();
            let mut payload_buffer = EncoderBuffer::new(&mut payload_buffer[0..payload_len]);

            // Try to encode the payload into the buffer
            self.payload()
                .encode(&mut payload_buffer, minimum_payload_len, overhead_len);

            // read how much was written
            payload_buffer.len()
        };

        // The payload didn't have anything to write so rewind the cursor
        if payload_len == 0 {
            buffer.set_position(0);
            return Err(PacketEncodingError::EmptyPayload(buffer));
        }

        debug_assert!(
            payload_len >= minimum_payload_len,
            "payloads should write at least the minimum_len"
        );

        // Advance the buffer cursor by what the payload wrote
        buffer.advance_position(payload_len);

        // Update the payload_len cursor with the actual payload len
        let actual_payload_len = buffer.len() + crypto.tag_len() - header_len;
        payload_len_cursor.update(&mut buffer, actual_payload_len);

        // Encrypt the written payload. Note that the tag is appended to the
        // buffer in the `encrypt` function.
        let (encrypted_payload, remaining) =
            crate::crypto::encrypt(crypto, packet_number, packet_number_len, header_len, buffer)
                .expect("encryption should always work");

        // Protect the packet
        let protected_payload = crate::crypto::protect(crypto, encrypted_payload)
            .expect("header protection should always work");

        // SUCCESS!!!

        Ok((protected_payload, remaining))
    }
}
