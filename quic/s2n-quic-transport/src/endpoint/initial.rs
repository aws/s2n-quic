use crate::{
    connection::{
        self,
        id::{ConnectionInfo, Generator as _},
        SynchronizedSharedConnectionState, Trait as _,
    },
    endpoint,
    recovery::congestion_controller::{self, Endpoint as _},
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

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.1
        //# A client MUST expand the payload of all UDP datagrams carrying
        //# Initial packets to at least the smallest allowed maximum datagram
        //# size of 1200 bytes
        if datagram.payload_len < 1200 {
            return Err(TransportError::PROTOCOL_VIOLATION.with_reason("packet too small"));
        }

        let destination_connection_id: connection::Id =
            packet.destination_connection_id().try_into()?;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.2
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

        let initial_crypto =
            <<Config::ConnectionConfig as connection::Config>::TLSSession as CryptoSuite>::InitialCrypto::new_server(
                destination_connection_id.as_bytes(),
            );

        let largest_packet_number = Default::default();
        let packet = packet.unprotect(&initial_crypto, largest_packet_number)?;
        let packet = packet.decrypt(&initial_crypto)?;

        // TODO handle token with stateless retry

        let internal_connection_id = self.connection_id_generator.generate_id();

        let connection_info = ConnectionInfo::new(&datagram.remote_address);
        let endpoint_context = self.config.context();

        let initial_connection_id = endpoint_context
            .connection_id_format
            .generate(&connection_info);

        let connection_id_expiration = endpoint_context
            .connection_id_format
            .lifetime()
            .map(|lifetime| datagram.timestamp + lifetime);

        let connection_id_mapper_registration = self.connection_id_mapper.create_registration(
            internal_connection_id,
            &initial_connection_id,
            connection_id_expiration,
        );

        let timer = self.timer_manager.create_timer(
            internal_connection_id,
            datagram.timestamp + Duration::from_secs(3600),
        ); // TODO: make it so we don't arm for a given time and immediately change it

        let wakeup_handle = self
            .wakeup_queue
            .create_wakeup_handle(internal_connection_id);

        let mut transport_parameters = ServerTransportParameters::default();

        // TODO initialize transport parameters from Limits provider values
        let max = s2n_quic_core::varint::VarInt::from_u32(core::u32::MAX);
        transport_parameters.initial_max_data = max.try_into().unwrap();
        transport_parameters.initial_max_stream_data_bidi_local = max.try_into().unwrap();
        transport_parameters.initial_max_stream_data_bidi_remote = max.try_into().unwrap();
        transport_parameters.initial_max_stream_data_bidi_remote = max.try_into().unwrap();
        transport_parameters.initial_max_stream_data_uni = max.try_into().unwrap();
        transport_parameters.initial_max_streams_bidi = max.try_into().unwrap();
        transport_parameters.initial_max_streams_uni = max.try_into().unwrap();

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.3
        //# A server includes the Destination Connection ID field from the first
        //# Initial packet it received from the client in the
        //# original_destination_connection_id transport parameter
        transport_parameters.original_destination_connection_id = destination_connection_id
            .try_into()
            .expect("connection ID already validated");

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.3
        //# Each endpoint includes the value of the Source Connection ID field
        //# from the first Initial packet it sent in the
        //# initial_source_connection_id transport parameter
        transport_parameters.initial_source_connection_id = initial_connection_id
            .try_into()
            .expect("connection ID already validated");

        // TODO send retry_source_connection_id
        let tls_session = endpoint_context
            .tls
            .new_server_session(&transport_parameters);

        let path_info = congestion_controller::PathInfo::new(&datagram.remote_address);
        let congestion_controller = endpoint_context
            .congestion_controller
            .new_congestion_controller(path_info);

        let connection_config = self.config.create_connection_config();

        let connection_parameters = connection::Parameters {
            connection_config,
            internal_connection_id,
            connection_id_mapper_registration,
            timer,
            peer_connection_id: source_connection_id,
            local_connection_id: initial_connection_id,
            peer_socket_address: datagram.remote_address,
            congestion_controller,
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
        // It needs to be unlocked when we insert the connection in our map
        {
            let locked_shared_state = &mut *shared_state.lock();

            let endpoint_context = self.config.context();

            let path_id = connection.on_datagram_received(
                locked_shared_state,
                datagram,
                &source_connection_id,
                endpoint_context.congestion_controller,
            )?;

            connection.handle_cleartext_initial_packet(
                locked_shared_state,
                datagram,
                path_id,
                packet,
            )?;

            connection.handle_remaining_packets(
                locked_shared_state,
                datagram,
                path_id,
                destination_connection_id,
                endpoint_context.connection_id_format,
                remaining,
            )?;
        }

        // Only persist the connection if everything is good.
        // Otherwise the connection will automatically get dropped. This
        // will also clean up all state which was already allocated for
        // the connection
        self.connections.insert_connection(connection, shared_state);

        // The handshake has begun and we should start tracking it
        self.limits_manager.on_handshake_start();

        Ok(())
    }
}
