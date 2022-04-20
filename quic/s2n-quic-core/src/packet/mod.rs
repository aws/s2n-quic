// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::connection;
use s2n_codec::{decoder_invariant, DecoderBufferMut, DecoderBufferMutResult, DecoderError};

pub(crate) type Tag = u8;

#[macro_use]
pub mod short;
#[macro_use]
pub mod version_negotiation;
#[macro_use]
pub mod initial;
#[macro_use]
pub mod zero_rtt;
#[macro_use]
pub mod handshake;
#[macro_use]
pub mod retry;

pub mod decoding;
pub mod encoding;
pub mod interceptor;
pub mod key_phase;
pub mod long;

pub mod number;
pub mod stateless_reset;

pub use key_phase::{KeyPhase, ProtectedKeyPhase};

use connection::id::ConnectionInfo;
use handshake::ProtectedHandshake;
use initial::ProtectedInitial;
use retry::ProtectedRetry;
use short::ProtectedShort;
use version_negotiation::ProtectedVersionNegotiation;
use zero_rtt::ProtectedZeroRtt;

// === API ===

pub type RemainingBuffer<'a> = Option<DecoderBufferMut<'a>>;

#[derive(Debug)]
pub enum ProtectedPacket<'a> {
    Short(ProtectedShort<'a>),
    VersionNegotiation(ProtectedVersionNegotiation<'a>),
    Initial(ProtectedInitial<'a>),
    ZeroRtt(ProtectedZeroRtt<'a>),
    Handshake(ProtectedHandshake<'a>),
    Retry(ProtectedRetry<'a>),
}

impl<'a> ProtectedPacket<'a> {
    pub fn decode<Validator: connection::id::Validator>(
        buffer: DecoderBufferMut<'a>,
        connection_info: &ConnectionInfo,
        connection_id_validator: &Validator,
    ) -> DecoderBufferMutResult<'a, Self> {
        BasicPacketDecoder.decode_packet(buffer, connection_info, connection_id_validator)
    }

    /// Returns the packet's destination connection ID
    pub fn destination_connection_id(&self) -> &[u8] {
        match self {
            ProtectedPacket::Short(packet) => packet.destination_connection_id(),
            ProtectedPacket::VersionNegotiation(packet) => packet.destination_connection_id(),
            ProtectedPacket::Initial(packet) => packet.destination_connection_id(),
            ProtectedPacket::ZeroRtt(packet) => packet.destination_connection_id(),
            ProtectedPacket::Handshake(packet) => packet.destination_connection_id(),
            ProtectedPacket::Retry(packet) => packet.destination_connection_id(),
        }
    }

    /// Returns the packet's source connection ID
    pub fn source_connection_id(&self) -> Option<&[u8]> {
        match self {
            ProtectedPacket::Short(_packet) => None,
            ProtectedPacket::VersionNegotiation(packet) => Some(packet.source_connection_id()),
            ProtectedPacket::Initial(packet) => Some(packet.source_connection_id()),
            ProtectedPacket::ZeroRtt(packet) => Some(packet.source_connection_id()),
            ProtectedPacket::Handshake(packet) => Some(packet.source_connection_id()),
            ProtectedPacket::Retry(packet) => Some(packet.source_connection_id()),
        }
    }

    pub fn version(&self) -> Option<u32> {
        match self {
            ProtectedPacket::Short(_) => None,
            ProtectedPacket::VersionNegotiation(_) => None,
            ProtectedPacket::Initial(packet) => Some(packet.version),
            ProtectedPacket::ZeroRtt(packet) => Some(packet.version),
            ProtectedPacket::Handshake(packet) => Some(packet.version),
            ProtectedPacket::Retry(packet) => Some(packet.version),
        }
    }
}

#[derive(Debug)]
pub enum CleartextPacket<'a> {
    Short(short::CleartextShort<'a>),
    VersionNegotiation(version_negotiation::CleartextVersionNegotiation<'a>),
    Initial(initial::CleartextInitial<'a>),
    ZeroRtt(zero_rtt::CleartextZeroRtt<'a>),
    Handshake(handshake::CleartextHandshake<'a>),
    Retry(retry::CleartextRetry<'a>),
}

struct BasicPacketDecoder;

impl<'a> PacketDecoder<'a> for BasicPacketDecoder {
    type Error = DecoderError;
    type Output = ProtectedPacket<'a>;

    fn handle_short_packet(
        &mut self,
        packet: ProtectedShort<'a>,
    ) -> Result<Self::Output, DecoderError> {
        Ok(ProtectedPacket::Short(packet))
    }

    fn handle_version_negotiation_packet(
        &mut self,
        packet: ProtectedVersionNegotiation<'a>,
    ) -> Result<Self::Output, DecoderError> {
        Ok(ProtectedPacket::VersionNegotiation(packet))
    }

    fn handle_initial_packet(
        &mut self,
        packet: ProtectedInitial<'a>,
    ) -> Result<Self::Output, DecoderError> {
        Ok(ProtectedPacket::Initial(packet))
    }

    fn handle_zero_rtt_packet(
        &mut self,
        packet: ProtectedZeroRtt<'a>,
    ) -> Result<Self::Output, DecoderError> {
        Ok(ProtectedPacket::ZeroRtt(packet))
    }

    fn handle_handshake_packet(
        &mut self,
        packet: ProtectedHandshake<'a>,
    ) -> Result<Self::Output, DecoderError> {
        Ok(ProtectedPacket::Handshake(packet))
    }

    fn handle_retry_packet(
        &mut self,
        packet: ProtectedRetry<'a>,
    ) -> Result<Self::Output, DecoderError> {
        Ok(ProtectedPacket::Retry(packet))
    }
}

pub trait PacketDecoder<'a> {
    type Output;
    type Error: From<DecoderError>;

    fn handle_short_packet(
        &mut self,
        packet: ProtectedShort<'a>,
    ) -> Result<Self::Output, Self::Error>;

    fn handle_version_negotiation_packet(
        &mut self,
        packet: ProtectedVersionNegotiation<'a>,
    ) -> Result<Self::Output, Self::Error>;

    fn handle_initial_packet(
        &mut self,
        packet: ProtectedInitial<'a>,
    ) -> Result<Self::Output, Self::Error>;

    fn handle_zero_rtt_packet(
        &mut self,
        packet: ProtectedZeroRtt<'a>,
    ) -> Result<Self::Output, Self::Error>;

    fn handle_handshake_packet(
        &mut self,
        packet: ProtectedHandshake<'a>,
    ) -> Result<Self::Output, Self::Error>;

    fn handle_retry_packet(
        &mut self,
        packet: ProtectedRetry<'a>,
    ) -> Result<Self::Output, Self::Error>;

    fn decode_packet<Validator: connection::id::Validator>(
        &mut self,
        buffer: DecoderBufferMut<'a>,
        connection_info: &ConnectionInfo,
        connection_id_validator: &Validator,
    ) -> Result<(Self::Output, DecoderBufferMut<'a>), Self::Error> {
        let peek = buffer.peek();

        let (tag, peek) = peek.decode()?;

        macro_rules! version_negotiation {
            ($version:ident) => {{
                let (packet, buffer) = ProtectedVersionNegotiation::decode(tag, $version, buffer)?;
                let output = self.handle_version_negotiation_packet(packet)?;
                Ok((output, buffer))
            }};
        }

        macro_rules! long_packet {
            ($struct:ident, $handler:ident) => {{
                let (version, _peek) = peek.decode()?;
                if version == version_negotiation::VERSION {
                    version_negotiation!(version)
                } else {
                    let (packet, buffer) = $struct::decode(tag, version, buffer)?;
                    let output = self.$handler(packet)?;
                    Ok((output, buffer))
                }
            }};
        }

        match tag >> 4 {
            short_tag!() => {
                let (packet, buffer) = short::ProtectedShort::decode(
                    tag,
                    buffer,
                    connection_info,
                    connection_id_validator,
                )?;
                let output = self.handle_short_packet(packet)?;
                Ok((output, buffer))
            }
            version_negotiation_no_fixed_bit_tag!() => {
                let (version, _peek) = peek.decode()?;
                decoder_invariant!(
                    version_negotiation::VERSION == version,
                    "invalid version negotiation packet"
                );
                version_negotiation!(version)
            }
            initial_tag!() => long_packet!(ProtectedInitial, handle_initial_packet),
            zero_rtt_tag!() => long_packet!(ProtectedZeroRtt, handle_zero_rtt_packet),
            handshake_tag!() => long_packet!(ProtectedHandshake, handle_handshake_packet),
            retry_tag!() => long_packet!(ProtectedRetry, handle_retry_packet),
            _ => Err(DecoderError::InvariantViolation("invalid packet").into()),
        }
    }
}

#[cfg(test)]
mod snapshots {
    use super::*;

    macro_rules! snapshot {
        ($name:ident) => {
            #[test]
            fn $name() {
                s2n_codec::assert_codec_round_trip_sample_file!(
                    crate::packet::ProtectedPacket,
                    concat!("src/packet/test_samples/", stringify!($name), ".bin"),
                    |buffer| {
                        let remote_address = crate::inet::ip::SocketAddress::default();
                        let connection_info =
                            crate::connection::id::ConnectionInfo::new(&remote_address);
                        crate::packet::ProtectedPacket::decode(
                            buffer,
                            &connection_info,
                            &long::DESTINATION_CONNECTION_ID_MAX_LEN,
                        )
                        .unwrap()
                    }
                );
            }
        };
    }

    snapshot!(short);
    snapshot!(initial);
    snapshot!(zero_rtt);
    snapshot!(handshake);
    snapshot!(retry);
    snapshot!(version_negotiation);
}
