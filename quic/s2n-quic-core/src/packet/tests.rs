// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{id::ConnectionInfo, ProcessingError},
    crypto::key::testing,
    inet::SocketAddress,
    packet::{
        encoding::PacketEncoder, number::PacketNumberSpace, CleartextPacket, ProtectedPacket,
    },
};
use bolero::check;
use s2n_codec::{DecoderBufferMut, Encoder, EncoderBuffer};

#[test]
fn round_trip() {
    let mut encoder_data = vec![];
    check!().for_each(move |data| {
        let mut data = data.to_vec();
        // add a few bytes to the end for padding
        encoder_data.resize(data.len() * 2, 0);

        let mut decoder_buffer = DecoderBufferMut::new(&mut data);
        let mut encoder_buffer = EncoderBuffer::new(&mut encoder_data);
        let remote_address = SocketAddress::default();
        let connection_info = ConnectionInfo::new(&remote_address);

        while let Ok((packet, remaining)) =
            ProtectedPacket::decode(decoder_buffer, &connection_info, &20)
        {
            if let Ok(cleartext_packet) = decrypt_packet(packet) {
                encoder_buffer = encode_packet(cleartext_packet, encoder_buffer);
            }
            decoder_buffer = remaining;
        }
    });
}

fn decrypt_packet(packet: ProtectedPacket) -> Result<CleartextPacket, ProcessingError> {
    use ProtectedPacket::*;
    match packet {
        Handshake(packet) => {
            let dcid = packet.destination_connection_id().to_vec();
            let scid = packet.source_connection_id().to_vec();

            let packet = packet.unprotect(
                &testing::HeaderKey::new(),
                PacketNumberSpace::Handshake.new_packet_number(Default::default()),
            )?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());

            let packet = packet.decrypt(&testing::Key::new())?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());

            Ok(CleartextPacket::Handshake(packet))
        }
        Initial(packet) => {
            let dcid = packet.destination_connection_id().to_vec();
            let scid = packet.source_connection_id().to_vec();
            let token = packet.token().to_vec();

            let packet = packet.unprotect(
                &testing::HeaderKey::new(),
                PacketNumberSpace::Initial.new_packet_number(Default::default()),
            )?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());
            assert_eq!(token, packet.token());

            let packet = packet.decrypt(&testing::Key::new())?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());
            assert_eq!(token, packet.token());

            Ok(CleartextPacket::Initial(packet))
        }
        Retry(packet) => {
            let _ = packet.destination_connection_id();
            let _ = packet.source_connection_id();

            Ok(CleartextPacket::Retry(packet))
        }
        Short(packet) => {
            let dcid = packet.destination_connection_id().to_vec();

            let packet = packet.unprotect(
                &testing::HeaderKey::new(),
                PacketNumberSpace::ApplicationData.new_packet_number(Default::default()),
            )?;
            assert_eq!(dcid, packet.destination_connection_id());

            let packet = packet.decrypt(&testing::Key::new())?;
            assert_eq!(dcid, packet.destination_connection_id());

            Ok(CleartextPacket::Short(packet))
        }
        ZeroRtt(packet) => {
            let dcid = packet.destination_connection_id().to_vec();
            let scid = packet.source_connection_id().to_vec();

            let packet = packet.unprotect(
                &testing::HeaderKey::new(),
                PacketNumberSpace::ApplicationData.new_packet_number(Default::default()),
            )?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());

            let packet = packet.decrypt(&testing::Key::new())?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());

            Ok(CleartextPacket::ZeroRtt(packet))
        }
        VersionNegotiation(packet) => {
            let _: Vec<_> = packet.iter().collect();

            Ok(CleartextPacket::VersionNegotiation(packet))
        }
    }
}

fn encode_packet<'a>(packet: CleartextPacket, mut encoder: EncoderBuffer<'a>) -> EncoderBuffer<'a> {
    use CleartextPacket::*;
    let result = match packet {
        Handshake(packet) => packet.encode_packet(
            &mut testing::Key::new(),
            &testing::HeaderKey::new(),
            PacketNumberSpace::Handshake.new_packet_number(Default::default()),
            None,
            encoder,
        ),
        Initial(packet) => packet.encode_packet(
            &mut testing::Key::new(),
            &testing::HeaderKey::new(),
            PacketNumberSpace::Initial.new_packet_number(Default::default()),
            None,
            encoder,
        ),
        Retry(packet) => {
            encoder.encode(&packet);
            return encoder;
        }
        Short(packet) => packet.encode_packet(
            &mut testing::Key::new(),
            &testing::HeaderKey::new(),
            PacketNumberSpace::ApplicationData.new_packet_number(Default::default()),
            None,
            encoder,
        ),
        ZeroRtt(packet) => packet.encode_packet(
            &mut testing::Key::new(),
            &testing::HeaderKey::new(),
            PacketNumberSpace::ApplicationData.new_packet_number(Default::default()),
            None,
            encoder,
        ),
        VersionNegotiation(packet) => {
            encoder.encode(&packet);
            return encoder;
        }
    };

    match result {
        Ok((_, encoder)) => encoder,
        Err(err) => err.take_buffer(),
    }
}
