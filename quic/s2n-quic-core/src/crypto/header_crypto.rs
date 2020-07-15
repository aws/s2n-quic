use crate::{
    crypto::{EncryptedPayload, ProtectedPayload},
    packet::number::{PacketNumberSpace, TruncatedPacketNumber},
};
use s2n_codec::{DecoderBuffer, DecoderError};

//= https://tools.ietf.org/id/draft-ietf-quic-tls-22.txt#5.4
//#    Parts of QUIC packet headers, in particular the Packet Number field,
//#    are protected using a key that is derived separate to the packet
//#    protection key and IV.  The key derived using the "quic hp" label is
//#    used to provide confidentiality protection for those fields that are
//#    not exposed to on-path elements.
//#
//#    This protection applies to the least-significant bits of the first
//#    byte, plus the Packet Number field.  The four least-significant bits
//#    of the first byte are protected for packets with long headers; the
//#    five least significant bits of the first byte are protected for
//#    packets with short headers.  For both header forms, this covers the
//#    reserved bits and the Packet Number Length field; the Key Phase bit
//#    is also protected for packets with a short header.
//#
//#    The same header protection key is used for the duration of the
//#    connection, with the value not changing after a key update (see
//#    Section 6).  This allows header protection to be used to protect the
//#    key phase.
//#
//#    This process does not apply to Retry or Version Negotiation packets,
//#    which do not contain a protected payload or any of the fields that
//#    are protected by this process.

/// Types for which are able to perform header cryptography.
pub trait HeaderCrypto: Send {
    /// Derives a header protection mask from a sample buffer, to be
    /// used for opening a packet.
    ///
    /// The sample size is determined by the key function.
    fn opening_header_protection_mask(&self, ciphertext_sample: &[u8]) -> HeaderProtectionMask;

    /// Returns the sample size needed for the header protection
    /// buffer
    fn opening_sample_len(&self) -> usize;

    /// Derives a header protection mask from a sample buffer, to be
    /// used for sealing a packet.
    ///
    /// The sample size is determined by the key function.
    fn sealing_header_protection_mask(&self, ciphertext_sample: &[u8]) -> HeaderProtectionMask;

    /// Returns the sample size needed for the header protection
    /// buffer
    fn sealing_sample_len(&self) -> usize;
}

//= https://tools.ietf.org/id/draft-ietf-quic-tls-22.txt#5.4.1
//#    Header protection is applied after packet protection is applied (see
//#    Section 5.3).  The ciphertext of the packet is sampled and used as
//#    input to an encryption algorithm.  The algorithm used depends on the
//#    negotiated AEAD.
//#
//#    The output of this algorithm is a 5 byte mask which is applied to the
//#    protected header fields using exclusive OR.  The least significant
//#    bits of the first byte of the packet are masked by the least
//#    significant bits of the first mask byte, and the packet number is
//#    masked with the remaining bytes.  Any unused bytes of mask that might
//#    result from a shorter packet number encoding are unused.

pub const HEADER_PROTECTION_MASK_LEN: usize = 5;
pub type HeaderProtectionMask = [u8; HEADER_PROTECTION_MASK_LEN];

//= https://tools.ietf.org/id/draft-ietf-quic-tls-22.txt#5.4.1
//#    Figure 4 shows a sample algorithm for applying header protection.
//#    Removing header protection only differs in the order in which the
//#    packet number length (pn_length) is determined.
//#
//#    mask = header_protection(hp_key, sample)
//#
//#    pn_length = (packet[0] & 0x03) + 1
//#    if (packet[0] & 0x80) == 0x80:
//#       # Long header: 4 bits masked
//#       packet[0] ^= mask[0] & 0x0f
//#    else:
//#       # Short header: 5 bits masked
//#       packet[0] ^= mask[0] & 0x1f
//#
//#    # pn_offset is the start of the Packet Number field.
//#    packet[pn_offset:pn_offset+pn_length] ^= mask[1:1+pn_length]

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

pub(crate) fn apply_header_protection(
    mask: HeaderProtectionMask,
    payload: EncryptedPayload,
) -> Result<ProtectedPayload, DecoderError> {
    let header_len = payload.header_len;
    let packet_number_len = payload.packet_number_len;
    let payload = payload.buffer.into_less_safe_slice();

    payload[0] ^= mask[0] & mask_from_packet_tag(payload[0]);

    let header_with_pn_len = packet_number_len.bytesize() + header_len;
    let packet_number_bytes = &mut payload[header_len..header_with_pn_len];
    xor_mask(packet_number_bytes, &mask);

    Ok(ProtectedPayload::new(header_len, payload))
}

pub(crate) fn remove_header_protection(
    space: PacketNumberSpace,
    mask: HeaderProtectionMask,
    payload: ProtectedPayload,
) -> Result<(TruncatedPacketNumber, EncryptedPayload), DecoderError> {
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

//= https://tools.ietf.org/id/draft-ietf-quic-tls-22.txt#5.4.1
//#                   Figure 4: Header Protection Pseudocode
//#
//#    Figure 5 shows the protected fields of long and short headers marked
//#    with an E.  Figure 5 also shows the sampled fields.
//#
//#    Long Header:
//#    +-+-+-+-+-+-+-+-+
//#    |1|1|T T|E E E E|
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                    Version -> Length Fields                 ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+

const LONG_HEADER_TAG: u8 = 0b1000_0000;
const LONG_HEADER_MASK: u8 = 0b1111;

//= https://tools.ietf.org/id/draft-ietf-quic-tls-22.txt#5.4.1
//#    Short Header:
//#    +-+-+-+-+-+-+-+-+
//#    |0|1|S|E E E E E|
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |               Destination Connection ID (0/32..144)         ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+

const SHORT_HEADER_MASK: u8 = 0b1_1111;

//= https://tools.ietf.org/id/draft-ietf-quic-tls-22.txt#5.4.1
//#    Common Fields:
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |E E E E E E E E E  Packet Number (8/16/24/32) E E E E E E E E...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |   [Protected Payload (8/16/24)]             ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |             Sampled part of Protected Payload (128)         ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                 Protected Payload Remainder (*)             ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#              Figure 5: Header Protection and Ciphertext Sample
//#
//#    Before a TLS ciphersuite can be used with QUIC, a header protection
//#    algorithm MUST be specified for the AEAD used with that ciphersuite.
//#    This document defines algorithms for AEAD_AES_128_GCM,
//#    AEAD_AES_128_CCM, AEAD_AES_256_GCM (all AES AEADs are defined in
//#    [AEAD]), and AEAD_CHACHA20_POLY1305 [CHACHA].  Prior to TLS selecting
//#    a ciphersuite, AES header protection is used (Section 5.4.3),
//#    matching the AEAD_AES_128_GCM packet protection.
