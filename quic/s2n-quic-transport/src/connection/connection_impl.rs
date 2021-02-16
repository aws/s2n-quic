//! Contains the implementation of the `Connection`

use crate::{
    connection::{
        self, id::ConnectionInfo, limits::Limits, local_id_registry::LocalIdRegistrationError,
        CloseReason as ConnectionCloseReason, ConnectionIdMapper, ConnectionInterests,
        ConnectionTimerEntry, ConnectionTimers, ConnectionTransmission,
        ConnectionTransmissionContext, InternalConnectionId, Parameters as ConnectionParameters,
        ProcessingError, SharedConnectionState,
    },
    contexts::ConnectionOnTransmitError,
    path,
    recovery::{congestion_controller, RTTEstimator},
    space::PacketSpace,
    transmission,
};
use core::time::Duration;
use s2n_quic_core::{
    application::ApplicationErrorExt,
    connection::id::Interest,
    crypto::CryptoError,
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
    stateless_reset,
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
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.1
    Closing,
    /// The connection is draining, as described in
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.1
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
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.2.1
                //# An endpoint enters the closing state after initiating an immediate
                //# close.
                ConnectionState::Closing
            }
            ConnectionCloseReason::PeerImmediateClose(_error) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.2.2
                //# The draining state is entered once an endpoint receives a
                //# CONNECTION_CLOSE frame, which indicates that its peer is closing or
                //# draining.
                ConnectionState::Draining
            }
            ConnectionCloseReason::LocalObservedTransportErrror(_error) => {
                // Since the local side observes the error, it initiates the close
                // Therefore this is similar to an application initiated close
                ConnectionState::Closing
            }
            ConnectionCloseReason::StatelessReset => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
                //# If the last 16 bytes of the datagram are identical in value to a
                //# Stateless Reset Token, the endpoint MUST enter the draining period
                //# and not send any further packets on this connection.
                ConnectionState::Draining
            }
        }
    }
}

#[derive(Debug)]
pub struct ConnectionImpl<Config: connection::Config> {
    /// The configuration of this connection
    config: Config,
    /// The [`Connection`]s internal identifier
    internal_connection_id: InternalConnectionId,
    /// The connection ID to send packets from
    local_connection_id: connection::LocalId,
    /// The local ID registry which should be utilized by the connection
    local_id_registry: connection::LocalIdRegistry,
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
    path_manager: path::Manager<Config::CongestionController>,
    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //# In addition to counting packets sent, endpoints MUST count the number
    //# of received packets that fail authentication during the lifetime of a
    //# connection.
    packet_decryption_failures: u64,
    /// The limits applied to the current connection
    limits: Limits,
}

#[cfg(debug_assertions)]
impl<Config: connection::Config> Drop for ConnectionImpl<Config> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("\nLast known connection state: \n {:#?}", self);
        }
    }
}

/// Unprotects and decrypts packets for the given space.
///
/// This is a macro instead of a function because it removes the need to have a
/// complex trait with a bunch of generics for each of the packet spaces.
macro_rules! packet_validator {
    ($conn:ident, $packet:ident, $space:expr $(, $inspect:expr)?) => {{
        if let Some((space, handshake_status)) = $space {
            let crypto = space.crypto();
            let packet_number_decoder = space.packet_number_decoder();

            // TODO ensure this is all side-channel free and reserved bits are 0
            // https://github.com/awslabs/s2n-quic/issues/212

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.5
            //# Failure to unprotect a packet does not necessarily indicate the
            //# existence of a protocol error in a peer or an attack.

            // It may indicate the packet is a stateless reset however, so we will bubble
            // up the error to allow the caller to handle it.
            let $packet = crypto.unprotect_packet(|key|
                $packet.unprotect(key, packet_number_decoder)
            )?;
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#12.3
            //# A receiver MUST discard a newly unprotected packet unless it is
            //# certain that it has not processed another packet with the same packet
            //# number from the same packet number space.
            if space.is_duplicate($packet.packet_number) {
                None
            } else {
                $($inspect)?
                let phased_crypto = space.crypto_for_phase($packet.key_phase());

                match phased_crypto.decrypt_packet($conn, |key| {
                    $packet.decrypt(key)
                }) {
                    Ok(packet) => {
                        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.4
                        //= type=TODO
                        //= tracking-issue=479
                        //= feature=Key update
                        //# An endpoint that successfully removes protection with old
                        //# keys when newer keys were used for packets with lower packet numbers
                        //# MUST treat this as a connection error of type KEY_UPDATE_ERROR.
                        Some((packet, space, handshake_status))
                    }
                    Err(e) => {
                        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.4
                        //= type=TODO
                        //= tracking-issue=479
                        //= feature=Key update
                        //# Packets with higher packet numbers MUST be protected with either the
                        //# same or newer packet protection keys than packets with lower packet
                        //# numbers.
                        return Err(e)
                    }
                }
            }
        } else {
            None
        }
    }};
}

impl<ConfigType: connection::Config> ConnectionImpl<ConfigType> {
    fn update_crypto_state(
        &mut self,
        shared_state: &mut SharedConnectionState<ConfigType>,
        datagram: &DatagramInfo,
    ) -> Result<(), TransportError> {
        let space_manager = &mut shared_state.space_manager;
        space_manager.poll_crypto(
            &self.config,
            self.path_manager.active_path(),
            &mut self.local_id_registry,
            &self.limits,
            datagram.timestamp,
        )?;

        if matches!(self.state, ConnectionState::Handshaking)
            && space_manager.is_handshake_confirmed()
        {
            // Move into the HandshakeCompleted state. This will signal the
            // necessary interest to hand over the connection to the application.
            self.accept_state = AcceptState::HandshakeCompleted;
            // Move the connection into the active state.
            self.state = ConnectionState::Active;

            // Since we now have all transport parameters, we start the idle timer
            self.restart_peer_idle_timer(datagram.timestamp);

            // We don't expect any further initial packets on this connection, so start
            // a timer to remove the mapping from the initial ID to the internal connection ID
            // to give time for any delayed initial packets to arrive.
            if ConfigType::ENDPOINT_TYPE.is_server() {
                self.start_initial_id_timer(datagram.timestamp);
            }
        }

        Ok(())
    }

    /// Returns the idle timeout based on transport parameters of both peers
    fn get_idle_timer_duration(&self) -> Duration {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.1
        //= type=TODO
        //# Each endpoint advertises a max_idle_timeout, but the effective value
        //# at an endpoint is computed as the minimum of the two advertised
        //# values.  By announcing a max_idle_timeout, an endpoint commits to
        //# initiating an immediate close (Section 10.2) if it abandons the
        //# connection prior to the effective value.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.1
        //= type=TODO
        //# To avoid excessively small idle timeout periods, endpoints MUST
        //# increase the idle timeout period to be at least three times the
        //# current Probe Timeout (PTO).  This allows for multiple PTOs to
        //# expire, and therefore multiple probes to be sent and lost, prior to
        //# idle timeout.

        // TODO: Derive this from transport parameters and pto
        Duration::from_secs(30)
    }

    fn restart_peer_idle_timer(&mut self, timestamp: Timestamp) {
        self.timers
            .peer_idle_timer
            .set(timestamp + self.get_idle_timer_duration())
    }

    fn start_initial_id_timer(&mut self, timestamp: Timestamp) {
        self.timers
            .initial_id_expiration_timer
            .set(timestamp + self.get_idle_timer_duration())
    }
}

impl<Config: connection::Config> connection::AeadIntegrityLimitTracking for ConnectionImpl<Config> {
    fn on_decryption_error(&mut self) {
        self.packet_decryption_failures += 1
    }

    fn decryption_error_count(&self) -> u64 {
        self.packet_decryption_failures
    }
}

impl<Config: connection::Config> connection::Trait for ConnectionImpl<Config> {
    /// Static configuration of a connection
    type Config = Config;

    fn is_handshaking(&self) -> bool {
        self.accept_state == AcceptState::Handshaking
    }

    /// Creates a new `Connection` instance with the given configuration
    fn new(parameters: ConnectionParameters<Self::Config>) -> Self {
        // The path manager always starts with a single path containing the known peer and local
        // connection ids.
        let rtt_estimator = RTTEstimator::new(parameters.limits.ack_settings().max_ack_delay);
        // Assume clients validate the server's address implicitly.
        let peer_validated = Self::Config::ENDPOINT_TYPE.is_server();
        let initial_path = path::Path::new(
            parameters.peer_socket_address,
            parameters.peer_connection_id,
            rtt_estimator,
            parameters.congestion_controller,
            peer_validated,
        );

        let path_manager = path::Manager::new(initial_path, parameters.peer_id_registry);

        Self {
            config: parameters.connection_config,
            internal_connection_id: parameters.internal_connection_id,
            local_connection_id: parameters.local_connection_id,
            local_id_registry: parameters.local_id_registry,
            timers: Default::default(),
            timer_entry: parameters.timer,
            quic_version: parameters.quic_version,
            accept_state: AcceptState::Handshaking,
            state: ConnectionState::Handshaking,
            path_manager,
            packet_decryption_failures: 0,
            limits: parameters.limits,
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

    /// Returns the QUIC version selected for the current connection
    fn quic_version(&self) -> u32 {
        self.quic_version
    }

    /// Initiates closing the connection as described in
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10
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
            ConnectionState::Handshaking => {
                // TODO: Decrement the inflight handshake counter
                // https://github.com/awslabs/s2n-quic/issues/162
            }
            ConnectionState::Active => {}
        }

        // TODO: Rember close reason
        // TODO: Build a CONNECTION_CLOSE frame based on the keys that are available
        // at the moment. We need to use the highest set of available keys as
        // described in https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3

        // We are not interested in this timer anymore
        // TODO: There might be more such timers need to get added in the future
        self.timers.peer_idle_timer.cancel();
        self.state = close_reason.into();

        shared_state
            .space_manager
            .discard_initial(self.path_manager.active_path_mut());
        shared_state
            .space_manager
            .discard_handshake(self.path_manager.active_path_mut());
        shared_state.space_manager.discard_zero_rtt_crypto();
        if let Some((application, _handshake_status)) = shared_state.space_manager.application_mut()
        {
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

    /// Generates and registers new connection IDs using the given `ConnectionIdFormat`
    fn on_new_connection_id<
        ConnectionIdFormat: connection::id::Format,
        StatelessResetTokenGenerator: stateless_reset::token::Generator,
    >(
        &mut self,
        connection_id_format: &mut ConnectionIdFormat,
        stateless_reset_token_generator: &mut StatelessResetTokenGenerator,
        timestamp: Timestamp,
    ) -> Result<(), LocalIdRegistrationError> {
        match self.local_id_registry.connection_id_interest() {
            Interest::New(mut count) => {
                let connection_info =
                    ConnectionInfo::new(&self.path_manager.active_path().peer_socket_address);

                while count > 0 {
                    let id = connection_id_format.generate(&connection_info);
                    let expiration = connection_id_format
                        .lifetime()
                        .map(|duration| timestamp + duration);
                    let stateless_reset_token = stateless_reset_token_generator.generate(&id);
                    self.local_id_registry.register_connection_id(
                        &id,
                        expiration,
                        stateless_reset_token,
                    )?;
                    count -= 1;
                }
                Ok(())
            }
            Interest::None => Ok(()),
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
                // TODO pull these from somewhere
                let ecn = Default::default();

                debug_assert!(
                    !self.path_manager.active_path().at_amplification_limit(),
                    "connection should not express transmission interest if amplification limited"
                );

                while let Ok(_idx) = queue.push(ConnectionTransmission {
                    context: ConnectionTransmissionContext {
                        quic_version: self.quic_version,
                        timestamp,
                        path_id: self.path_manager.active_path_id(),
                        path_manager: &mut self.path_manager,
                        source_connection_id: &self.local_connection_id,
                        local_id_registry: &mut self.local_id_registry,
                        ecn,
                    },
                    shared_state,
                }) {
                    count += 1;
                    if self.path_manager.active_path().at_amplification_limit() {
                        break;
                    }
                }
                // TODO  leave the psuedo in comment, TODO send this stuff
                // for path in path_manager.pending_paths() {
                // queue.push(path transmission context)
                // need shared_state, look at application_transmission for examples
                //  prob_path(path) // for mtu discovery or path
                //  if not validated, send challenge_frame;
                //  }
                //  TODO send probe for MTU changes
            }
            ConnectionState::Closing => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
                //= type=TODO
                //= tracking-issue=194
                //= feature=Connection Close
                //# An endpoint that wishes to communicate a fatal
                //# connection error MUST use a CONNECTION_CLOSE frame if it is able.
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
        connection_id_mapper: &mut ConnectionIdMapper,
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

        if self
            .timers
            .initial_id_expiration_timer
            .poll_expiration(timestamp)
            .is_ready()
        {
            connection_id_mapper.remove_initial_id(&self.internal_connection_id);
        }

        self.local_id_registry.on_timeout(timestamp);

        shared_state.space_manager.on_timeout(
            &mut self.local_id_registry,
            &mut self.path_manager,
            timestamp,
        );
    }

    /// Updates the per-connection timer based on individual component timers.
    /// This method is used in order to update the connection timer only once
    /// per interaction with the connection and thereby to batch timer updates.
    fn update_connection_timer(&mut self, shared_state: &mut SharedConnectionState<Self::Config>) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //# When ack-eliciting packets in multiple packet number spaces are in
        //# flight, the timer MUST be set to the earlier value of the Initial and
        //# Handshake packet number spaces.

        // find the earliest armed timer
        let earliest = core::iter::empty()
            .chain(self.timers.iter())
            .chain(shared_state.space_manager.timers())
            .chain(self.local_id_registry.timers())
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
            if let Some((application, _handshake_status)) =
                shared_state.space_manager.application_mut()
            {
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
    fn on_datagram_received<
        CC: congestion_controller::Endpoint<CongestionController = Config::CongestionController>,
    >(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        congestion_controller_endpoint: &mut CC,
    ) -> Result<path::Id, TransportError> {
        let is_handshake_confirmed = shared_state.space_manager.is_handshake_confirmed();

        let (id, unblocked) = self.path_manager.on_datagram_received(
            datagram,
            &self.limits,
            is_handshake_confirmed,
            || {
                let path_info = congestion_controller::PathInfo::new(&datagram.remote_address);
                // TODO set alpn if available
                congestion_controller_endpoint.new_congestion_controller(path_info)
            },
        )?;

        if unblocked {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.6
            //# When a server is blocked by anti-amplification limits, receiving a
            //# datagram unblocks it, even if none of the packets in the datagram are
            //# successfully processed.  In such a case, the PTO timer will need to
            //# be re-armed.
            shared_state
                .space_manager
                .on_amplification_unblocked(&self.path_manager[id], datagram.timestamp);
        }

        Ok(id)
    }

    /// Is called when a initial packet had been received
    fn handle_initial_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedInitial,
    ) -> Result<(), ProcessingError> {
        if let Some((packet, _space, _handshake_status)) =
            packet_validator!(self, packet, shared_state.space_manager.initial_mut())
        {
            self.handle_cleartext_initial_packet(shared_state, datagram, path_id, packet)?;

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.1
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
        path_id: path::Id,
        packet: CleartextInitial,
    ) -> Result<(), TransportError> {
        if let Some((space, handshake_status)) = shared_state.space_manager.initial_mut() {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2
            //= type=TODO
            //= tracking-issue=336
            //# Invalid packets that lack strong integrity protection, such as
            //# Initial, Retry, or Version Negotiation, MAY be discarded.
            // Attempt to validate some of the enclosed frames?

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.2
            //= type=TODO
            //= tracking-issue=385
            //# This token MUST be repeated by the client in all
            //# Initial packets it sends for that connection after it receives the
            //# Retry packet.
            // This can be checked on the server side by setting a value in the connection if a
            // token is received in the first Initial Packet. If that value is set, it should be
            // verified in all subsequent packets.

            if let Some(close) = space.handle_cleartext_payload(
                packet.packet_number,
                packet.payload,
                datagram,
                path_id,
                &mut self.path_manager,
                handshake_status,
                &mut self.local_id_registry,
            )? {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2
                //# An
                //# endpoint MUST generate a connection error if processing the contents
                //# of these packets prior to discovering an error, unless it fully
                //# reverts these changes.

                self.close(
                    shared_state,
                    ConnectionCloseReason::PeerImmediateClose(close),
                    datagram.timestamp,
                );
                return Ok(());
            }

            // try to move the crypto state machine forward
            self.update_crypto_state(shared_state, datagram)?;
        }

        Ok(())
    }

    /// Is called when a handshake packet had been received
    fn handle_handshake_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedHandshake,
    ) -> Result<(), ProcessingError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.1
        //= type=TODO
        //= tracking-issue=337
        //# The client MAY drop these packets, or MAY buffer them in anticipation
        //# of later packets that allow it to compute the key.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.2
        //= type=TODO
        //= tracking-issue=340
        //# Clients are not able to send Handshake packets prior to
        //# receiving a server response, so servers SHOULD ignore any such
        //# packets.

        if let Some((packet, space, handshake_status)) =
            packet_validator!(self, packet, shared_state.space_manager.handshake_mut())
        {
            if let Some(close) = space.handle_cleartext_payload(
                packet.packet_number,
                packet.payload,
                datagram,
                path_id,
                &mut self.path_manager,
                handshake_status,
                &mut self.local_id_registry,
            )? {
                self.close(
                    shared_state,
                    ConnectionCloseReason::PeerImmediateClose(close),
                    datagram.timestamp,
                );
                return Ok(());
            }

            if Self::Config::ENDPOINT_TYPE.is_server() {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.1
                //# a server MUST discard Initial keys when it first
                //# successfully processes a Handshake packet.
                shared_state
                    .space_manager
                    .discard_initial(self.path_manager.active_path_mut());

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
                //# Once the server has successfully processed a
                //# Handshake packet from the client, it can consider the client address
                //# to have been validated.
                self.path_manager[path_id].on_validated();
            }

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.1
            //# An endpoint restarts its idle timer when a packet from its peer is
            //# received and processed successfully.
            self.restart_peer_idle_timer(datagram.timestamp);

            // try to move the crypto state machine forward
            self.update_crypto_state(shared_state, datagram)?;
        }

        Ok(())
    }

    /// Is called when a short packet had been received
    fn handle_short_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedShort,
    ) -> Result<(), ProcessingError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.7
        //# Endpoints in either role MUST NOT decrypt 1-RTT packets from
        //# their peer prior to completing the handshake.

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.7
        //# A server MUST NOT process
        //# incoming 1-RTT protected packets before the TLS handshake is
        //# complete.

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.7
        //# Even if it has 1-RTT secrets, a client MUST NOT
        //# process incoming 1-RTT protected packets before the TLS handshake is
        //# complete.

        if !shared_state.space_manager.is_handshake_complete() {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.7
            //= type=TODO
            //= tracking-issue=320
            //# Received
            //# packets protected with 1-RTT keys MAY be stored and later decrypted
            //# and used once the handshake is complete.

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.7
            //= type=TODO
            //= tracking-issue=320
            //= feature=0-RTT
            //# The server MAY retain these packets for
            //# later decryption in anticipation of receiving a ClientHello.

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.1
            //= type=TODO
            //# The client MAY drop these packets, or MAY buffer them in anticipation
            //# of later packets that allow it to compute the key.

            return Ok(());
        }

        if let Some((packet, space, handshake_status)) = packet_validator!(
            self,
            packet,
            shared_state.space_manager.application_mut(),
            {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.2
                //= type=TODO
                //= tracking-issue=478
                //= feature=Key update
                //# The endpoint MUST update its
                //# send keys to the corresponding key phase in response, as described in
                //# Section 6.1.

                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.2
                //= type=TODO
                //= tracking-issue=478
                //= feature=Key update
                //# Sending keys MUST be updated before sending an
                //# acknowledgement for the packet that was received with updated keys.

                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.2
                //= type=TODO
                //= tracking-issue=479
                //= feature=Key update
                //# An endpoint
                //# MAY treat consecutive key updates as a connection error of type
                //# KEY_UPDATE_ERROR.

                if packet.key_phase != Default::default() {
                    dbg!("key updates are not currently implemented");
                    return Err(CryptoError::INTERNAL_ERROR.into());
                }
            }
        ) {
            if let Some(close) = space.handle_cleartext_payload(
                packet.packet_number,
                packet.payload,
                datagram,
                path_id,
                &mut self.path_manager,
                handshake_status,
                &mut self.local_id_registry,
            )? {
                self.close(
                    shared_state,
                    ConnectionCloseReason::PeerImmediateClose(close),
                    datagram.timestamp,
                );
                return Ok(());
            }

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.1
            //# An endpoint restarts its idle timer when a packet from its peer is
            //# received and processed successfully.
            self.restart_peer_idle_timer(datagram.timestamp);

        // Currently, the application space does not have any crypto state.
        // If, at some point, we decide to add it, we need to call `update_crypto_state` here.
        // (note this comment is indented incorrectly by rustfmt. It applies above, not below. How
        // to fix?)
        } else {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
            //= type=TODO
            //= tracking-issue=448
            //= feature=AEAD Limits
            //# If the total number of received packets that fail
            //# authentication within the connection, across all keys, exceeds the
            //# integrity limit for the selected AEAD, the endpoint MUST immediately
            //# close the connection with a connection error of type
            //# AEAD_LIMIT_REACHED and not process any more packets.

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
            //= type=TODO
            //= tracking-issue=451
            //= feature=AEAD Limits
            //# If a key update is not possible or
            //# integrity limits are reached, the endpoint MUST stop using the
            //# connection and only send stateless resets in response to receiving
            //# packets.
        }

        Ok(())
    }

    /// Is called when a version negotiation packet had been received
    fn handle_version_negotiation_packet(
        &mut self,
        _shared_state: &mut SharedConnectionState<Self::Config>,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedVersionNegotiation,
    ) -> Result<(), ProcessingError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.2
        //= type=TODO
        //= feature=Version negotiation handler
        //= tracking-issue=349
        //# A client that supports only this version of QUIC MUST abandon the
        //# current connection attempt if it receives a Version Negotiation
        //# packet, with the following two exceptions.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.2
        //= type=TODO
        //= feature=Version negotiation handler
        //= tracking-issue=349
        //# A client MUST discard any
        //# Version Negotiation packet if it has received and successfully
        //# processed any other packet, including an earlier Version Negotiation
        //# packet.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.2
        //= type=TODO
        //= feature=Version negotiation handler
        //= tracking-issue=349
        //# A client MUST discard a Version Negotiation packet that
        //# lists the QUIC version selected by the client.

        Ok(())
    }

    /// Is called when a zero rtt packet had been received
    fn handle_zero_rtt_packet(
        &mut self,
        _shared_state: &mut SharedConnectionState<Self::Config>,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedZeroRTT,
    ) -> Result<(), ProcessingError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.2
        //= type=TODO
        //= tracking-issue=339
        //# If the packet is a 0-RTT packet, the server MAY buffer a limited
        //# number of these packets in anticipation of a late-arriving Initial
        //# packet.

        // TODO
        Ok(())
    }

    /// Is called when a retry packet had been received
    fn handle_retry_packet(
        &mut self,
        _shared_state: &mut SharedConnectionState<Self::Config>,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedRetry,
    ) -> Result<(), ProcessingError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
        //= type=TODO
        //= tracking-issue=386
        //= feature=Client Retry
        //# The client MUST NOT use
        //# the token provided in a Retry for future connections.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
        //= type=TODO
        //= tracking-issue=386
        //= feature=Client Retry
        //# In comparison, a
        //# token obtained in a Retry packet MUST be used immediately during the
        //# connection attempt and cannot be used in subsequent connection
        //# attempts.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
        //= type=TODO
        //= tracking-issue=393
        //= feature=Client Retry
        //# The client
        //# MUST include the token in all Initial packets it sends, unless a
        //# Retry replaces the token with a newer one.

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
                use transmission::{interest::Provider as _, Interest};

                let mut transmission = Interest::default();

                transmission += self.path_manager.transmission_interest();

                let constraint = self.path_manager.active_path().transmission_constraint();

                // don't iterate over everything if we can't send anyway
                if !constraint.is_amplification_limited() {
                    transmission += shared_state.space_manager.transmission_interest();
                    transmission += self.local_id_registry.transmission_interest();
                }

                interests.transmission = transmission.can_transmit(constraint);
                interests.new_connection_id = self.local_id_registry.connection_id_interest()
                    != connection::id::Interest::None;
            }
            ConnectionState::Closing => {
                // TODO: Ask the Close Sender whether it needs to transmit
            }
            ConnectionState::Draining => {
                use connection::finalization::Provider as _;

                // This is a pure wait state. We do not want to transmit data here
                interests.finalization =
                    shared_state.space_manager.finalization_status().is_final();
            }
            ConnectionState::Finished => {
                // Remove the connection from the endpoint
                interests.finalization = true;
            }
        }

        interests
    }
}
