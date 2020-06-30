use crate::{
    connection::{
        ConnectionConfig, ConnectionParameters, ConnectionTrait, SynchronizedSharedConnectionState,
    },
    endpoint::{ConnectionIdGenerator, Endpoint, EndpointConfig},
    space::PacketSpaceManager,
};
use alloc::sync::Arc;
use core::{convert::TryInto, time::Duration};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    connection::ConnectionId,
    crypto::{tls::TLSEndpoint, CryptoSuite, InitialCrypto},
    inet::DatagramInfo,
    packet::initial::ProtectedInitial,
    transport::error::TransportError,
    transport_error,
};

//# 14.  Packet Size
//#
//#    The QUIC packet size includes the QUIC header and protected payload,
//#    but not the UDP or IP header.
//#
//#    Clients MUST ensure they send the first Initial packet in a single IP
//#    packet.  Similarly, the first Initial packet sent after receiving a
//#    Retry packet MUST be sent in a single IP packet.
//#
//#    The payload of a UDP datagram carrying the first Initial packet MUST
//#    be expanded to at least 1200 bytes, by adding PADDING frames to the
//#    Initial packet and/or by coalescing the Initial packet (see
//#    Section 12.2).  Sending a UDP datagram of this size ensures that the
//#    network path supports a reasonable Maximum Transmission Unit (MTU),
//#    and helps reduce the amplitude of amplification attacks caused by
//#    server responses toward an unverified client address; see Section 8.

const MINIMUM_INITIAL_PACKET_LEN: usize = 1200;

//#    The datagram containing the first Initial packet from a client MAY
//#    exceed 1200 bytes if the client believes that the Path Maximum
//#    Transmission Unit (PMTU) supports the size that it chooses.
//#
//#    A server MAY send a CONNECTION_CLOSE frame with error code
//#    PROTOCOL_VIOLATION in response to the first Initial packet it
//#    receives from a client if the UDP datagram is smaller than 1200
//#    bytes.  It MUST NOT send any other frame type in response, or
//#    otherwise behave as if any part of the offending packet was processed
//#    as valid.
//#
//#    The server MUST also limit the number of bytes it sends before
//#    validating the address of the client; see Section 8.

//# 7.2.  Negotiating Connection IDs
//#
//#    A connection ID is used to ensure consistent routing of packets, as
//#    described in Section 5.1.  The long header contains two connection
//#    IDs: the Destination Connection ID is chosen by the recipient of the
//#    packet and is used to provide consistent routing; the Source
//#    Connection ID is used to set the Destination Connection ID used by
//#    the peer.
//#
//#    During the handshake, packets with the long header (Section 17.2) are
//#    used to establish the connection ID that each endpoint uses.  Each
//#    endpoint uses the Source Connection ID field to specify the
//#    connection ID that is used in the Destination Connection ID field of
//#    packets being sent to them.  Upon receiving a packet, each endpoint
//#    sets the Destination Connection ID it sends to match the value of the
//#    Source Connection ID that they receive.
//#
//#    When an Initial packet is sent by a client that has not previously
//#    received an Initial or Retry packet from the server, it populates the
//#    Destination Connection ID field with an unpredictable value.  This
//#    MUST be at least 8 bytes in length.  Until a packet is received from
//#    the server, the client MUST use the same value unless it abandons the
//#    connection attempt and starts a new one.  The initial Destination
//#    Connection ID is used to determine packet protection keys for Initial
//#    packets.

const DESTINATION_CONNECTION_ID_MIN_LEN: usize = 8;

//#    The client populates the Source Connection ID field with a value of
//#    its choosing and sets the SCID Len field to indicate the length.
//#
//#    The first flight of 0-RTT packets use the same Destination and Source
//#    Connection ID values as the client's first Initial.
//#
//#    Upon first receiving an Initial or Retry packet from the server, the
//#    client uses the Source Connection ID supplied by the server as the
//#    Destination Connection ID for subsequent packets, including any
//#    subsequent 0-RTT packets.  That means that a client might change the
//#    Destination Connection ID twice during connection establishment, once
//#    in response to a Retry and once in response to the first Initial
//#    packet from the server.  Once a client has received an Initial packet
//#    from the server, it MUST discard any packet it receives with a
//#    different Source Connection ID.
//#
//#    A client MUST only change the value it sends in the Destination
//#    Connection ID in response to the first packet of each type it
//#    receives from the server (Retry or Initial); a server MUST set its
//#    value based on the Initial packet.  Any additional changes are not
//#    permitted; if subsequent packets of those types include a different
//#    Source Connection ID, they MUST be discarded.  This avoids problems
//#    that might arise from stateless processing of multiple Initial
//#    packets producing different connection IDs.
//#
//#    The connection ID can change over the lifetime of a connection,
//#    especially in response to connection migration (Section 9); see
//#    Section 5.1.1 for details.

impl<ConfigType: EndpointConfig> Endpoint<ConfigType> {
    pub(super) fn handle_initial_packet(
        &mut self,
        datagram: &DatagramInfo,
        packet: ProtectedInitial,
        remaining: DecoderBufferMut,
    ) -> Result<(), TransportError> {
        debug_assert!(
            ConfigType::ENDPOINT_TYPE.is_server(),
            "only servers can accept new initial connections"
        );

        // TODO: Validate version
        // TODO: Check that the connection ID is at least 8 byte
        // But maybe we really would need to do this before or inside parsing
        if datagram.payload_len < MINIMUM_INITIAL_PACKET_LEN {
            return Err(transport_error!(PROTOCOL_VIOLATION, "packet too small"));
        }

        let destination_connection_id: ConnectionId =
            packet.destination_connection_id().try_into()?;

        if destination_connection_id.len() < DESTINATION_CONNECTION_ID_MIN_LEN {
            return Err(transport_error!(
                PROTOCOL_VIOLATION,
                "destination connection id too short"
            ));
        }

        let source_connection_id: ConnectionId = packet.source_connection_id().try_into()?;

        // TODO check if we're busy
        // TODO check the version number

        let initial_crypto =
            <<ConfigType::ConnectionConfigType as ConnectionConfig>::TLSSession as CryptoSuite>::InitialCrypto::new_server(
                destination_connection_id.as_bytes(),
            );

        let largest_packet_number = Default::default();
        let packet = packet.unprotect(&initial_crypto, largest_packet_number)?;
        let packet = packet.decrypt(&initial_crypto)?;

        // TODO handle token with stateless retry

        let internal_connection_id = self.connection_id_generator.generate_id();
        let (local_connection_id, _connection_id_expiration) =
            self.local_connection_id_generator.generate_connection_id();
        let mut connection_id_mapper_registration = self
            .connection_id_mapper
            .create_registration(internal_connection_id);
        connection_id_mapper_registration
            .register_connection_id(&local_connection_id)
            .expect("can register connection ID");

        let timer = self.timer_manager.create_timer(
            internal_connection_id,
            datagram.timestamp + Duration::from_secs(3600),
        ); // TODO: Fixme

        let wakeup_handle = self
            .wakeup_queue
            .create_wakeup_handle(internal_connection_id);

        let tls_session = self.tls_endpoint.new_server_session();

        let connection_config = self.config.create_connection_config();

        let connection_parameters = ConnectionParameters {
            connection_config,
            internal_connection_id,
            connection_id_mapper_registration,
            timer,
            peer_connection_id: source_connection_id,
            local_connection_id,
            peer_socket_address: datagram.remote_address,
            timestamp: datagram.timestamp,
            quic_version: packet.version,
        };

        let space_manager =
            PacketSpaceManager::new(tls_session, initial_crypto, datagram.timestamp);

        let shared_state = Arc::new(SynchronizedSharedConnectionState::new(
            space_manager,
            wakeup_handle,
        ));

        let mut connection =
            <ConfigType as EndpointConfig>::ConnectionType::new(connection_parameters);

        // The scope is needed in order to lock the shared state only for a certain duration.
        // It needs to be unlocked when wee insert the connection in our map
        {
            let locked_shared_state = &mut *shared_state.lock();

            connection.handle_cleartext_initial_packet(locked_shared_state, datagram, packet)?;

            connection.handle_remaining_packets(
                locked_shared_state,
                datagram,
                destination_connection_id,
                remaining,
            )?;
        }

        // Only persist the connection if everything is good.
        // Otherwise the connection will automatically get dropped. This
        // will also clean up all state which was already allocated for
        // the connection
        self.connections.insert_connection(connection, shared_state);
        // TODO increment inflight handshakes
        Ok(())
    }
}
