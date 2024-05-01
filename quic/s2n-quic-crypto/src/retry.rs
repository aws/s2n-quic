// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{constant_time, ring_aead as aead};
use s2n_quic_core::crypto::{
    self, packet_protection,
    retry::{IntegrityTag, NONCE_BYTES, SECRET_KEY_BYTES},
};

lazy_static::lazy_static! {
    /// Compute the Initial salt once, as the seed is constant
    static ref SECRET_KEY: aead::LessSafeKey = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_128_GCM, &SECRET_KEY_BYTES).unwrap(),
    );
}

#[derive(Debug)]
pub struct RetryKey;

impl crypto::RetryKey for RetryKey {
    fn generate_tag(pseudo_packet: &[u8]) -> IntegrityTag {
        let nonce = aead::Nonce::assume_unique_for_key(NONCE_BYTES);
        let tag = SECRET_KEY
            .seal_in_place_separate_tag(nonce, aead::Aad::from(pseudo_packet), &mut [])
            .expect("in_out len is 0 and should always be less than the nonce max bytes");

        tag.as_ref()
            .try_into()
            .expect("AES_128_GCM tag len should always be 128 bits")
    }

    fn validate(pseudo_packet: &[u8], tag: IntegrityTag) -> Result<(), packet_protection::Error> {
        let expected = Self::generate_tag(pseudo_packet);

        constant_time::verify_slices_are_equal(&expected, &tag)
            .map_err(|_| packet_protection::Error::DECRYPT_ERROR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;
    use s2n_codec::{DecoderBufferMut, Encoder, EncoderBuffer};
    use s2n_quic_core::{
        connection,
        connection::id::ConnectionInfo,
        crypto::{retry, RetryKey as _},
        inet, packet,
        packet::number::{PacketNumberSpace, TruncatedPacketNumber},
        random, token,
        varint::VarInt,
    };

    #[test]
    fn test_tag_validation() {
        let invalid_tag: [u8; 16] = hex!("00112233445566778899aabbccddeeff");

        assert!(
            RetryKey::validate(&retry::example::PSEUDO_PACKET, retry::example::EXPECTED_TAG)
                .is_ok()
        );
        assert!(RetryKey::validate(&retry::example::PSEUDO_PACKET, invalid_tag).is_err());
    }

    fn pn(space: PacketNumberSpace) -> TruncatedPacketNumber {
        let pn = space.new_packet_number(VarInt::new(0x1).unwrap());
        pn.truncate(pn).unwrap()
    }

    #[test]
    fn test_packet_encode() {
        let remote_address = inet::ip::SocketAddress::default();
        let mut token_format = token::testing::Format::new();
        // Values are taken from the retry packet example. Since this is the Initial packet that
        // creates the retry, source_connection_id of the Initial is set to the destination
        // connection id of the retry.
        let packet = packet::initial::Initial {
            version: 0x01,
            destination_connection_id: &retry::example::ODCID[..],
            source_connection_id: &retry::example::DCID[..],
            token: &retry::example::TOKEN[..],
            packet_number: pn(PacketNumberSpace::Initial),
            payload: &[1u8, 2, 3, 4, 5][..],
        };

        let mut buf = vec![0u8; 1200];
        let mut encoder = EncoderBuffer::new(&mut buf);
        encoder.encode(&packet);
        let len = encoder.len();
        let decoder = DecoderBufferMut::new(&mut buf[..len]);
        let connection_info = ConnectionInfo::new(&remote_address);
        let mut output_buf = vec![0u8; 1200];

        if let Some(packet) =
            match packet::ProtectedPacket::decode(decoder, &connection_info, &3).unwrap() {
                (packet::ProtectedPacket::Initial(packet), _) => Some(packet),
                _ => None,
            }
        {
            let local_conn_id = connection::LocalId::try_from_bytes(&retry::example::SCID).unwrap();
            if let Some(range) = packet::retry::Retry::encode_packet::<_, RetryKey>(
                &remote_address,
                &packet,
                &local_conn_id,
                &mut random::testing::Generator(5),
                &mut token_format,
                &mut output_buf,
            ) {
                assert_eq!(&output_buf[range], &retry::example::PACKET[..]);
            }
        }
    }

    #[test]
    #[should_panic]
    fn test_odcid_different_from_local_cid() {
        let remote_address = inet::ip::SocketAddress::default();
        let mut token_format = token::testing::Format::new();
        // Values are taken from the retry packet example. Since this is the Initial packet that
        // creates the retry, source_connection_id of the Initial is set to the destination
        // connection id of the retry.
        let packet = packet::initial::Initial {
            version: 0xff00_0020,
            destination_connection_id: &retry::example::ODCID[..],
            source_connection_id: &retry::example::DCID[..],
            token: &retry::example::TOKEN[..],
            packet_number: pn(PacketNumberSpace::Initial),
            payload: &[1u8, 2, 3, 4, 5][..],
        };

        let mut buf = vec![0u8; 1200];
        let mut encoder = EncoderBuffer::new(&mut buf);
        encoder.encode(&packet);
        let len = encoder.len();
        // Test the packet encoding when an invalid local_conn_id is used
        let decoder = DecoderBufferMut::new(&mut buf[..len]);
        let connection_info = ConnectionInfo::new(&remote_address);
        let mut output_buf = vec![0u8; 1200];
        if let Some(packet) =
            match packet::ProtectedPacket::decode(decoder, &connection_info, &3).unwrap() {
                (packet::ProtectedPacket::Initial(packet), _) => Some(packet),
                _ => None,
            }
        {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.5.1
            //= type=test
            //# This value MUST NOT be equal to the Destination
            //# Connection ID field of the packet sent by the client.
            let local_conn_id =
                connection::LocalId::try_from_bytes(&retry::example::ODCID).unwrap();
            assert!(packet::retry::Retry::encode_packet::<_, RetryKey>(
                &remote_address,
                &packet,
                &local_conn_id,
                &mut random::testing::Generator(5),
                &mut token_format,
                &mut output_buf,
            )
            .is_none());
        }
    }
}
