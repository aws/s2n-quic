//! Contains the implementation of the `Connection`

use crate::{
    connection::{
        self, CloseReason as ConnectionCloseReason, ConnectionIdMapperRegistration,
        ConnectionInterests, ConnectionTimerEntry, ConnectionTimers, ConnectionTransmission,
        ConnectionTransmissionContext, InternalConnectionId, Parameters as ConnectionParameters,
        SharedConnectionState,
    },
    contexts::ConnectionOnTransmitError,
};
use core::time::Duration;
use s2n_quic_core::{
    application::ApplicationErrorExt,
    inet::DatagramInfo,
    io::tx,
    packet::{
        handshake::ProtectedHandshake,
        initial::{CleartextInitial, ProtectedInitial},
        retry::ProtectedRetry,
        short::ProtectedShort,
        version_negotiation::ProtectedVersionNegotiation,
        zero_rtt::ProtectedZeroRTT,
    },
    path_manager::PathManager,
    time::Timestamp,
    transport::error::TransportError,
};

/// Possible states for handing over a connection from the endpoint to the
/// application.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum AcceptState {
    /// The connection is handshaking on the server side and not yet visible
    /// to the application.
    Handshaking,
    /// The connection has completed the handshake but hasn't been handed over
    /// to the application yet.
    HandshakeCompleted,
    /// The connection has been handed over to the application and can be
    /// actively utilized from there.
    Active,
}

/// Possible states of a connection
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ConnectionState {
    /// The connection is performing the handshake
    Handshaking,
    /// The connection is active
    Active,
    /// The connection is closing, as described in
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#10.1
    Closing,
    /// The connection is draining, as described in
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#10.1
    Draining,
    /// The connection was drained, and is in its terminal state.
    /// The connection will be removed from the endpoint when it reached this state.
    Finished,
}

impl<'a> From<ConnectionCloseReason<'a>> for ConnectionState {
    fn from(close_reason: ConnectionCloseReason<'a>) -> Self {
        match close_reason {
            ConnectionCloseReason::IdleTimerExpired => {
                // If the idle timer expired we directly move into the final state
                ConnectionState::Finished
            }
            ConnectionCloseReason::LocalImmediateClose(_error) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#10.1
                //# An endpoint enters a closing period after initiating an immediate close (Section 10.3).
                ConnectionState::Closing
            }
            ConnectionCloseReason::PeerImmediateClose(_error) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#10.1
                //# The draining state is entered once an endpoint receives a signal that its peer is closing or draining.
                ConnectionState::Draining
            }
            ConnectionCloseReason::LocalObservedTransportErrror(_error) => {
                // Since the local side observes the error, it initiates the close
                // Therefore this is similar to an application initiated close
                ConnectionState::Closing
            }
        }
    }
}

pub struct ConnectionImpl<ConfigType: connection::Config> {
    /// The configuration of this connection
    config: ConfigType,
    /// The [`Connection`]s internal identifier
    internal_connection_id: InternalConnectionId,
    /// The connection ID mapper registration which should be utilized by the connection
    #[allow(dead_code)] // TODO: temporary supression until connections support ID registration
    connection_id_mapper_registration: ConnectionIdMapperRegistration,
    /// The timers which are used within the connection
    timers: ConnectionTimers,
    /// The timer entry in the endpoint timer list
    timer_entry: ConnectionTimerEntry,
    /// The QUIC protocol version which is used for this particular connection
    quic_version: u32,
    /// Describes whether the connection is known to be accepted by the application
    accept_state: AcceptState,
    /// The current state of the connection
    state: ConnectionState,
    /// Manage the paths that the connection could use
    path_manager: PathManager,
}

impl<ConfigType: connection::Config> ConnectionImpl<ConfigType> {
    fn update_crypto_state(
        &mut self,
        shared_state: &mut SharedConnectionState<ConfigType>,
        datagram: &DatagramInfo,
    ) -> Result<(), TransportError> {
        let space_manager = &mut shared_state.space_manager;
        space_manager.poll_crypto(&self.config, datagram.timestamp)?;

        if matches!(self.state, ConnectionState::Handshaking)
            && space_manager.application().is_some()
        {
            // Move into the HandshakeCompleted state. This will signal the
            // necessary interest to hand over the connection to the application.
            self.accept_state = AcceptState::HandshakeCompleted;
            // Move the connection into the active state.
            self.state = ConnectionState::Active;

            // Since we now have all transport parameters, we start the idle timer
            self.restart_peer_idle_timer(datagram.timestamp);
        }

        Ok(())
    }

    /// Returns the idle timeout based on transport parameters of both peers
    fn get_idle_timer_duration(&self) -> Duration {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#10.2
        //# Each endpoint advertises a max_idle_timeout, but the effective value
        //# at an endpoint is computed as the minimum of the two advertised
        //# values.  By announcing a max_idle_timeout, an endpoint commits to
        //# initiating an immediate close (Section 10.3) if it abandons the
        //# connection prior to the effective value.

        // TODO: Derive this from transport parameters
        Duration::from_millis(5000)
    }

    fn restart_peer_idle_timer(&mut self, timestamp: Timestamp) {
        self.timers
            .peer_idle_timer
            .set(timestamp + self.get_idle_timer_duration())
    }
}

/// Creates a closure which unprotects and decrypts packets for a given space.
///
/// This is a macro instead of a function because it removes the need to have a
/// complex trait with a bunch of generics for each of the packet spaces.
macro_rules! packet_validator {
    ($packet:ident) => {
        move |space| {
            let crypto = &space.crypto;
            let packet_number_decoder = space.packet_number_decoder();

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-27.txt#5.5
            //# Failure to unprotect a packet does not necessarily indicate the
            //# existence of a protocol error in a peer or an attack.

            // In this case we silently drop the packet
            let packet = $packet.unprotect(crypto, packet_number_decoder).ok()?;

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#12.3
            //# A receiver MUST discard a newly unprotected packet unless it is
            //# certain that it has not processed another packet with the same packet
            //# number from the same packet number space.
            if space.is_duplicate(packet.packet_number) {
                return None;
            }

            let packet = packet.decrypt(crypto).ok()?;

            Some(packet)
        }
    };
}

impl<ConfigType: connection::Config> connection::Trait for ConnectionImpl<ConfigType> {
    /// Static configuration of a connection
    type Config = ConfigType;

    /// Creates a new `Connection` instance with the given configuration
    fn new(parameters: ConnectionParameters<Self::Config>) -> Self {
        // The path manager always starts with a single path containing the known peer and local
        // connetion ids.
        let path_manager: PathManager = PathManager::default();

        Self {
            config: parameters.connection_config,
            internal_connection_id: parameters.internal_connection_id,
            connection_id_mapper_registration: parameters.connection_id_mapper_registration,
            timers: Default::default(),
            timer_entry: parameters.timer,
            quic_version: parameters.quic_version,
            accept_state: AcceptState::Handshaking,
            state: ConnectionState::Handshaking,
            path_manager,
        }
    }

    /// Returns the connections configuration
    fn config(&self) -> &Self::Config {
        &self.config
    }

    /// Returns the Connections internal ID
    fn internal_connection_id(&self) -> InternalConnectionId {
        self.internal_connection_id
    }

    /// Initiates closing the connection as described in
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#10
    ///
    /// This method can be called for any of the close reasons:
    /// - Idle timeout
    /// - Immediate close
    fn close(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        close_reason: ConnectionCloseReason,
        timestamp: Timestamp,
    ) {
        match self.state {
            ConnectionState::Closing | ConnectionState::Draining | ConnectionState::Finished => {
                // The connection is already closing
                return;
            }
            ConnectionState::Handshaking | ConnectionState::Active => {}
        }

        // TODO: Rember close reason
        // TODO: Build a CONNECTION_CLOSE frame based on the keys that are available
        // at the moment. We need to use the highest set of available keys as
        // described in https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#10.3

        // We are not interested in this timer anymore
        // TODO: There might be more such timers need to get added in the future
        self.timers.peer_idle_timer.cancel();
        self.state = close_reason.into();

        shared_state.space_manager.discard_initial();
        shared_state.space_manager.discard_handshake();
        shared_state.space_manager.discard_zero_rtt_crypto();
        if let Some(application) = shared_state.space_manager.application_mut() {
            // Close all streams with the derived error
            application.stream_manager.close(close_reason.into());
        }
        // TODO: Discard application state?

        if let ConnectionState::Closing | ConnectionState::Draining = self.state {
            // Start closing/draining timer
            // TODO: The time should be coming from config + PTO estimation
            let delay = core::time::Duration::from_millis(100);
            self.timers.close_timer.set(timestamp + delay);
        }
    }

    /// Queries the connection for outgoing packets
    fn on_transmit<Tx: tx::Queue>(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        queue: &mut Tx,
        timestamp: Timestamp,
    ) -> Result<(), ConnectionOnTransmitError> {
        let mut count = 0;

        match self.state {
            ConnectionState::Handshaking | ConnectionState::Active => {
                let ecn = Default::default();

                while queue
                    .push(ConnectionTransmission {
                        context: ConnectionTransmissionContext {
                            quic_version: self.quic_version,
                            timestamp,
                            local_endpoint_type: Self::Config::ENDPOINT_TYPE,
                            path: self.path_manager.get_active_path_mut(),
                            ecn,
                        },
                        shared_state,
                    })
                    .is_ok()
                {
                    count += 1;
                }
            }
            ConnectionState::Closing => {
                // We are only allowed to send CONNECTION_CLOSE frames in this
                // state.
                // TODO: Ask the ConnectionCloseSender to send data
            }
            ConnectionState::Draining | ConnectionState::Finished => {
                // We are not allowed to send any data in this states
            }
        }

        if count == 0 {
            Err(ConnectionOnTransmitError::NoDatagram)
        } else {
            Ok(())
        }
    }

    /// Handles all timeouts on the `Connection`.
    ///
    /// `timestamp` passes the current time.
    fn on_timeout(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        timestamp: Timestamp,
    ) {
        if self
            .timers
            .peer_idle_timer
            .poll_expiration(timestamp)
            .is_ready()
        {
            self.close(
                shared_state,
                ConnectionCloseReason::IdleTimerExpired,
                timestamp,
            );
        }

        if self
            .timers
            .close_timer
            .poll_expiration(timestamp)
            .is_ready()
        {
            if let ConnectionState::Closing | ConnectionState::Draining = self.state {
                self.state = ConnectionState::Finished;
            }
        }

        shared_state.space_manager.on_timeout(timestamp);
    }

    /// Updates the per-connection timer based on individual component timers.
    /// This method is used in order to update the connection timer only once
    /// per interaction with the connection and thereby to batch timer updates.
    fn update_connection_timer(&mut self, shared_state: &mut SharedConnectionState<Self::Config>) {
        // find the earliest armed timer
        let earliest = core::iter::empty()
            .chain(self.timers.iter())
            .chain(shared_state.space_manager.timers())
            .min()
            .cloned();

        self.timer_entry.update(earliest);
    }

    /// Handles all external wakeups on the [`Connection`].
    fn on_wakeup(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        timestamp: Timestamp,
    ) {
        // This method is intentionally mostly empty at the moment. The most important thing on a
        // wakeup is that the connection manager synchronizes the interests of the individual connection.
        // This will happen automatically through the [`interests()`] call after the [`Connection`]
        // was accessed. Therefore we do not have to do anything special here.

        // For active connections we have to check if the application requested
        // to close them
        if self.state == ConnectionState::Active {
            if let Some(application) = shared_state.space_manager.application_mut() {
                if let Some(stream_error) = application.stream_manager.close_reason() {
                    // A connection close was requested. This needs to have an
                    // associated error code which can be used as `TransportError`
                    let error_code = stream_error.application_error_code().expect(concat!(
                        "The connection should only be closeable through an ",
                        "API call which submits an error code while active"
                    ));
                    self.close(
                        shared_state,
                        ConnectionCloseReason::LocalImmediateClose(error_code),
                        timestamp,
                    );
                }
            }
        }

        shared_state.wakeup_handle.wakeup_handled();
    }

    // Packet handling
    fn on_datagram_received(
        &mut self,
        _shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
    ) -> Result<(), TransportError> {
        self.path_manager
            .get_active_path_mut()
            .on_bytes_received(datagram.payload_len as u32);
        Ok(())
    }

    /// Is called when a initial packet had been received
    fn handle_initial_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: ProtectedInitial,
    ) -> Result<(), TransportError> {
        if let Some(packet) = shared_state
            .space_manager
            .initial_mut()
            .and_then(packet_validator!(packet))
        {
            // We get here from transport::endpoint::receive_datagram. Whether the connection
            // was known or not, this function is called. So we can verify whether this is a new
            // connection's initial packet, or an existing connection being migrated to a new
            // endpoint.
            if self.path_manager.is_new_path(
                &datagram.remote_address,
                &connection::Id::try_from_bytes(packet.destination_connection_id).unwrap(),
            ) && self.state == ConnectionState::Handshaking
            {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-29#9
                //# The design of QUIC relies on endpoints retaining a stable address
                //# for the duration of the handshake.  An endpoint MUST NOT initiate
                //# connection migration before the handshake is confirmed, as defined
                //# in section 4.1.2 of [QUIC-TLS].
                return Err(TransportError::PROTOCOL_VIOLATION);
            }

            // The path is already known
            self.handle_cleartext_initial_packet(shared_state, datagram, packet)?;

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#10.2
            //# An endpoint restarts its idle timer when a packet from its peer is
            //# received and processed successfully.
            self.restart_peer_idle_timer(datagram.timestamp);
        }

        Ok(())
    }

    /// Is called when an unprotected initial packet had been received
    fn handle_cleartext_initial_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: CleartextInitial,
    ) -> Result<(), TransportError> {
        self.handle_cleartext_packet(shared_state, datagram, packet)?;

        // try to move the crypto state machine forward
        self.update_crypto_state(shared_state, datagram)?;

        Ok(())
    }

    /// Is called when a handshake packet had been received
    fn handle_handshake_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: ProtectedHandshake,
    ) -> Result<(), TransportError> {
        if let Some(packet) = shared_state
            .space_manager
            .handshake_mut()
            .and_then(packet_validator!(packet))
        {
            self.handle_cleartext_packet(shared_state, datagram, packet)?;

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-27.txt#4.10.1
            //# A server MUST discard Initial keys when it first successfully
            //# processes a Handshake packet.

            if Self::Config::ENDPOINT_TYPE.is_server() {
                shared_state.space_manager.discard_initial();
            }

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#10.2
            //# An endpoint restarts its idle timer when a packet from its peer is
            //# received and processed successfully.
            self.restart_peer_idle_timer(datagram.timestamp);

            // try to move the crypto state machine forward
            self.update_crypto_state(shared_state, datagram)?;
        };

        Ok(())
    }

    /// Is called when a short packet had been received
    fn handle_short_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: ProtectedShort,
    ) -> Result<(), TransportError> {
        if let Some(packet) = shared_state
            .space_manager
            .application_mut()
            .and_then(packet_validator!(packet))
        {
            self.handle_cleartext_packet(shared_state, datagram, packet)?;

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#10.2
            //# An endpoint restarts its idle timer when a packet from its peer is
            //# received and processed successfully.
            self.restart_peer_idle_timer(datagram.timestamp);

            // Currently, the application space does not have any crypto state.
            // If, at some point, we decide to add it, we need to call `update_crypto_state` here.
        };

        Ok(())
    }

    /// Is called when a version negotiation packet had been received
    fn handle_version_negotiation_packet(
        &mut self,
        _shared_state: &mut SharedConnectionState<Self::Config>,
        _datagram: &DatagramInfo,
        _packet: ProtectedVersionNegotiation,
    ) -> Result<(), TransportError> {
        // TODO
        Ok(())
    }

    /// Is called when a zero rtt packet had been received
    fn handle_zero_rtt_packet(
        &mut self,
        _shared_state: &mut SharedConnectionState<Self::Config>,
        _datagram: &DatagramInfo,
        _packet: ProtectedZeroRTT,
    ) -> Result<(), TransportError> {
        // TODO
        Ok(())
    }

    /// Is called when a retry packet had been received
    fn handle_retry_packet(
        &mut self,
        _shared_state: &mut SharedConnectionState<Self::Config>,
        _datagram: &DatagramInfo,
        _packet: ProtectedRetry,
    ) -> Result<(), TransportError> {
        // TODO
        Ok(())
    }

    fn handle_transport_error(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        transport_error: TransportError,
    ) {
        dbg!(&transport_error);
        self.close(
            shared_state,
            ConnectionCloseReason::LocalObservedTransportErrror(transport_error),
            datagram.timestamp,
        );
    }

    fn mark_as_accepted(&mut self) {
        debug_assert!(
            self.accept_state == AcceptState::HandshakeCompleted,
            "mark_accepted() should only be called on connections which have finished the handshake");
        self.accept_state = AcceptState::Active;
    }

    fn interests(&self, shared_state: &SharedConnectionState<Self::Config>) -> ConnectionInterests {
        let mut interests = ConnectionInterests::default();

        if self.accept_state == AcceptState::HandshakeCompleted {
            interests.accept = true;
        }

        match self.state {
            ConnectionState::Active | ConnectionState::Handshaking => {
                interests += shared_state.space_manager.interests();
            }
            ConnectionState::Closing => {
                // TODO: Ask the Close Sender whether it needs to transmit
            }
            ConnectionState::Draining => {
                // This is a pure wait state. We do not want to transmit data here
            }
            ConnectionState::Finished => {
                // Remove the connection from the endpoint
                interests.finalization = true;
            }
        }

        interests
    }
}
