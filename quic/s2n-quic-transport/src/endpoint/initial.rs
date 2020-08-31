use crate::{
    connection::{self, id::Generator as _, SynchronizedSharedConnectionState, Trait as _},
    endpoint,
    space::PacketSpaceManager,
};
use alloc::sync::Arc;
use core::{convert::TryInto, time::Duration};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    crypto::{tls::Endpoint as TLSEndpoint, CryptoSuite, InitialCrypto},
    inet::DatagramInfo,
    packet::initial::ProtectedInitial,
    transport::{error::TransportError, parameters::ServerTransportParameters},
};

impl<Config: endpoint::Config> endpoint::Endpoint<Config> {
    pub(super) fn handle_initial_packet(
        &mut self,
        datagram: &DatagramInfo,
        packet: ProtectedInitial,
        remaining: DecoderBufferMut,
    ) -> Result<(), TransportError> {
        debug_assert!(
            Config::ENDPOINT_TYPE.is_server(),
            "only servers can accept new initial connections"
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#14.1
        //# A client MUST expand the payload of all UDP datagrams carrying
        //# Initial packets to at least the smallest allowed maximum packet size
        //# (1200 bytes) by adding PADDING frames to the Initial packet or by
        //# coalescing the Initial packet
        if datagram.payload_len < 1200 {
            return Err(TransportError::PROTOCOL_VIOLATION.with_reason("packet too small"));
        }

        let destination_connection_id: connection::Id =
            packet.destination_connection_id().try_into()?;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#7.2
        //# When an Initial packet is sent by a client that has not previously
        //# received an Initial or Retry packet from the server, the client
        //# populates the Destination Connection ID field with an unpredictable
        //# value.  This Destination Connection ID MUST be at least 8 bytes in
        //# length.
        if destination_connection_id.len() < 8 {
            return Err(TransportError::PROTOCOL_VIOLATION
                .with_reason("destination connection id too short"));
        }

        let source_connection_id: connection::Id = packet.source_connection_id().try_into()?;

        // TODO check if we're busy
        // TODO check the version number

        let initial_crypto =
            <<Config::ConnectionConfig as connection::Config>::TLSSession as CryptoSuite>::InitialCrypto::new_server(
                destination_connection_id.as_bytes(),
            );

        let largest_packet_number = Default::default();
        let packet = packet.unprotect(&initial_crypto, largest_packet_number)?;
        let packet = packet.decrypt(&initial_crypto)?;

        // TODO handle token with stateless retry

        let internal_connection_id = self.connection_id_generator.generate_id();
        // TODO store the expiration of the connection ID
        let (local_connection_id, _connection_id_expiration) =
            self.config.connection_id_format().generate();

        let mut connection_id_mapper_registration = self
            .connection_id_mapper
            .create_registration(internal_connection_id);

        connection_id_mapper_registration
            .register_connection_id(&local_connection_id)
            .expect("can register connection ID");

        let timer = self.timer_manager.create_timer(
            internal_connection_id,
            datagram.timestamp + Duration::from_secs(3600),
        ); // TODO: make it so we don't arm for a given time and immediately change it

        let wakeup_handle = self
            .wakeup_queue
            .create_wakeup_handle(internal_connection_id);

        // TODO initialize transport parameters from provider values
        // TODO pass connection_ids for authentication
        let transport_parameters = ServerTransportParameters::default();

        let tls_session = self
            .config
            .tls_endpoint()
            .new_server_session(&transport_parameters);

        let connection_config = self.config.create_connection_config();

        let connection_parameters = connection::Parameters {
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

        let mut connection = <Config as endpoint::Config>::Connection::new(connection_parameters);

        // The scope is needed in order to lock the shared state only for a certain duration.
        // It needs to be unlocked when wee insert the connection in our map
        {
            let locked_shared_state = &mut *shared_state.lock();

            connection.handle_cleartext_initial_packet(locked_shared_state, datagram, packet)?;

            connection.handle_remaining_packets(
                locked_shared_state,
                datagram,
                destination_connection_id,
                self.config.connection_id_format(),
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
