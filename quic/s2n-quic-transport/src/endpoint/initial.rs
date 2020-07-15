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

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#14
//# A client MUST expand the payload of all UDP datagrams carrying
//# Initial packets to at least 1200 bytes, by adding PADDING frames to
//# the Initial packet or by coalescing the Initial packet (see
//# Section 12.2).

const MINIMUM_INITIAL_PACKET_LEN: usize = 1200;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#7.2
//# When an Initial packet is sent by a client that has not previously
//# received an Initial or Retry packet from the server, it populates the
//# Destination Connection ID field with an unpredictable value.  This
//# MUST be at least 8 bytes in length.

const DESTINATION_CONNECTION_ID_MIN_LEN: usize = 8;

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
