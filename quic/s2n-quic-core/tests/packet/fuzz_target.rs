use bolero::check;
use s2n_codec::{DecoderBufferMut, Encoder, EncoderBuffer};
use s2n_quic_core::{
    crypto::CryptoError,
    packet::{
        encoding::PacketEncoder, number::PacketNumberSpace, CleartextPacket, ProtectedPacket,
    },
};

fn main() {
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

fn decrypt_packet(packet: ProtectedPacket) -> Result<CleartextPacket, CryptoError> {
    use ProtectedPacket::*;
    match packet {
        Handshake(packet) => {
            let dcid = packet.destination_connection_id().to_vec();
            let scid = packet.source_connection_id().to_vec();

            let packet = packet.unprotect(
                &NullHandshakeCrypto::new(),
                PacketNumberSpace::Handshake.new_packet_number(Default::default()),
            )?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());

            let packet = packet.decrypt(&NullHandshakeCrypto::new())?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());

            Ok(CleartextPacket::Handshake(packet))
        }
        Initial(packet) => {
            let dcid = packet.destination_connection_id().to_vec();
            let scid = packet.source_connection_id().to_vec();
            let token = packet.token().to_vec();

            let packet = packet.unprotect(
                &NullInitialCrypto::new(),
                PacketNumberSpace::Initial.new_packet_number(Default::default()),
            )?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());
            assert_eq!(token, packet.token());

            let packet = packet.decrypt(&NullInitialCrypto::new())?;
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
                &NullOneRTTCrypto::new(),
                PacketNumberSpace::ApplicationData.new_packet_number(Default::default()),
            )?;
            assert_eq!(dcid, packet.destination_connection_id());

            let packet = packet.decrypt(&NullOneRTTCrypto::new())?;
            assert_eq!(dcid, packet.destination_connection_id());

            Ok(CleartextPacket::Short(packet))
        }
        ZeroRTT(packet) => {
            let dcid = packet.destination_connection_id().to_vec();
            let scid = packet.source_connection_id().to_vec();

            let packet = packet.unprotect(
                &NullZeroRTTCrypto::new(),
                PacketNumberSpace::ApplicationData.new_packet_number(Default::default()),
            )?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());

            let packet = packet.decrypt(&NullZeroRTTCrypto::new())?;
            assert_eq!(dcid, packet.destination_connection_id());
            assert_eq!(scid, packet.source_connection_id());

            Ok(CleartextPacket::ZeroRTT(packet))
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
            &NullHandshakeCrypto::new(),
            PacketNumberSpace::Handshake.new_packet_number(Default::default()),
            encoder,
        ),
        Initial(packet) => packet.encode_packet(
            &NullInitialCrypto::new(),
            PacketNumberSpace::Initial.new_packet_number(Default::default()),
            encoder,
        ),
        Retry(packet) => {
            encoder.encode(&packet);
            return encoder;
        }
        Short(packet) => packet.encode_packet(
            &NullOneRTTCrypto::new(),
            PacketNumberSpace::ApplicationData.new_packet_number(Default::default()),
            encoder,
        ),
        ZeroRTT(packet) => packet.encode_packet(
            &NullZeroRTTCrypto::new(),
            PacketNumberSpace::ApplicationData.new_packet_number(Default::default()),
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

// NULL Crypto implementation

use s2n_quic_core::{
    connection::id::ConnectionInfo,
    crypto::{
        handshake::HandshakeCrypto,
        header_crypto::{HeaderCrypto, HeaderProtectionMask},
        initial::InitialCrypto,
        key::Key,
        one_rtt::OneRTTCrypto,
        zero_rtt::ZeroRTTCrypto,
    },
    inet::SocketAddress,
};

#[derive(Copy, Clone, Debug, Default)]
pub struct NullCrypto;

impl NullCrypto {
    pub const fn new() -> Self {
        Self
    }
}

impl Key for NullCrypto {
    fn decrypt(
        &self,
        _packet_number: u64,
        _header: &[u8],
        _payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        Ok(())
    }

    fn encrypt(
        &self,
        _packet_number: u64,
        _header: &[u8],
        _payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        Ok(())
    }

    fn tag_len(&self) -> usize {
        0
    }

    fn aead_confidentiality_limit(&self) -> u64 {
        0
    }

    fn aead_integrity_limit(&self) -> u64 {
        0
    }
}

#[test]
fn key_test() {
    let crypto = NullCrypto;
    let header = vec![1, 2, 3];
    let mut payload = vec![4, 5, 6];

    crypto.decrypt(0, &header, &mut payload).unwrap();
    assert_eq!(payload, &[4, 5, 6]);

    crypto.encrypt(0, &header, &mut payload).unwrap();
    assert_eq!(payload, &[4, 5, 6]);
}

impl HeaderCrypto for NullCrypto {
    fn opening_header_protection_mask(&self, _sample: &[u8]) -> HeaderProtectionMask {
        [0; 5]
    }

    fn opening_sample_len(&self) -> usize {
        0
    }

    fn sealing_header_protection_mask(&self, _sample: &[u8]) -> HeaderProtectionMask {
        [0; 5]
    }

    fn sealing_sample_len(&self) -> usize {
        0
    }
}

#[test]
fn header_crypto_test() {
    let crypto = NullCrypto;
    crypto.opening_header_protection_mask(&[]);
    assert_eq!(crypto.opening_sample_len(), 0);
    crypto.sealing_header_protection_mask(&[]);
    assert_eq!(crypto.sealing_sample_len(), 0);
}

pub type NullInitialCrypto = NullCrypto;
impl InitialCrypto for NullCrypto {
    fn new_server(_connection_id: &[u8]) -> Self {
        NullCrypto
    }

    fn new_client(_connection_id: &[u8]) -> Self {
        NullCrypto
    }
}

pub type NullHandshakeCrypto = NullCrypto;
impl HandshakeCrypto for NullCrypto {}

pub type NullOneRTTCrypto = NullCrypto;
impl OneRTTCrypto for NullCrypto {}

pub type NullZeroRTTCrypto = NullCrypto;
impl ZeroRTTCrypto for NullCrypto {}
