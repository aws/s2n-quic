use super::Nonce;
use crate::crypto::encrypt;
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::assume;

#[inline]
pub fn finish<C>(mut encoder: EncoderBuffer, nonce: Nonce, crypto: &mut C) -> usize
where
    C: encrypt::Key,
{
    let header_offset = encoder.len();

    encoder.advance_position(crypto.tag_len());

    let packet_len = encoder.len();

    let slice = encoder.as_mut_slice();
    let (header, payload_and_tag) = unsafe {
        assume!(slice.len() >= header_offset);
        slice.split_at_mut(header_offset)
    };

    crypto.encrypt(nonce, header, None, payload_and_tag);

    packet_len
}
