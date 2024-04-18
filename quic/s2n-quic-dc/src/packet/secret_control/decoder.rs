// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_codec::{
    DecoderBuffer, DecoderBufferMut, DecoderBufferMutResult as Rm, DecoderBufferResult as R,
    DecoderError, DecoderValue,
};
use s2n_quic_core::varint::VarInt;

macro_rules! impl_packet {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug)]
        pub struct Packet<'a> {
            header: &'a [u8],
            value: $name,
            crypto_tag: &'a [u8],
        }

        impl<'a> Packet<'a> {
            #[inline]
            pub fn decode(buffer: DecoderBufferMut<'a>, crypto_tag_len: usize) -> Rm<Packet> {
                let header_len = decoder::header_len::<$name>(buffer.peek())?;
                let ((header, value, crypto_tag), buffer) =
                    decoder::header(buffer, header_len, crypto_tag_len)?;
                let packet = Self {
                    header,
                    value,
                    crypto_tag,
                };
                Ok((packet, buffer))
            }

            #[inline]
            pub fn credential_id(&self) -> &crate::credentials::Id {
                &self.value.credential_id
            }

            #[inline]
            pub fn authenticate<C>(&self, crypto: &mut C) -> Option<&$name>
            where
                C: decrypt::Key,
            {
                let Self {
                    header,
                    value,
                    crypto_tag,
                } = self;

                crypto
                    .decrypt(
                        value.nonce(),
                        header,
                        &[],
                        crypto_tag,
                        bytes::buf::UninitSlice::new(&mut []),
                    )
                    .ok()?;

                Some(value)
            }
        }
    };
}

#[inline]
pub fn header_len<'a, T>(buffer: DecoderBuffer<'a>) -> Result<usize, DecoderError>
where
    T: DecoderValue<'a>,
{
    let before_len = buffer.len();
    let (_, buffer) = buffer.decode::<T>()?;
    Ok(before_len - buffer.len())
}

#[inline]
pub fn header<'a, T>(
    buffer: DecoderBufferMut<'a>,
    header_len: usize,
    crypto_tag_len: usize,
) -> Rm<'a, (&[u8], T, &[u8])>
where
    T: DecoderValue<'a>,
{
    let (header, buffer) = buffer.decode_slice(header_len)?;
    let header = header.freeze();
    let (value, _) = header.decode::<T>()?;
    let header = header.into_less_safe_slice();

    let (crypto_tag, buffer) = buffer.decode_slice(crypto_tag_len)?;
    let crypto_tag = crypto_tag.into_less_safe_slice();

    Ok(((header, value, crypto_tag), buffer))
}

#[inline]
pub fn sized<T>(buffer: DecoderBuffer) -> R<T>
where
    VarInt: TryInto<T>,
{
    let (value, buffer) = buffer.decode::<VarInt>()?;
    let value = value
        .try_into()
        .map_err(|_| DecoderError::InvariantViolation("value overflow"))?;
    Ok((value, buffer))
}
