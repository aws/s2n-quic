// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contains the implementation of the `Connection`

use crate::{
    connection::{
        self,
        close_sender::CloseSender,
        id::{ConnectionInfo, Interest},
        limits::Limits,
        local_id_registry::LocalIdRegistrationError,
        ConnectionIdMapper, ConnectionInterests, ConnectionTimers, ConnectionTransmission,
        ConnectionTransmissionContext, InternalConnectionId, Parameters as ConnectionParameters,
        ProcessingError,
    },
    contexts::{ConnectionApiCallContext, ConnectionOnTransmitError},
    endpoint, path,
    recovery::RttEstimator,
    space::{PacketSpace, PacketSpaceManager},
    stream, transmission,
    transmission::interest::Provider,
    wakeup_queue::WakeupHandle,
};
use bytes::Bytes;
use core::{
    fmt,
    task::{Context, Poll},
    time::Duration,
};
use s2n_quic_core::{
    application,
    event::{self, ConnectionPublisher as _, IntoEvent as _},
    inet::{DatagramInfo, SocketAddress},
    io::tx,
    packet::{
        handshake::ProtectedHandshake,
        initial::{CleartextInitial, ProtectedInitial},
        number::{PacketNumber, PacketNumberSpace},
        retry::ProtectedRetry,
        short::ProtectedShort,
        version_negotiation::ProtectedVersionNegotiation,
        zero_rtt::ProtectedZeroRtt,
    },
    path::{Handle as _, MaxMtu},
    random, stateless_reset,
    time::{timer, Timestamp},
    transport,
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

impl From<connection::Error> for ConnectionState {
    fn from(error: connection::Error) -> Self {
        match error {
            connection::Error::IdleTimerExpired => {
                // If the idle timer expired we directly move into the final state
                ConnectionState::Finished
            }
            connection::Error::NoValidPath => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
                //# When an endpoint has no validated path on which to send packets, it
                //# MAY discard connection state.

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
                //# If an endpoint has no state about the last validated peer address, it
                //# MUST close the connection silently by discarding all connection
                //# state.
                ConnectionState::Finished
            }
            connection::Error::Closed { initiator }
            | connection::Error::Transport { initiator, .. }
            | connection::Error::Application { initiator, .. }
                if initiator.is_local() =>
            {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.1
                //# An endpoint enters the closing state after initiating an immediate
                //# close.
                ConnectionState::Closing
            }
            connection::Error::Closed { .. }
            | connection::Error::Transport { .. }
            | connection::Error::Application { .. } => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.2.2
                //# The draining state is entered once an endpoint receives a
                //# CONNECTION_CLOSE frame, which indicates that its peer is closing or
                //# draining.
                ConnectionState::Draining
            }
            connection::Error::StatelessReset => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
                //# If the last 16 bytes of the datagram are identical in value to a
                //# Stateless Reset Token, the endpoint MUST enter the draining period
                //# and not send any further packets on this connection.
                ConnectionState::Draining
            }
            _ => {
                // catch all
                ConnectionState::Closing
            }
        }
    }
}

#[derive(Debug)]
pub struct ConnectionImpl<Config: endpoint::Config> {
    /// The local ID registry which should be utilized by the connection
    local_id_registry: connection::LocalIdRegistry,
    /// The timers which are used within the connection
    timers: ConnectionTimers,
    /// Describes whether the connection is known to be accepted by the application
    accept_state: AcceptState,
    /// The current state of the connection
    state: ConnectionState,
    /// Manage the paths that the connection could use
    path_manager: path::Manager<Config>,
    /// The limits applied to the current connection
    limits: Limits,
    /// The error set on the connection
    ///
    /// This is stored so future calls from the application return the same error
    error: Result<(), connection::Error>,
    /// Sends CONNECTION_CLOSE close frames after the connection is closed
    close_sender: CloseSender,
    /// Manages all of the different packet spaces and their respective components
    space_manager: PacketSpaceManager<Config>,
    /// Holds the handle for waking up the endpoint from a application call
    wakeup_handle: WakeupHandle<InternalConnectionId>,
    event_context: EventContext<Config>,
}

struct EventContext<Config: endpoint::Config> {
    /// The [`Connection`]s internal identifier
    internal_connection_id: InternalConnectionId,

    /// The QUIC protocol version which is used for this particular connection
    quic_version: u32,

    /// Holds the event context associated with the connection
    context: <Config::EventSubscriber as event::Subscriber>::ConnectionContext,
}

impl<Config: endpoint::Config> fmt::Debug for EventContext<Config> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("EventContext")
            .field("internal_connection_id", &self.internal_connection_id)
            .field("quic_version", &self.quic_version)
            .finish()
    }
}

impl<Config: endpoint::Config> EventContext<Config> {
    #[inline]
    fn publisher<'a>(
        &'a mut self,
        timestamp: Timestamp,
        subscriber: &'a mut Config::EventSubscriber,
    ) -> event::ConnectionPublisherSubscriber<'a, Config::EventSubscriber> {
        event::ConnectionPublisherSubscriber::new(
            event::builder::ConnectionMeta {
                endpoint_type: Config::ENDPOINT_TYPE,
                id: self.internal_connection_id.into(),
                timestamp,
            },
            self.quic_version,
            subscriber,
            &mut self.context,
        )
    }
}

/// Safety: we use some `Rc<RefCell<T>>` inside of the connection but these values
/// never leave the API boundary and will be all sent together
unsafe impl<Config: endpoint::Config> Send for ConnectionImpl<Config> {}

#[cfg(debug_assertions)]
impl<Config: endpoint::Config> Drop for ConnectionImpl<Config> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("\nLast known connection state: \n {:#?}", self);
        }
    }
}

/// Creates a transmission context for the given connection
///
/// This is a macro rather than a function to get around borrowing limitations
macro_rules! transmission_context {
    (
        $self:ident,
        $outcome:expr,
        $path_id:expr,
        $timestamp:expr,
        $transmission_mode:expr,
        $subscriber:expr,
        $(,)?
    ) => {
        ConnectionTransmissionContext {
            quic_version: $self.event_context.quic_version,
            timestamp: $timestamp,
            path_id: $path_id,
            path_manager: &mut $self.path_manager,
            local_id_registry: &mut $self.local_id_registry,
            outcome: $outcome,
            ecn: Default::default(),
            min_packet_len: None,
            transmission_mode: $transmission_mode,
            publisher: &mut $self.event_context.publisher($timestamp, $subscriber),
        }
    };
}

impl<Config: endpoint::Config> ConnectionImpl<Config> {
    fn update_crypto_state(
        &mut self,
        datagram: &DatagramInfo,
        subscriber: &mut Config::EventSubscriber,
    ) -> Result<(), connection::Error> {
        let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);
        let space_manager = &mut self.space_manager;
        space_manager.poll_crypto(
            self.path_manager.active_path(),
            &mut self.local_id_registry,
            &mut self.limits,
            datagram.timestamp,
            &mut publisher,
        )?;

        if matches!(self.state, ConnectionState::Handshaking)
            && space_manager.is_handshake_confirmed()
        {
            // Move into the HandshakeCompleted state. This will signal the
            // necessary interest to hand over the connection to the application.
            self.accept_state = AcceptState::HandshakeCompleted;
            // Move the connection into the active state.
            self.state = ConnectionState::Active;

            // We don't expect any further initial packets on this connection, so start
            // a timer to remove the mapping from the initial ID to the internal connection ID
            // to give time for any delayed initial packets to arrive.
            if Config::ENDPOINT_TYPE.is_server() {
                self.timers
                    .initial_id_expiration_timer
                    .set(datagram.timestamp + 3 * self.current_pto())
            }
        }

        Ok(())
    }

    /// Returns the idle timeout based on transport parameters of both peers
    fn get_idle_timer_duration(&self) -> Option<Duration> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.1
        //# Each endpoint advertises a max_idle_timeout, but the effective value
        //# at an endpoint is computed as the minimum of the two advertised
        //# values.  By announcing a max_idle_timeout, an endpoint commits to
        //# initiating an immediate close (Section 10.2) if it abandons the
        //# connection prior to the effective value.
        let mut duration = self.limits.max_idle_timeout()?;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.1
        //# To avoid excessively small idle timeout periods, endpoints MUST
        //# increase the idle timeout period to be at least three times the
        //# current Probe Timeout (PTO).  This allows for multiple PTOs to
        //# expire, and therefore multiple probes to be sent and lost, prior to
        //# idle timeout.
        duration = duration.max(3 * self.current_pto());

        Some(duration)
    }

    fn on_processed_packet(&mut self, timestamp: Timestamp) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.1
        //# An endpoint restarts its idle timer when a packet from its peer is
        //# received and processed successfully.
        if let Some(duration) = self.get_idle_timer_duration() {
            self.timers.peer_idle_timer.set(timestamp + duration);
            self.timers.reset_peer_idle_timer_on_send = true;
        }
    }

    fn on_ack_eliciting_packet_sent(&mut self, timestamp: Timestamp) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.1
        //# An endpoint also restarts its
        //# idle timer when sending an ack-eliciting packet if no other ack-
        //# eliciting packets have been sent since last receiving and processing
        //# a packet.

        // reset the value back to `false` after reading it
        if core::mem::take(&mut self.timers.reset_peer_idle_timer_on_send) {
            if let Some(duration) = self.get_idle_timer_duration() {
                self.timers.peer_idle_timer.set(timestamp + duration);
            }
        }
    }

    fn current_pto(&self) -> Duration {
        self.path_manager.active_path().pto_period({
            // Incorporate `max_ack_delay` into the timeout
            PacketNumberSpace::ApplicationData
        })
    }

    /// Send path validation frames for the non-active path.
    ///
    /// Since non-probing frames can only be sent on the active path, a separate
    /// transmission context with Mode::PathValidationOnly is used to send on
    /// other paths.
    fn path_validation_only_transmission<'a, 'sub, Tx: tx::Queue<Handle = Config::PathHandle>>(
        &mut self,
        queue: &mut Tx,
        timestamp: Timestamp,
        outcome: &'a mut transmission::Outcome,
        subscriber: &'sub mut Config::EventSubscriber,
    ) -> usize {
        let mut count = 0;
        let mut pending_paths = self.path_manager.paths_pending_validation();
        let ecn = Default::default();
        while let Some((path_id, path_manager)) = pending_paths.next_path() {
            // It is more efficient to coalesce path validation and other
            // frames for the active path so we skip PathValidationOnly
            // and handle transmission for the active path seperately.
            if path_id == path_manager.active_path_id() {
                continue;
            }

            if !path_manager[path_id].at_amplification_limit()
                && queue
                    .push(ConnectionTransmission {
                        context: ConnectionTransmissionContext {
                            quic_version: self.event_context.quic_version,
                            timestamp,
                            path_id,
                            path_manager,
                            local_id_registry: &mut self.local_id_registry,
                            outcome,
                            ecn,
                            min_packet_len: None,
                            transmission_mode: transmission::Mode::PathValidationOnly,
                            publisher: &mut self.event_context.publisher(timestamp, subscriber),
                        },
                        space_manager: &mut self.space_manager,
                    })
                    .is_ok()
            {
                count += 1;
            }
        }

        count
    }
}

impl<Config: endpoint::Config> connection::Trait for ConnectionImpl<Config> {
    /// Static configuration of a connection
    type Config = Config;

    fn is_handshaking(&self) -> bool {
        self.accept_state == AcceptState::Handshaking
    }

    /// Creates a new `Connection` instance with the given configuration
    fn new(parameters: ConnectionParameters<Self::Config>) -> Self {
        let mut event_context = EventContext {
            context: parameters.event_context,
            internal_connection_id: parameters.internal_connection_id,
            quic_version: parameters.quic_version,
        };

        // The path manager always starts with a single path containing the known peer and local
        // connection ids.
        let rtt_estimator = RttEstimator::new(parameters.limits.ack_settings().max_ack_delay);
        // Assume clients validate the server's address implicitly.
        let peer_validated = Self::Config::ENDPOINT_TYPE.is_server();
        let initial_path = path::Path::new(
            parameters.path_handle,
            parameters.peer_connection_id,
            parameters.local_connection_id,
            rtt_estimator,
            parameters.congestion_controller,
            peer_validated,
            parameters.max_mtu,
        );

        let path_manager = path::Manager::new(initial_path, parameters.peer_id_registry);

        let mut publisher =
            event_context.publisher(parameters.timestamp, parameters.event_subscriber);

        publisher.on_connection_started(event::builder::ConnectionStarted {
            path: event::builder::Path {
                local_addr: parameters.path_handle.local_address().into_event(),
                local_cid: parameters.local_connection_id.into_event(),
                remote_addr: parameters.path_handle.remote_address().into_event(),
                remote_cid: parameters.peer_connection_id.into_event(),
                id: path_manager.active_path_id().into_event(),
            },
        });

        Self {
            local_id_registry: parameters.local_id_registry,
            timers: Default::default(),
            accept_state: AcceptState::Handshaking,
            state: ConnectionState::Handshaking,
            path_manager,
            limits: parameters.limits,
            error: Ok(()),
            close_sender: CloseSender::default(),
            space_manager: parameters.space_manager,
            wakeup_handle: parameters.wakeup_handle,
            event_context,
        }
    }

    /// Returns the Connections internal ID
    fn internal_connection_id(&self) -> InternalConnectionId {
        self.event_context.internal_connection_id
    }

    /// Returns the QUIC version selected for the current connection
    fn quic_version(&self) -> u32 {
        self.event_context.quic_version
    }

    /// Initiates closing the connection as described in
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10
    fn close(
        &mut self,
        error: connection::Error,
        close_formatter: &Config::ConnectionCloseFormatter,
        packet_buffer: &mut endpoint::PacketBuffer,
        timestamp: Timestamp,
        subscriber: &mut Config::EventSubscriber,
    ) {
        match self.state {
            ConnectionState::Closing | ConnectionState::Draining | ConnectionState::Finished => {
                // The connection is already closing
                return;
            }
            ConnectionState::Handshaking | ConnectionState::Active => {}
        }

        let mut publisher = self.event_context.publisher(timestamp, subscriber);

        publisher.on_connection_closed(event::builder::ConnectionClosed { error });

        // We don't need any timers anymore
        self.timers.cancel();
        // Let the path manager know we're closing
        self.path_manager.on_closing();
        // Update the connection state based on the type of error
        self.state = error.into();
        self.error = Err(error);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.3
        //# An endpoint that wishes to communicate a fatal
        //# connection error MUST use a CONNECTION_CLOSE frame if it is able.

        let remote_address = self.path_manager.active_path().remote_address();
        let close_context = s2n_quic_core::connection::close::Context::new(&remote_address);
        let active_path_id = self.path_manager.active_path_id();

        if let Some((early_connection_close, connection_close)) =
            s2n_quic_core::connection::error::as_frame(error, close_formatter, &close_context)
        {
            let mut outcome = transmission::Outcome::new(PacketNumber::default());
            let mut context = transmission_context!(
                self,
                &mut outcome,
                active_path_id,
                timestamp,
                transmission::Mode::Normal,
                subscriber,
            );

            if let Some(packet) = self.space_manager.on_transmit_close(
                &early_connection_close,
                &connection_close,
                &mut context,
                packet_buffer,
            ) {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2
                //# The closing and draining connection states exist to ensure that
                //# connections close cleanly and that delayed or reordered packets are
                //# properly discarded.  These states SHOULD persist for at least three
                //# times the current Probe Timeout (PTO) interval as defined in
                //# [QUIC-RECOVERY].
                let timeout = 3 * self.current_pto();

                self.close_sender.close(packet, timeout, timestamp);
            } else if cfg!(debug_assertions) {
                panic!("missing packet spaces before sending connection close frame");
            } else {
                // if we couldn't send anything, just discard the connection
                self.state = ConnectionState::Finished;
            }
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.1
        //# In the closing state, an endpoint retains only enough information to
        //# generate a packet containing a CONNECTION_CLOSE frame and to identify
        //# packets as belonging to the connection.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.1
        //# An endpoint's selected connection ID and the QUIC version are
        //# sufficient information to identify packets for a closing connection;
        //# the endpoint MAY discard all other connection state.
        let mut publisher = self.event_context.publisher(timestamp, subscriber);
        self.space_manager.discard_initial(
            self.path_manager.active_path_mut(),
            active_path_id,
            &mut publisher,
        );
        self.space_manager.discard_handshake(
            self.path_manager.active_path_mut(),
            active_path_id,
            &mut publisher,
        );
        self.space_manager.discard_zero_rtt_crypto();

        // We don't discard the application space so the application can
        // be notified and read what happened.
        if let Some((application, _handshake_status)) = self.space_manager.application_mut() {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2
            //# A CONNECTION_CLOSE frame
            //# causes all streams to immediately become closed; open streams can be
            //# assumed to be implicitly reset.

            // Close all streams with the derived error
            application.stream_manager.close(error);
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
                let remote_address = self.path_manager.active_path().remote_address();
                let connection_info = ConnectionInfo::new(&remote_address);

                while count > 0 {
                    let id = connection_id_format.generate(&connection_info);
                    let expiration = connection_id_format
                        .lifetime()
                        .map(|duration| timestamp + duration);
                    let stateless_reset_token =
                        stateless_reset_token_generator.generate(id.as_bytes());
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
    fn on_transmit<Tx: tx::Queue<Handle = Config::PathHandle>>(
        &mut self,
        queue: &mut Tx,
        timestamp: Timestamp,
        subscriber: &mut Config::EventSubscriber,
    ) -> Result<(), ConnectionOnTransmitError> {
        let mut count = 0;

        debug_assert!(
            !self.path_manager.is_amplification_limited(),
            "connection should not express transmission interest if amplification limited"
        );

        match self.state {
            ConnectionState::Handshaking | ConnectionState::Active => {
                let mut outcome = transmission::Outcome::new(PacketNumber::default());

                while !self.path_manager.active_path().at_amplification_limit()
                    && queue
                        .push(ConnectionTransmission {
                            context: transmission_context!(
                                self,
                                &mut outcome,
                                self.path_manager.active_path_id(),
                                timestamp,
                                transmission::Mode::Normal,
                                subscriber,
                            ),
                            space_manager: &mut self.space_manager,
                        })
                        .is_ok()
                {
                    count += 1;
                }

                if outcome.ack_elicitation.is_ack_eliciting() {
                    self.on_ack_eliciting_packet_sent(timestamp);
                }

                // Send an MTU probe if necessary
                if self
                    .path_manager
                    .active_path()
                    .mtu_controller
                    .can_transmit(self.path_manager.active_path().transmission_constraint())
                    && queue
                        .push(ConnectionTransmission {
                            context: transmission_context!(
                                self,
                                &mut outcome,
                                self.path_manager.active_path_id(),
                                timestamp,
                                transmission::Mode::MtuProbing,
                                subscriber,
                            ),
                            space_manager: &mut self.space_manager,
                        })
                        .is_ok()
                {
                    count += 1;
                }

                // PathValidationOnly handles transmission on non-active paths. Transmission
                // on the active path should be handled prior to this.
                count += self.path_validation_only_transmission(
                    queue,
                    timestamp,
                    &mut outcome,
                    subscriber,
                );
            }
            ConnectionState::Closing => {
                let mut publisher = self.event_context.publisher(timestamp, subscriber);
                let path = self.path_manager.active_path_mut();

                if queue
                    .push(self.close_sender.transmission(path, &mut publisher))
                    .is_ok()
                {
                    count += 1;
                }
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
        connection_id_mapper: &mut ConnectionIdMapper,
        timestamp: Timestamp,
        subscriber: &mut Config::EventSubscriber,
    ) -> Result<(), connection::Error> {
        if self.close_sender.on_timeout(timestamp).is_ready() {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2
            //# Once its closing or draining state ends, an endpoint SHOULD discard
            //# all connection state.
            self.state = ConnectionState::Finished;
        }

        if self
            .timers
            .initial_id_expiration_timer
            .poll_expiration(timestamp)
            .is_ready()
        {
            connection_id_mapper.remove_initial_id(&self.event_context.internal_connection_id);
        }

        self.path_manager.on_timeout(timestamp)?;
        self.local_id_registry.on_timeout(timestamp);

        let mut publisher = self.event_context.publisher(timestamp, subscriber);
        self.space_manager.on_timeout(
            &mut self.local_id_registry,
            &mut self.path_manager,
            timestamp,
            &mut publisher,
        );

        if self
            .timers
            .peer_idle_timer
            .poll_expiration(timestamp)
            .is_ready()
        {
            return Err(connection::Error::IdleTimerExpired);
        }

        // TODO: enable this check once all of the component timers are fixed
        /*
        if cfg!(debug_assertions) {
            use timer::Provider;

            // make sure that all of the components have been updated and no longer expire
            // with the current timestamp

            (&self, &shared_state).for_each_timer(|timer| {
                assert!(
                    !timer.is_expired(timestamp),
                    "timer has not been reset on timeout; now: {}, timer: {:?}",
                    timestamp,
                    timer,
                );
                Ok(())
            });
        }
        */

        Ok(())
    }

    /// Handles all external wakeups on the [`Connection`].
    fn on_wakeup(&mut self, _timestamp: Timestamp) -> Result<(), connection::Error> {
        self.wakeup_handle.wakeup_handled();

        // return an error if the application set one
        self.error?;

        Ok(())
    }

    // Packet handling
    fn on_datagram_received(
        &mut self,
        path_handle: &Config::PathHandle,
        datagram: &DatagramInfo,
        congestion_controller_endpoint: &mut Config::CongestionControllerEndpoint,
        random: &mut Config::RandomGenerator,
        max_mtu: MaxMtu,
        subscriber: &mut Config::EventSubscriber,
    ) -> Result<path::Id, connection::Error> {
        let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //# The design of QUIC relies on endpoints retaining a stable address
        //# for the duration of the handshake.  An endpoint MUST NOT initiate
        //# connection migration before the handshake is confirmed, as defined
        //# in section 4.1.2 of [QUIC-TLS].

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.1
        //# An endpoint in the closing state MUST either discard
        //# packets received from an unvalidated address or limit the cumulative
        //# size of packets it sends to an unvalidated address to three times the
        //# size of packets it receives from that address.
        let handshake_confirmed = self.space_manager.is_handshake_confirmed();

        let (id, unblocked) = self.path_manager.on_datagram_received(
            path_handle,
            datagram,
            &self.limits,
            handshake_confirmed,
            congestion_controller_endpoint,
            random,
            max_mtu,
            &mut publisher,
        )?;

        publisher.on_datagram_received(event::builder::DatagramReceived {
            len: datagram.payload_len as u16,
        });

        if matches!(self.state, ConnectionState::Closing) {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.1
            //# An endpoint in the closing
            //# state sends a packet containing a CONNECTION_CLOSE frame in response
            //# to any incoming packet that it attributes to the connection.

            // The connection is in the closing state so notify the close sender
            // that it may need to retransmit the close frame
            if id == self.path_manager.active_path_id() {
                let rtt = self.path_manager[id].rtt_estimator.latest_rtt();
                self.close_sender
                    .on_datagram_received(rtt, datagram.timestamp);
            }
        } else if unblocked {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.6
            //# When a server is blocked by anti-amplification limits, receiving a
            //# datagram unblocks it, even if none of the packets in the datagram are
            //# successfully processed.  In such a case, the PTO timer will need to
            //# be re-armed.
            self.space_manager
                .on_amplification_unblocked(&self.path_manager[id], datagram.timestamp);
        }

        Ok(id)
    }

    /// Is called when a initial packet had been received
    fn handle_initial_packet<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedInitial,
        random_generator: &mut Rnd,
        subscriber: &mut Config::EventSubscriber,
    ) -> Result<(), ProcessingError> {
        if let Some((space, _status)) = self.space_manager.initial_mut() {
            let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);

            let packet = space.validate_and_decrypt_packet(packet, path_id, &mut publisher)?;

            publisher.on_packet_received(event::builder::PacketReceived {
                packet_header: event::builder::PacketHeader {
                    packet_type: packet.packet_number.into_event(),
                    version: Some(packet.version),
                },
            });

            self.handle_cleartext_initial_packet(
                datagram,
                path_id,
                packet,
                random_generator,
                subscriber,
            )?;
        }

        Ok(())
    }

    /// Is called when an unprotected initial packet had been received
    fn handle_cleartext_initial_packet<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: CleartextInitial,
        random_generator: &mut Rnd,
        subscriber: &mut Config::EventSubscriber,
    ) -> Result<(), ProcessingError> {
        if let Some((space, handshake_status)) = self.space_manager.initial_mut() {
            let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);

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

            space.handle_cleartext_payload(
                packet.packet_number,
                packet.payload,
                datagram,
                path_id,
                &mut self.path_manager,
                handshake_status,
                &mut self.local_id_registry,
                random_generator,
                &mut publisher,
            )?;

            // try to move the crypto state machine forward
            self.update_crypto_state(datagram, subscriber)?;

            // notify the connection a packet was processed
            self.on_processed_packet(datagram.timestamp);
        }

        Ok(())
    }

    /// Is called when a handshake packet had been received
    fn handle_handshake_packet<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedHandshake,
        random_generator: &mut Rnd,
        subscriber: &mut Config::EventSubscriber,
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

        if let Some((space, handshake_status)) = self.space_manager.handshake_mut() {
            let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);

            let packet = space.validate_and_decrypt_packet(packet, path_id, &mut publisher)?;

            publisher.on_packet_received(event::builder::PacketReceived {
                packet_header: event::builder::PacketHeader {
                    packet_type: packet.packet_number.into_event(),
                    version: Some(packet.version),
                },
            });

            space.handle_cleartext_payload(
                packet.packet_number,
                packet.payload,
                datagram,
                path_id,
                &mut self.path_manager,
                handshake_status,
                &mut self.local_id_registry,
                random_generator,
                &mut publisher,
            )?;

            if Self::Config::ENDPOINT_TYPE.is_server() {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.1
                //# a server MUST discard Initial keys when it first
                //# successfully processes a Handshake packet.
                self.space_manager.discard_initial(
                    self.path_manager.active_path_mut(),
                    path_id,
                    &mut publisher,
                );

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
                //# Once the server has successfully processed a
                //# Handshake packet from the client, it can consider the client address
                //# to have been validated.
                self.path_manager[path_id].on_handshake_packet();
            }

            // try to move the crypto state machine forward
            self.update_crypto_state(datagram, subscriber)?;

            // notify the connection a packet was processed
            self.on_processed_packet(datagram.timestamp);
        }

        Ok(())
    }

    /// Is called when a short packet had been received
    fn handle_short_packet<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedShort,
        random_generator: &mut Rnd,
        subscriber: &mut Config::EventSubscriber,
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

        if !self.space_manager.is_handshake_complete() {
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

        if let Some((space, handshake_status)) = self.space_manager.application_mut() {
            let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);

            let packet = space.validate_and_decrypt_packet(
                packet,
                datagram,
                &self.path_manager.active_path().rtt_estimator,
                path_id,
                &mut publisher,
            )?;

            // Connection Ids are issued to the peer after the handshake is
            // confirmed and the handshake space is discarded. Therefore only
            // short packets need to be processed for local_connection_id changes.
            self.path_manager[path_id].on_process_local_connection_id(
                path_id,
                &packet,
                &datagram.destination_connection_id,
                &mut publisher,
            );

            space.handle_cleartext_payload(
                packet.packet_number,
                packet.payload,
                datagram,
                path_id,
                &mut self.path_manager,
                handshake_status,
                &mut self.local_id_registry,
                random_generator,
                &mut publisher,
            )?;

            // Currently, the application space does not have any crypto state.
            // If, at some point, we decide to add it, we need to call `update_crypto_state` here.

            // notify the connection a packet was processed
            self.on_processed_packet(datagram.timestamp);

            let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);
            publisher.on_packet_received(event::builder::PacketReceived {
                packet_header: event::builder::PacketHeader {
                    packet_type: packet.packet_number.into_event(),
                    version: Some(publisher.quic_version()),
                },
            });
        }

        Ok(())
    }

    /// Is called when a version negotiation packet had been received
    fn handle_version_negotiation_packet(
        &mut self,
        datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedVersionNegotiation,
        subscriber: &mut Config::EventSubscriber,
    ) -> Result<(), ProcessingError> {
        let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);

        publisher.on_packet_received(event::builder::PacketReceived {
            packet_header: event::builder::PacketHeader {
                packet_type: event::builder::PacketType::VersionNegotiation {},
                version: Some(publisher.quic_version()),
            },
        });
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
        datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedZeroRtt,
        subscriber: &mut Config::EventSubscriber,
    ) -> Result<(), ProcessingError> {
        let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);

        publisher.on_packet_received(event::builder::PacketReceived {
            packet_header: event::builder::PacketHeader {
                // FIXME: remove when we support zero-rtt. Since there is a
                // `IntoEvent<PacketType>` for PacketNumber` this can be replaced
                // wih `packet_number.into_event()` once the packet number is
                // available.
                packet_type: event::builder::PacketType::ZeroRtt { number: 0 },
                version: Some(publisher.quic_version()),
            },
        });
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
        datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedRetry,
        subscriber: &mut Config::EventSubscriber,
    ) -> Result<(), ProcessingError> {
        let mut publisher = self.event_context.publisher(datagram.timestamp, subscriber);

        publisher.on_packet_received(event::builder::PacketReceived {
            packet_header: event::builder::PacketHeader {
                packet_type: event::builder::PacketType::Retry {},
                version: Some(publisher.quic_version()),
            },
        });
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

    fn mark_as_accepted(&mut self) {
        debug_assert!(
            self.accept_state == AcceptState::HandshakeCompleted,
            "mark_accepted() should only be called on connections which have finished the handshake");
        self.accept_state = AcceptState::Active;
    }

    fn interests(&self) -> ConnectionInterests {
        use crate::connection::finalization::Provider as _;
        use timer::Provider as _;
        use transmission::interest::Provider as _;

        let mut interests = ConnectionInterests {
            timeout: self.next_expiration(),
            ..Default::default()
        };

        if self.accept_state == AcceptState::HandshakeCompleted {
            interests.accept = true;
        }

        match self.state {
            ConnectionState::Active | ConnectionState::Handshaking => {
                let constraint = self.path_manager.transmission_constraint();

                interests.transmission = self.can_transmit(constraint);
                interests.new_connection_id =
                    // Only issue new Connection Ids to the peer when we know they won't be used
                    // for Initial or Handshake packets.
                    // This is important so that Connection Ids can't be linked to the
                    // Application space.
                    self.space_manager.initial().is_none()
                    && self.space_manager.handshake().is_none()
                    && self.local_id_registry.connection_id_interest()
                        != connection::id::Interest::None;
            }
            ConnectionState::Closing => {
                let constraint = self.path_manager.active_path().transmission_constraint();
                interests.closing = true;
                interests.transmission = self.close_sender.can_transmit(constraint);
                interests.finalization = self.close_sender.finalization_status().is_final();
            }
            ConnectionState::Draining | ConnectionState::Finished => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.2
                //# While otherwise identical to the closing state, an
                //# endpoint in the draining state MUST NOT send any packets.
                interests.transmission = false;

                // Remove the connection from the endpoint
                interests.finalization = true;
            }
        }

        interests
    }

    // public API methods

    fn poll_stream_request(
        &mut self,
        stream_id: stream::StreamId,
        request: &mut stream::ops::Request,
        context: Option<&Context>,
    ) -> Result<stream::ops::Response, stream::StreamError> {
        self.error?;

        let (space, _) = self
            .space_manager
            .application_mut()
            .ok_or(connection::Error::Unspecified)?;

        let mut api_context = ConnectionApiCallContext::from_wakeup_handle(&mut self.wakeup_handle);

        space
            .stream_manager
            .poll_request(stream_id, &mut api_context, request, context)
    }

    fn poll_accept_stream(
        &mut self,
        stream_type: Option<stream::StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<stream::StreamId>, connection::Error>> {
        self.error?;

        let (space, _) = self
            .space_manager
            .application_mut()
            .ok_or(connection::Error::Unspecified)?;

        space.stream_manager.poll_accept(stream_type, context)
    }

    fn poll_open_stream(
        &mut self,
        stream_type: stream::StreamType,
        context: &Context,
    ) -> Poll<Result<stream::StreamId, connection::Error>> {
        self.error?;

        let (space, _) = self
            .space_manager
            .application_mut()
            .ok_or(connection::Error::Unspecified)?;

        space.stream_manager.poll_open(stream_type, context)
    }

    fn application_close(&mut self, error: Option<application::Error>) {
        self.error = Err(match error {
            Some(error) => connection::Error::Application {
                error,
                initiator: endpoint::Location::Local,
            },
            None => transport::Error::APPLICATION_ERROR.into(),
        });
    }

    fn sni(&self) -> Option<Bytes> {
        // TODO move SNI to connection
        self.space_manager.application()?.sni.clone()
    }

    fn alpn(&self) -> Bytes {
        // TODO move ALPN to connection
        if let Some(space) = self.space_manager.application() {
            space.alpn.clone()
        } else {
            Bytes::from_static(&[])
        }
    }

    fn ping(&mut self) -> Result<(), connection::Error> {
        self.error?;

        if let Some((space, _)) = self.space_manager.application_mut() {
            space.ping();

            self.wakeup_handle.wakeup();
        } else {
            // applications can't ping until the application space is available
            // TODO: maybe return a better error message?
            return Err(connection::Error::Unspecified);
        }

        Ok(())
    }

    fn local_address(&self) -> Result<SocketAddress, connection::Error> {
        Ok(*self.path_manager.active_path().handle.local_address())
    }

    fn remote_address(&self) -> Result<SocketAddress, connection::Error> {
        Ok(*self.path_manager.active_path().handle.remote_address())
    }
}

impl<Config: endpoint::Config> timer::Provider for ConnectionImpl<Config> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        // find the earliest armed timer
        self.timers.timers(query)?;
        self.close_sender.timers(query)?;
        self.local_id_registry.timers(query)?;
        self.path_manager.timers(query)?;
        self.space_manager.timers(query)?;

        Ok(())
    }
}

impl<Config: endpoint::Config> transmission::interest::Provider for ConnectionImpl<Config> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.path_manager.transmission_interest(query)?;

        self.space_manager.transmission_interest(query)?;

        self.local_id_registry.transmission_interest(query)?;
        self.path_manager
            .active_path()
            .mtu_controller
            .transmission_interest(query)?;

        Ok(())
    }
}
