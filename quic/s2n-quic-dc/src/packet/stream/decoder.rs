use crate::{
    credentials::Credentials,
    crypto,
    packet::stream::{self, Tag},
};
use s2n_codec::{
    decoder_invariant, u24, CheckedRange, DecoderBufferMut, DecoderBufferMutResult as R,
    DecoderError,
};
use s2n_quic_core::{assume, varint::VarInt};

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
    pub credentials: Credentials,
    pub source_control_port: u16,
    pub source_stream_port: Option<u16>,
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
            credentials: packet.credentials,
            source_control_port: packet.source_control_port,
            source_stream_port: packet.source_stream_port,
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

#[derive(Debug)]
pub struct Packet<'a> {
    tag: Tag,
    credentials: Credentials,
    source_control_port: u16,
    source_stream_port: Option<u16>,
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

impl<'a> Packet<'a> {
    #[inline]
    pub fn tag(&self) -> Tag {
        self.tag
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
    pub fn source_stream_port(&self) -> Option<u16> {
        self.source_stream_port
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
            .map_or(false, |v| *v == 0)
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
    pub fn decrypt<D>(
        &mut self,
        d: &mut D,
        payload_out: &mut crypto::UninitSlice,
    ) -> Result<(), crypto::decrypt::Error>
    where
        D: crypto::decrypt::Key,
    {
        self.remove_retransmit(d);

        let nonce = self.original_packet_number.as_u64();
        let header = &self.header;
        let payload = &self.payload;
        let auth_tag = &self.auth_tag;

        d.decrypt(nonce, header, payload, auth_tag, payload_out)?;

        Ok(())
    }

    #[inline]
    pub fn decrypt_in_place<D>(&mut self, d: &mut D) -> Result<(), crypto::decrypt::Error>
    where
        D: crypto::decrypt::Key,
    {
        self.remove_retransmit(d);

        let nonce = self.original_packet_number.as_u64();
        let header = &self.header;

        let payload_len = self.payload.len();
        let payload_ptr = self.payload.as_mut_ptr();
        let tag_len = self.auth_tag.len();
        let tag_ptr = self.auth_tag.as_mut_ptr();
        let payload_and_tag = unsafe {
            debug_assert_eq!(payload_ptr.add(payload_len), tag_ptr);

            core::slice::from_raw_parts_mut(payload_ptr, payload_len + tag_len)
        };

        d.decrypt_in_place(nonce, header, payload_and_tag)?;

        Ok(())
    }

    #[inline]
    fn remove_retransmit<D>(&mut self, d: &mut D)
    where
        D: crypto::decrypt::Key,
    {
        let original_packet_number = self.original_packet_number.as_u64();
        let retransmission_packet_number = self.packet_number.as_u64();

        if original_packet_number != retransmission_packet_number {
            d.retransmission_tag(
                original_packet_number,
                retransmission_packet_number,
                self.auth_tag,
            );
            let offset = self.retransmission_packet_number_offset as usize;
            let range = offset..offset + 3;
            self.header[range].copy_from_slice(&[0; 3]);
        }
    }

    #[inline]
    #[cfg(debug_assertions)]
    pub fn retransmit<K>(
        buffer: DecoderBufferMut,
        retransmission_packet_number: VarInt,
        key: &mut K,
    ) -> Result<(), DecoderError>
    where
        K: crypto::encrypt::Key,
    {
        let buffer = buffer.into_less_safe_slice();

        let mut before = Self::snapshot(buffer, key.tag_len());
        // the auth tag will have changed so clear it
        before.auth_tag.clear();

        Self::retransmit_impl(
            DecoderBufferMut::new(buffer),
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
        retransmission_packet_number: VarInt,
        key: &mut K,
    ) -> Result<(), DecoderError>
    where
        K: crypto::encrypt::Key,
    {
        Self::retransmit_impl(buffer, retransmission_packet_number, key)
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
        retransmission_packet_number: VarInt,
        key: &mut K,
    ) -> Result<(), DecoderError>
    where
        K: crypto::encrypt::Key,
    {
        unsafe {
            assume!(key.tag_len() >= 16, "tag len needs to be at least 16 bytes");
        }

        let (tag, buffer) = buffer.decode::<Tag>()?;

        let (credentials, buffer) = buffer.decode::<Credentials>()?;

        debug_assert_eq!(&credentials, key.credentials());

        let (_source_control_port, buffer) = buffer.decode::<u16>()?;

        let (_source_stream_port, buffer) = if tag.has_source_stream_port() {
            let (port, buffer) = buffer.decode::<u16>()?;
            (Some(port), buffer)
        } else {
            (None, buffer)
        };

        let (stream_id, buffer) = buffer.decode::<stream::Id>()?;

        decoder_invariant!(
            stream_id.is_reliable,
            "only reliable streams can be retransmitted"
        );

        let (original_packet_number, buffer) = buffer.decode::<VarInt>()?;
        let (retransmission_packet_number_buffer, buffer) = buffer.decode_slice(3)?;
        let retransmission_packet_number_buffer =
            retransmission_packet_number_buffer.into_less_safe_slice();
        let retransmission_packet_number_buffer: &mut [u8; 3] =
            retransmission_packet_number_buffer.try_into().unwrap();

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

        let original_packet_number = original_packet_number.as_u64();

        let relative: u24 = relative
            .as_u64()
            .try_into()
            .map_err(|_| DecoderError::InvariantViolation("packet is too old"))?;

        // undo the previous retransmission if needed
        let prev_value = u24::from_be_bytes(*retransmission_packet_number_buffer);
        if prev_value != u24::ZERO {
            let retransmission_packet_number = original_packet_number + *prev_value as u64;
            key.retransmission_tag(
                original_packet_number,
                retransmission_packet_number,
                auth_tag,
            );
        }

        retransmission_packet_number_buffer.copy_from_slice(&relative.to_be_bytes());

        key.retransmission_tag(
            original_packet_number,
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
            credentials,
            source_control_port,
            source_stream_port,
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

            let (source_control_port, buffer) = buffer.decode()?;

            let (source_stream_port, buffer) = if tag.has_source_stream_port() {
                let (port, buffer) = buffer.decode()?;
                (Some(port), buffer)
            } else {
                (None, buffer)
            };

            let (stream_id, buffer) = buffer.decode::<stream::Id>()?;

            let (original_packet_number, buffer) = buffer.decode::<VarInt>()?;

            let retransmission_packet_number_offset = (start_len - buffer.len()) as u8;
            let (packet_number, buffer) = if stream_id.is_reliable {
                let (rel, buffer) = buffer.decode::<u24>()?;
                let rel = VarInt::from_u32(*rel);
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
                credentials,
                source_control_port,
                source_stream_port,
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
            credentials,
            source_control_port,
            source_stream_port,
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
