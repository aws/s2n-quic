use crate::endpoint;
use alloc::collections::VecDeque;
use core::{marker::PhantomData, time::Duration};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet,
    packet::ProtectedPacket,
    path::MINIMUM_MTU,
    transport::error::TransportError,
};

#[derive(Debug)]
pub struct Negotiator<C> {
    transmissions: VecDeque<Transmission>,
    max_peers: usize,
    config: PhantomData<C>,
}

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

impl<Config: endpoint::Config> Default for Negotiator<Config> {
    fn default() -> Self {
        Self::new(endpoint::DEFAULT_MAX_PEERS)
    }
}

impl<Config: endpoint::Config> Negotiator<Config> {
    pub fn new(max_peers: usize) -> Self {
        Self {
            transmissions: if Config::ENDPOINT_TYPE.is_server() {
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
        remote_address: SocketAddress,
        payload_len: usize,
        packet: &ProtectedPacket,
    ) -> Result<(), TransportError> {
        // always forward packets for clients on to connections
        if Config::ENDPOINT_TYPE.is_client() {
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
            ProtectedPacket::VersionNegotiation(_) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.1
                //# An endpoint MUST NOT send a Version Negotiation packet
                //# in response to receiving a Version Negotiation packet.
                return Ok(());
            }
            _ => return Ok(()),
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.2
        //# If a server receives a packet that indicates an unsupported version
        //# but is large enough to initiate a new connection for any supported
        //# version, the server SHOULD send a Version Negotiation packet as
        //# described in Section 6.1.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6
        //# A server might not send a Version
        //# Negotiation packet if the datagram it receives is smaller than the
        //# minimum size specified in a different version;
        if payload_len < (MINIMUM_MTU as usize) {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.2
            //# Servers MUST
            //# drop smaller packets that specify unsupported versions.
            return Err(TransportError::NO_ERROR);
        }

        {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.2
            //# A server MAY limit the number of packets
            //# to which it responds with a Version Negotiation packet.

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.1
            //# A server MAY limit the number of Version Negotiation packets it
            //# sends.

            // store the peer's address if we're not at capacity
            if self.transmissions.len() != self.max_peers {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.2
                //# Servers SHOULD respond with a Version
                //# Negotiation packet, provided that the datagram is sufficiently long.
                self.transmissions
                    .push_back(Transmission::new(remote_address, packet));
            }
        }

        Err(TransportError::NO_ERROR)
    }

    pub fn on_transmit<Tx: tx::Queue>(&mut self, queue: &mut Tx) {
        // clients don't transmit version negotiation packets
        if Config::ENDPOINT_TYPE.is_client() {
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

struct Transmission {
    remote_address: SocketAddress,
    // The MINIMUM_MTU size allows for at least 170 supported versions
    packet: [u8; MINIMUM_MTU as usize],
    packet_len: usize,
}

impl core::fmt::Debug for Transmission {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Transmission")
            .field("remote_address", &self.remote_address)
            .field("packet_len", &self.packet_len)
            .field("packet", &&self.packet[0..self.packet_len])
            .finish()
    }
}

impl Transmission {
    pub fn new(
        remote_address: SocketAddress,
        initial_packet: &packet::initial::ProtectedInitial,
    ) -> Self {
        let mut packet_buf = [0u8; MINIMUM_MTU as usize];
        let version_packet = packet::version_negotiation::VersionNegotiation::from_initial(
            initial_packet,
            SupportedVersions,
        );

        let mut buffer = EncoderBuffer::new(&mut packet_buf);
        version_packet.encode(&mut buffer);
        let packet_len = buffer.len();

        Self {
            remote_address,
            packet: packet_buf,
            packet_len,
        }
    }
}

impl AsRef<[u8]> for Transmission {
    fn as_ref(&self) -> &[u8] {
        &self.packet[..self.packet_len]
    }
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
        let packet = self.as_ref();
        buffer[..packet.len()].copy_from_slice(packet);
        packet.len()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SupportedVersions;

impl EncoderValue for SupportedVersions {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        for version in SUPPORTED_VERSIONS {
            encoder.encode(version);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.3
        //# Endpoints MAY add reserved versions to any field where unknown or
        //# unsupported versions are ignored to test that a peer correctly
        //# ignores the value.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.3
        //# Endpoints MAY send packets with a reserved version to test that a
        //# peer correctly discards the packet.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.3
        //# For a server to use a new version in the future, clients need to
        //# correctly handle unsupported versions.  Some version numbers
        //# (0x?a?a?a?a as defined in Section 15) are reserved for inclusion in
        //# fields that contain version numbers.
        encoder.encode(&0xdadada);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;
    use s2n_codec::{DecoderBufferMut, Encoder, EncoderBuffer};
    use s2n_quic_core::{
        connection,
        connection::id::ConnectionInfo,
        inet::DatagramInfo,
        packet::{
            handshake::Handshake,
            initial::Initial,
            number::{PacketNumberSpace, TruncatedPacketNumber},
            short::Short,
            version_negotiation::VersionNegotiation,
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
            destination_connection_id: connection::LocalId::TEST_ID,
        }
    }

    macro_rules! on_packet {
        ($negotiator:ident, $remote_address:expr, $payload_len:expr, $packet:expr) => {{
            let mut buffer = vec![0u8; 1200];
            let mut encoder = EncoderBuffer::new(&mut buffer);

            encoder.encode(&$packet);

            let len = encoder.len();
            let decoder = DecoderBufferMut::new(&mut buffer[..len]);
            let remote_address = SocketAddress::default();
            let connection_info = ConnectionInfo::new(&remote_address);
            let (packet, _) = ProtectedPacket::decode(decoder, &connection_info, &3).unwrap();
            $negotiator.on_packet($remote_address, $payload_len, &packet)
        }};
    }

    fn on_initial_packet<C: endpoint::Config>(
        datagram_info: DatagramInfo,
        version: u32,
        negotiator: &mut Negotiator<C>,
    ) -> Result<(), TransportError> {
        on_packet!(
            negotiator,
            datagram_info.remote_address,
            datagram_info.payload_len,
            Initial {
                version,
                destination_connection_id: &[1u8, 2, 3][..],
                source_connection_id: &[4u8, 5, 6, 7][..],
                token: &[][..],
                packet_number: pn(PacketNumberSpace::Initial),
                payload: &[1u8, 2, 3, 4, 5][..],
            }
        )
    }

    fn on_future_version_initial_packet<C: endpoint::Config>(
        datagram_info: DatagramInfo,
        version: u32,
        negotiator: &mut Negotiator<C>,
    ) -> Result<(), TransportError> {
        on_packet!(
            negotiator,
            datagram_info.remote_address,
            datagram_info.payload_len,
            Initial {
                version,
                // Maximum length connection IDs that may be valid in future versions
                destination_connection_id: &[1u8; size_of::<u8>()][..],
                source_connection_id: &[2u8; size_of::<u8>()][..],
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
            datagram_info.remote_address,
            datagram_info.payload_len,
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
            datagram_info.remote_address,
            datagram_info.payload_len,
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
            datagram_info.remote_address,
            datagram_info.payload_len,
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
            datagram_info.remote_address,
            datagram_info.payload_len,
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
    fn server_future_version_initial_test() {
        let mut server = Server::default();

        assert_eq!(
            on_future_version_initial_packet(datagram_info(1200), INVALID_VERSION, &mut server),
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
