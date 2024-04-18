// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint;
use alloc::collections::VecDeque;
use core::time::Duration;
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    event,
    inet::ExplicitCongestionNotification,
    io::tx,
    packet,
    packet::ProtectedPacket,
    path::{self, MINIMUM_MAX_DATAGRAM_SIZE},
};

#[derive(Debug)]
pub struct Negotiator<Config: endpoint::Config> {
    transmissions: VecDeque<Transmission<Config::PathHandle>>,
    max_peers: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Error;

const SUPPORTED_VERSIONS: &[u32] = &[
    0x1, // Draft 34 / Version 1 (https://github.com/quicwg/base-drafts/wiki/21st-Implementation-Draft)
];

macro_rules! is_supported {
    ($packet:ident, $publisher:ident) => {{
        let supported = SUPPORTED_VERSIONS
            .iter()
            .cloned()
            .any(|v| v == $packet.version);

        if supported {
            //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.1
            //# Upon receiving a client initial with a supported version, the
            //# server logs this event with server_versions and chosen_version set
            $publisher.on_version_information(event::builder::VersionInformation {
                server_versions: &SUPPORTED_VERSIONS,
                client_versions: &[],
                chosen_version: Some($packet.version),
            });
        } else {
            //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.1
            //# Upon receiving a client initial with an unsupported version, the
            //# server logs this event with server_versions set and
            //# client_versions to the single-element array containing the
            //# client's attempted version.  The absence of chosen_version implies
            //# no overlap was found.
            $publisher.on_version_information(event::builder::VersionInformation {
                server_versions: &SUPPORTED_VERSIONS,
                client_versions: &[$packet.version],
                chosen_version: None,
            });
        }

        supported
    }};
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
        }
    }

    pub fn on_packet<Pub: event::EndpointPublisher>(
        &mut self,
        path: &Config::PathHandle,
        payload_len: usize,
        packet: &ProtectedPacket,
        publisher: &mut Pub,
    ) -> Result<(), Error> {
        // always forward packets for clients on to connections
        if Config::ENDPOINT_TYPE.is_client() {
            return Ok(());
        }

        let packet = match packet {
            ProtectedPacket::Initial(packet) => {
                if is_supported!(packet, publisher) {
                    return Ok(());
                }
                packet
            }
            ProtectedPacket::ZeroRtt(packet) => {
                if is_supported!(packet, publisher) {
                    return Ok(());
                }

                //= https://www.rfc-editor.org/rfc/rfc9000#section-6.1
                //# a server that is able to recognize packets as
                //# 0-RTT might choose not to send Version Negotiation packets in
                //# response to 0-RTT packets with the expectation that it will
                //# eventually receive an Initial packet.
                return Err(Error);
            }
            ProtectedPacket::VersionNegotiation(_packet) => {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-6.1
                //# An endpoint MUST NOT send a Version Negotiation packet
                //# in response to receiving a Version Negotiation packet.
                return Ok(());
            }
            _ => return Ok(()),
        };

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
        //# If a server receives a packet that indicates an unsupported version
        //# and if the packet is large enough to initiate a new connection for
        //# any supported version, the server SHOULD send a Version Negotiation
        //# packet as described in Section 6.1.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-6
        //# A server might not send a Version
        //# Negotiation packet if the datagram it receives is smaller than the
        //# minimum size specified in a different version;
        if payload_len < (MINIMUM_MAX_DATAGRAM_SIZE as usize) {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
            //# Servers MUST
            //# drop smaller packets that specify unsupported versions.
            return Err(Error);
        }

        {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
            //# A server MAY limit the number of packets
            //# to which it responds with a Version Negotiation packet.

            //= https://www.rfc-editor.org/rfc/rfc9000#section-6.1
            //# A server MAY limit the number of Version Negotiation packets it
            //# sends.

            // store the peer's address if we're not at capacity
            if self.transmissions.len() != self.max_peers {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
                //# Servers SHOULD respond with a Version
                //# Negotiation packet, provided that the datagram is sufficiently long.
                self.transmissions
                    .push_back(Transmission::new(*path, packet));
            }
        }

        Err(Error)
    }

    pub fn on_transmit<
        Tx: tx::Queue<Handle = Config::PathHandle>,
        Pub: event::EndpointPublisher,
    >(
        &mut self,
        queue: &mut Tx,
        publisher: &mut Pub,
    ) {
        // clients don't transmit version negotiation packets
        if Config::ENDPOINT_TYPE.is_client() {
            return;
        }

        while let Some(transmission) = self.transmissions.pop_front() {
            match queue.push(&transmission) {
                Ok(tx::Outcome { len, .. }) => {
                    publisher.on_endpoint_packet_sent(event::builder::EndpointPacketSent {
                        packet_header: event::builder::PacketHeader::VersionNegotiation {},
                    });

                    publisher.on_endpoint_datagram_sent(event::builder::EndpointDatagramSent {
                        len: len as u16,
                        gso_offset: 0,
                    });
                }
                Err(_) => {
                    self.transmissions.push_front(transmission);
                    return;
                }
            }
        }
    }
}

struct Transmission<Path: path::Handle> {
    path: Path,
    // The MINIMUM_MAX_DATAGRAM_SIZE size allows for at least 170 supported versions
    packet: [u8; MINIMUM_MAX_DATAGRAM_SIZE as usize],
    packet_len: usize,
}

impl<Path: path::Handle> core::fmt::Debug for Transmission<Path> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Transmission")
            .field("remote_address", &self.path.remote_address())
            .field("local_address", &self.path.local_address())
            .field("packet_len", &self.packet_len)
            .field("packet", &&self.packet[0..self.packet_len])
            .finish()
    }
}

impl<Path: path::Handle> Transmission<Path> {
    pub fn new(path: Path, initial_packet: &packet::initial::ProtectedInitial) -> Self {
        let mut packet_buf = [0u8; MINIMUM_MAX_DATAGRAM_SIZE as usize];
        let version_packet = packet::version_negotiation::VersionNegotiation::from_initial(
            initial_packet,
            SupportedVersions,
        );

        let mut buffer = EncoderBuffer::new(&mut packet_buf);
        version_packet.encode(&mut buffer);
        let packet_len = buffer.len();

        Self {
            path,
            packet: packet_buf,
            packet_len,
        }
    }
}

impl<Path: path::Handle> AsRef<[u8]> for Transmission<Path> {
    fn as_ref(&self) -> &[u8] {
        &self.packet[..self.packet_len]
    }
}

impl<Path: path::Handle> tx::Message for &Transmission<Path> {
    type Handle = Path;

    fn path_handle(&self) -> &Self::Handle {
        &self.path
    }

    #[inline]
    fn ecn(&mut self) -> ExplicitCongestionNotification {
        Default::default()
    }

    #[inline]
    fn delay(&mut self) -> Duration {
        Default::default()
    }

    #[inline]
    fn ipv6_flow_label(&mut self) -> u32 {
        0
    }

    #[inline]
    fn can_gso(&self, segment_len: usize, _segment_count: usize) -> bool {
        segment_len >= self.as_ref().len()
    }

    #[inline]
    fn write_payload(
        &mut self,
        mut buffer: tx::PayloadBuffer,
        _gso_offset: usize,
    ) -> Result<usize, tx::Error> {
        buffer.write(self.as_ref())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SupportedVersions;

impl EncoderValue for SupportedVersions {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        for version in SUPPORTED_VERSIONS {
            encoder.encode(version);
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-6.3
        //# Endpoints MAY add reserved versions to any field where unknown or
        //# unsupported versions are ignored to test that a peer correctly
        //# ignores the value.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-6.3
        //# Endpoints MAY send packets with a reserved version to test that a
        //# peer correctly discards the packet.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-6.3
        //# For a server to use a new version in the future, clients need to
        //# correctly handle unsupported versions.  Some version numbers
        //# (0x?a?a?a?a as defined in Section 15) are reserved for inclusion in
        //# fields that contain version numbers.
        encoder.encode(&0xdadadadau32);
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;
    use crate::endpoint::testing;
    use core::mem::size_of;
    use s2n_codec::{DecoderBufferMut, Encoder, EncoderBuffer};
    use s2n_quic_core::{
        connection,
        connection::id::ConnectionInfo,
        event::testing::Publisher,
        inet::{DatagramInfo, SocketAddress},
        packet::{
            handshake::Handshake,
            initial::Initial,
            number::{PacketNumberSpace, TruncatedPacketNumber},
            short::Short,
            version_negotiation::VersionNegotiation,
            zero_rtt::ZeroRtt,
        },
        path::RemoteAddress,
        time::clock::testing as time,
        varint::VarInt,
    };

    type Server = Negotiator<testing::Server>;
    type Client = Negotiator<testing::Client>;

    // TAG to append to packet payloads to ensure they meet the minimum packet size
    // that would be expected had they undergone packet protection.
    const DUMMY_TAG: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

    fn datagram_info(payload_len: usize) -> (RemoteAddress, DatagramInfo) {
        (
            RemoteAddress::from(SocketAddress::default()),
            DatagramInfo {
                timestamp: time::now(),
                payload_len,
                ecn: Default::default(),
                destination_connection_id: connection::LocalId::TEST_ID,
                destination_connection_id_classification: connection::id::Classification::Local,
                source_connection_id: None,
            },
        )
    }

    macro_rules! on_packet {
        (
            $negotiator:ident,
            $publisher:ident,
            $remote_address:expr,
            $payload_len:expr,
            $packet:expr
        ) => {{
            let mut buffer = vec![0u8; 1200];
            let mut encoder = EncoderBuffer::new(&mut buffer);

            encoder.encode(&$packet);

            let len = encoder.len();
            let decoder = DecoderBufferMut::new(&mut buffer[..len]);
            let remote_address = SocketAddress::default();
            let connection_info = ConnectionInfo::new(&remote_address);
            let (packet, _) = ProtectedPacket::decode(decoder, &connection_info, &3).unwrap();
            $negotiator.on_packet(&$remote_address, $payload_len, &packet, $publisher)
        }};
    }

    fn on_initial_packet<C: endpoint::Config>(
        datagram_info: (C::PathHandle, DatagramInfo),
        version: u32,
        negotiator: &mut Negotiator<C>,
        publisher: &mut Publisher,
    ) -> Result<(), Error> {
        on_packet!(
            negotiator,
            publisher,
            datagram_info.0,
            datagram_info.1.payload_len,
            Initial {
                version,
                destination_connection_id: &[1u8, 2, 3][..],
                source_connection_id: &[4u8, 5, 6, 7][..],
                token: &[0u8; 0][..],
                packet_number: pn(PacketNumberSpace::Initial),
                payload: &[1u8, 2, 3, 4, 5][..],
            }
        )
    }

    fn on_future_version_initial_packet<C: endpoint::Config>(
        datagram_info: (C::PathHandle, DatagramInfo),
        version: u32,
        negotiator: &mut Negotiator<C>,
        publisher: &mut Publisher,
    ) -> Result<(), Error> {
        let mut payload = vec![1u8, 2, 3, 4, 5];
        payload.extend_from_slice(&DUMMY_TAG[..]);

        on_packet!(
            negotiator,
            publisher,
            datagram_info.0,
            datagram_info.1.payload_len,
            Initial {
                version,
                // Maximum length connection IDs that may be valid in future versions
                destination_connection_id: &[1u8; size_of::<u8>()][..],
                source_connection_id: &[2u8; size_of::<u8>()][..],
                token: &[0u8; 0][..],
                packet_number: pn(PacketNumberSpace::Initial),
                payload: payload.as_slice(),
            }
        )
    }

    fn on_handshake_packet<C: endpoint::Config>(
        datagram_info: (C::PathHandle, DatagramInfo),
        version: u32,
        negotiator: &mut Negotiator<C>,
        publisher: &mut Publisher,
    ) -> Result<(), Error> {
        let mut payload = vec![1u8, 2, 3, 4, 5];
        payload.extend_from_slice(&DUMMY_TAG[..]);

        on_packet!(
            negotiator,
            publisher,
            datagram_info.0,
            datagram_info.1.payload_len,
            Handshake {
                version,
                destination_connection_id: &[1u8, 2, 3][..],
                source_connection_id: &[4u8, 5, 6][..],
                packet_number: pn(PacketNumberSpace::ApplicationData),
                payload: payload.as_slice(),
            }
        )
    }

    fn on_zerortt_packet<C: endpoint::Config>(
        datagram_info: (C::PathHandle, DatagramInfo),
        version: u32,
        negotiator: &mut Negotiator<C>,
        publisher: &mut Publisher,
    ) -> Result<(), Error> {
        let mut payload = vec![1u8, 2, 3, 4, 5];
        payload.extend_from_slice(&DUMMY_TAG[..]);

        on_packet!(
            negotiator,
            publisher,
            datagram_info.0,
            datagram_info.1.payload_len,
            ZeroRtt {
                version,
                destination_connection_id: &[1u8, 2, 3][..],
                source_connection_id: &[4u8, 5, 6][..],
                packet_number: pn(PacketNumberSpace::ApplicationData),
                payload: payload.as_slice(),
            }
        )
    }

    fn on_version_negotiation_packet<C: endpoint::Config>(
        datagram_info: (C::PathHandle, DatagramInfo),
        negotiator: &mut Negotiator<C>,
        publisher: &mut Publisher,
    ) -> Result<(), Error> {
        on_packet!(
            negotiator,
            publisher,
            datagram_info.0,
            datagram_info.1.payload_len,
            VersionNegotiation {
                tag: 0,
                destination_connection_id: &[1u8, 2, 3][..],
                source_connection_id: &[4u8, 5, 6][..],
                supported_versions: SupportedVersions,
            }
        )
    }

    fn on_short_packet<C: endpoint::Config>(
        datagram_info: (C::PathHandle, DatagramInfo),
        negotiator: &mut Negotiator<C>,
        publisher: &mut Publisher,
    ) -> Result<(), Error> {
        let mut payload = vec![1u8, 2, 3, 4, 5];
        payload.extend_from_slice(&DUMMY_TAG[..]);

        on_packet!(
            negotiator,
            publisher,
            datagram_info.0,
            datagram_info.1.payload_len,
            Short {
                destination_connection_id: &[1u8, 2, 3][..],
                key_phase: Default::default(),
                spin_bit: Default::default(),
                packet_number: pn(PacketNumberSpace::ApplicationData),
                payload: payload.as_slice(),
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
        let mut publisher = Publisher::snapshot();

        assert_eq!(
            on_initial_packet(
                datagram_info(1200),
                INVALID_VERSION,
                &mut client,
                &mut publisher
            ),
            Ok(()),
            "client implementation should always allow version negotiator packets through",
        );

        assert_eq!(
            on_zerortt_packet(
                datagram_info(1200),
                INVALID_VERSION,
                &mut client,
                &mut publisher
            ),
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
        let mut publisher = Publisher::snapshot();

        assert_eq!(
            on_initial_packet(
                datagram_info(1200),
                INVALID_VERSION,
                &mut server,
                &mut publisher
            ),
            Err(Error),
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
        let mut publisher = Publisher::snapshot();

        assert_eq!(
            on_future_version_initial_packet(
                datagram_info(1200),
                INVALID_VERSION,
                &mut server,
                &mut publisher
            ),
            Err(Error),
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
        let mut publisher = Publisher::snapshot();

        assert_eq!(
            on_zerortt_packet(
                datagram_info(1200),
                INVALID_VERSION,
                &mut server,
                &mut publisher
            ),
            Err(Error),
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
        let mut publisher = Publisher::snapshot();

        assert_eq!(
            on_initial_packet(
                datagram_info(32),
                INVALID_VERSION,
                &mut server,
                &mut publisher
            ),
            Err(Error),
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
        let mut publisher = Publisher::snapshot();

        for _ in 0..5 {
            assert_eq!(
                on_initial_packet(
                    datagram_info(1200),
                    INVALID_VERSION,
                    &mut server,
                    &mut publisher
                ),
                Err(Error),
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
        let mut publisher = Publisher::snapshot();

        assert_eq!(
            on_version_negotiation_packet(datagram_info(1200), &mut server, &mut publisher),
            Ok(()),
            "server implementations should allow other packets through to connections"
        );

        assert_eq!(
            on_handshake_packet(
                datagram_info(1200),
                INVALID_VERSION,
                &mut server,
                &mut publisher
            ),
            Ok(()),
            "server implementations should allow other packets through to connections"
        );

        assert_eq!(
            on_short_packet(datagram_info(1200), &mut server, &mut publisher),
            Ok(()),
            "server implementations should allow other packets through to connections"
        );

        assert!(
            server.transmissions.is_empty(),
            "servers should not negotiate with version negotiation packets"
        );
    }
}
