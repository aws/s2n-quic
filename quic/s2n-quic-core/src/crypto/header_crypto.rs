// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::{EncryptedPayload, ProtectedPayload},
    packet::number::{PacketNumberSpace, TruncatedPacketNumber},
};
use s2n_codec::{DecoderBuffer, DecoderError};

/// Types for which are able to perform header cryptography.
pub trait HeaderKey: Send {
    /// Derives a header protection mask from a sample buffer, to be
    /// used for opening a packet.
    ///
    /// The sample size is determined by the key function.
    fn opening_header_protection_mask(&self, ciphertext_sample: &'_ [u8]) -> HeaderProtectionMask;

    /// Returns the sample size needed for the header protection
    /// buffer
    fn opening_sample_len(&self) -> usize;

    /// Derives a header protection mask from a sample buffer, to be
    /// used for sealing a packet.
    ///
    /// The sample size is determined by the key function.
    fn sealing_header_protection_mask(&self, ciphertext_sample: &'_ [u8]) -> HeaderProtectionMask;

    /// Returns the sample size needed for the header protection
    /// buffer
    fn sealing_sample_len(&self) -> usize;
}

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.4.1
//# The output of this algorithm is a 5 byte mask that is applied to the
//# protected header fields using exclusive OR.

pub const HEADER_PROTECTION_MASK_LEN: usize = 5;
pub type HeaderProtectionMask = [u8; HEADER_PROTECTION_MASK_LEN];

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.4.1
//# Figure 6 shows a sample algorithm for applying header protection.
//# Removing header protection only differs in the order in which the
//# packet number length (pn_length) is determined (here "^" is used to
//# represent exclusive OR).
//#
//# mask = header_protection(hp_key, sample)
//#
//# pn_length = (packet[0] & 0x03) + 1
//# if (packet[0] & 0x80) == 0x80:
//# # Long header: 4 bits masked
//# packet[0] ^= mask[0] & 0x0f
//# else:
//# # Short header: 5 bits masked
//# packet[0] ^= mask[0] & 0x1f
//#
//# # pn_offset is the start of the Packet Number field.
//# packet[pn_offset:pn_offset+pn_length] ^= mask[1:1+pn_length]

const LONG_HEADER_TAG: u8 = 0x80;
pub(crate) const LONG_HEADER_MASK: u8 = 0x0f;
pub(crate) const SHORT_HEADER_MASK: u8 = 0x1f;

#[inline(always)]
fn mask_from_packet_tag(tag: u8) -> u8 {
    if tag & LONG_HEADER_TAG == LONG_HEADER_TAG {
        LONG_HEADER_MASK
    } else {
        SHORT_HEADER_MASK
    }
}

#[inline(always)]
fn xor_mask(payload: &mut [u8], mask: &[u8]) {
    for (payload_byte, mask_byte) in payload.iter_mut().zip(&mask[1..]) {
        *payload_byte ^= mask_byte;
    }
}

#[inline]
pub(crate) fn apply_header_protection<'a>(
    mask: HeaderProtectionMask,
    payload: EncryptedPayload<'a>,
) -> ProtectedPayload<'a> {
    let header_len = payload.header_len;
    let packet_number_len = payload.packet_number_len;
    let payload = payload.buffer.into_less_safe_slice();

    payload[0] ^= mask[0] & mask_from_packet_tag(payload[0]);

    let header_with_pn_len = packet_number_len.bytesize() + header_len;
    let packet_number_bytes = &mut payload[header_len..header_with_pn_len];
    xor_mask(packet_number_bytes, &mask);

    ProtectedPayload::new(header_len, payload)
}

#[inline]
pub(crate) fn remove_header_protection<'a>(
    space: PacketNumberSpace,
    mask: HeaderProtectionMask,
    payload: ProtectedPayload<'a>,
) -> Result<(TruncatedPacketNumber, EncryptedPayload<'a>), DecoderError> {
    let header_len = payload.header_len;
    let payload = payload.buffer.into_less_safe_slice();

    payload[0] ^= mask[0] & mask_from_packet_tag(payload[0]);
    let packet_number_len = space.new_packet_number_len(payload[0]);

    let header_with_pn_len = packet_number_len.bytesize() + header_len;
    let packet_number = {
        let packet_number_bytes = &mut payload[header_len..header_with_pn_len];
        xor_mask(packet_number_bytes, &mask);

        let (packet_number, _) = packet_number_len
            .decode_truncated_packet_number(DecoderBuffer::new(packet_number_bytes))?;
        packet_number
    };

    Ok((
        packet_number,
        EncryptedPayload::new(header_len, packet_number_len, payload),
    ))
}
