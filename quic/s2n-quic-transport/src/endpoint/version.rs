use crate::endpoint;
use alloc::collections::VecDeque;
use core::{convert::TryInto, marker::PhantomData, time::Duration};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    connection,
    inet::{DatagramInfo, ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet::{version_negotiation::VersionNegotiation, ProtectedPacket},
    path::MINIMUM_MTU,
    transport::error::TransportError,
};

#[derive(Debug)]
pub struct Negotiator<C> {
    transmissions: VecDeque<Transmission>,
    max_peers: usize,
    config: PhantomData<C>,
}

const DEFAULT_MAX_PEERS: usize = 1024;

const SUPPORTED_VERSIONS: &[u32] = &[
    0xff00_0020, // draft-32 (https://github.com/quicwg/base-drafts/wiki/20th-Implementation-Draft)
    0xff00_001f, // draft-31
    0xff00_001e, // draft-30
    0xff00_001d, // draft-29 (https://github.com/quicwg/base-drafts/wiki/19th-Implementation-Draft)
];

macro_rules! is_supported {
    ($packet:ident) => {
        SUPPORTED_VERSIONS
            .iter()
            .cloned()
            .any(|v| v == $packet.version)
    };
}

impl<C: endpoint::Config> Default for Negotiator<C> {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_PEERS)
    }
}

impl<C: endpoint::Config> Negotiator<C> {
    pub fn new(max_peers: usize) -> Self {
        Self {
            transmissions: if C::ENDPOINT_TYPE.is_server() {
                VecDeque::with_capacity(max_peers)
            } else {
                VecDeque::new()
            },
            max_peers,
            config: PhantomData,
        }
    }

    pub fn on_packet(
        &mut self,
        datagram_info: &DatagramInfo,
        packet: &ProtectedPacket,
    ) -> Result<(), TransportError> {
        // always forward packets for clients on to connections
        if C::ENDPOINT_TYPE.is_client() {
            return Ok(());
        }

        let packet = match packet {
            ProtectedPacket::Initial(packet) => {
                if is_supported!(packet) {
                    return Ok(());
                }
                packet
            }
            ProtectedPacket::ZeroRTT(packet) => {
                if is_supported!(packet) {
                    return Ok(());
                }

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.1
                //# a server that is able to recognize packets as
                //# 0-RTT might choose not to send Version Negotiation packets in
                //# response to 0-RTT packets with the expectation that it will
                //# eventually receive an Initial packet.
                return Err(TransportError::NO_ERROR);
            }
            _ => return Ok(()),
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6
        //# A server might not send a Version
        //# Negotiation packet if the datagram it receives is smaller than the
        //# minimum size specified in a different version;
        if datagram_info.payload_len < (MINIMUM_MTU as usize) {
            return Err(TransportError::NO_ERROR);
        }

        // store the peer's address if we're not at capacity
        if self.transmissions.len() != self.max_peers {
            self.transmissions.push_back(Transmission {
                remote_address: datagram_info.remote_address,
                destination_connection_id: packet.destination_connection_id().try_into()?,
                source_connection_id: packet.source_connection_id().try_into()?,
            });
        }

        Err(TransportError::NO_ERROR)
    }

    pub fn on_transmit<Tx: tx::Queue>(&mut self, queue: &mut Tx) {
        // clients don't transmit version negotiation packets
        if C::ENDPOINT_TYPE.is_client() {
            return;
        }

        while let Some(transmission) = self.transmissions.pop_front() {
            if queue.push(&transmission).is_err() {
                self.transmissions.push_front(transmission);
                return;
            }
        }
    }
}

#[derive(Debug)]
struct Transmission {
    remote_address: SocketAddress,
    destination_connection_id: connection::Id,
    source_connection_id: connection::Id,
}

impl tx::Message for &Transmission {
    fn remote_address(&mut self) -> SocketAddress {
        self.remote_address
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        Default::default()
    }

    fn delay(&mut self) -> Duration {
        Default::default()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        0
    }

    fn write_payload(&mut self, buffer: &mut [u8]) -> usize {
        let mut buffer = EncoderBuffer::new(buffer);
        VersionNegotiation {
            tag: 0,
            destination_connection_id: self.destination_connection_id.as_ref(),
            source_connection_id: self.source_connection_id.as_ref(),
            supported_versions: SupportedVersions,
        }
        .encode(&mut buffer);
        buffer.len()
    }
}

#[derive(Clone, Copy, Debug)]
struct SupportedVersions;

impl EncoderValue for SupportedVersions {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        for version in SUPPORTED_VERSIONS {
            encoder.encode(version);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_codec::{DecoderBufferMut, Encoder, EncoderBuffer};
    use s2n_quic_core::{
        packet::{
            handshake::Handshake,
            initial::Initial,
            number::{PacketNumberSpace, TruncatedPacketNumber},
            short::Short,
            zero_rtt::ZeroRTT,
        },
        varint::VarInt,
    };

    type Server = Negotiator<crate::endpoint::testing::Server>;
    type Client = Negotiator<crate::endpoint::testing::Client>;

    fn datagram_info(payload_len: usize) -> DatagramInfo {
        DatagramInfo {
            timestamp: s2n_quic_platform::time::now(),
            remote_address: SocketAddress::default(),
            payload_len,
            ecn: Default::default(),
        }
    }

    macro_rules! on_packet {
        ($negotiator:ident, $datagram_info:expr, $packet:expr) => {{
            let mut buffer = vec![0u8; 1200];
            let mut encoder = EncoderBuffer::new(&mut buffer);

            encoder.encode(&$packet);

            let len = encoder.len();
            let decoder = DecoderBufferMut::new(&mut buffer[..len]);
            let (packet, _) = ProtectedPacket::decode(decoder, &3).unwrap();
            $negotiator.on_packet(&$datagram_info, &packet)
        }};
    }

    fn on_initial_packet<C: endpoint::Config>(
        datagram_info: DatagramInfo,
        version: u32,
        negotiator: &mut Negotiator<C>,
    ) -> Result<(), TransportError> {
        on_packet!(
            negotiator,
            datagram_info,
            Initial {
                version,
                destination_connection_id: &[1u8, 2, 3][..],
                source_connection_id: &[4u8, 5, 6][..],
                token: &[][..],
                packet_number: pn(PacketNumberSpace::Initial),
                payload: &[1u8, 2, 3, 4, 5][..],
            }
        )
    }

    fn on_handshake_packet<C: endpoint::Config>(
        datagram_info: DatagramInfo,
        version: u32,
        negotiator: &mut Negotiator<C>,
    ) -> Result<(), TransportError> {
        on_packet!(
            negotiator,
            datagram_info,
            Handshake {
                version,
                destination_connection_id: &[1u8, 2, 3][..],
                source_connection_id: &[4u8, 5, 6][..],
                packet_number: pn(PacketNumberSpace::ApplicationData),
                payload: &[1u8, 2, 3, 4, 5][..],
            }
        )
    }

    fn on_zerortt_packet<C: endpoint::Config>(
        datagram_info: DatagramInfo,
        version: u32,
        negotiator: &mut Negotiator<C>,
    ) -> Result<(), TransportError> {
        on_packet!(
            negotiator,
            datagram_info,
            ZeroRTT {
                version,
                destination_connection_id: &[1u8, 2, 3][..],
                source_connection_id: &[4u8, 5, 6][..],
                packet_number: pn(PacketNumberSpace::ApplicationData),
                payload: &[1u8, 2, 3, 4, 5][..],
            }
        )
    }

    fn on_version_negotiation_packet<C: endpoint::Config>(
        datagram_info: DatagramInfo,
        negotiator: &mut Negotiator<C>,
    ) -> Result<(), TransportError> {
        on_packet!(
            negotiator,
            datagram_info,
            VersionNegotiation {
                tag: 0,
                destination_connection_id: &[1u8, 2, 3][..],
                source_connection_id: &[4u8, 5, 6][..],
                supported_versions: SupportedVersions,
            }
        )
    }

    fn on_short_packet<C: endpoint::Config>(
        datagram_info: DatagramInfo,
        negotiator: &mut Negotiator<C>,
    ) -> Result<(), TransportError> {
        on_packet!(
            negotiator,
            datagram_info,
            Short {
                destination_connection_id: &[1u8, 2, 3][..],
                key_phase: Default::default(),
                spin_bit: Default::default(),
                packet_number: pn(PacketNumberSpace::ApplicationData),
                payload: &[1u8, 2, 3, 4, 5][..],
            }
        )
    }

    fn pn(space: PacketNumberSpace) -> TruncatedPacketNumber {
        let pn = space.new_packet_number(VarInt::default());
        pn.truncate(pn).unwrap()
    }

    const INVALID_VERSION: u32 = 123;

    #[test]
    fn client_test() {
        let mut client = Client::default();

        assert_eq!(
            on_initial_packet(datagram_info(1200), INVALID_VERSION, &mut client),
            Ok(()),
            "client implementation should always allow version negotiator packets through",
        );

        assert_eq!(
            on_zerortt_packet(datagram_info(1200), INVALID_VERSION, &mut client),
            Ok(()),
            "client implementation should always allow version negotiator packets through",
        );

        assert!(
            client.transmissions.is_empty(),
            "clients should never negotiate at the endpoint level"
        );
    }

    #[test]
    fn server_initial_test() {
        let mut server = Server::default();

        assert_eq!(
            on_initial_packet(datagram_info(1200), INVALID_VERSION, &mut server),
            Err(TransportError::NO_ERROR),
            "server implementations should error on invalid versions"
        );

        assert!(
            !server.transmissions.is_empty(),
            "servers should negotiate with initial packets"
        );
    }

    #[test]
    fn server_zerortt_test() {
        let mut server = Server::default();

        assert_eq!(
            on_zerortt_packet(datagram_info(1200), INVALID_VERSION, &mut server),
            Err(TransportError::NO_ERROR),
            "server implementations should error on invalid versions"
        );

        assert!(
            server.transmissions.is_empty(),
            "servers should not negotiate with zero_rtt packets"
        );
    }

    #[test]
    fn server_undersized_test() {
        let mut server = Server::default();

        assert_eq!(
            on_initial_packet(datagram_info(32), INVALID_VERSION, &mut server),
            Err(TransportError::NO_ERROR),
            "server implementations should error on invalid versions"
        );

        assert!(
            server.transmissions.is_empty(),
            "servers should not negotiate with undersized packets"
        );
    }

    #[test]
    fn server_max_peers_test() {
        let mut server = Server::new(2);

        for _ in 0..5 {
            assert_eq!(
                on_initial_packet(datagram_info(1200), INVALID_VERSION, &mut server),
                Err(TransportError::NO_ERROR),
                "server implementations should error on invalid versions"
            );
        }

        assert_eq!(
            server.transmissions.len(),
            2,
            "servers should not negotiate with more than the allowed max_peers"
        );
    }

    #[test]
    fn server_other_packets_test() {
        let mut server = Server::default();

        assert_eq!(
            on_version_negotiation_packet(datagram_info(1200), &mut server),
            Ok(()),
            "server implementations should allow other packets through to connections"
        );

        assert_eq!(
            on_handshake_packet(datagram_info(1200), INVALID_VERSION, &mut server),
            Ok(()),
            "server implementations should allow other packets through to connections"
        );

        assert_eq!(
            on_short_packet(datagram_info(1200), &mut server),
            Ok(()),
            "server implementations should allow other packets through to connections"
        );

        assert!(
            server.transmissions.is_empty(),
            "servers should not negotiate with version negotiation packets"
        );
    }
}
