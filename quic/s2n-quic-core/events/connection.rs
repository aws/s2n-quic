// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("transport:application_protocol_information")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.2
//# QUIC implementations each have their own list of application level
//# protocols and versions thereof they support.
/// Application level protocol
struct ApplicationProtocolInformation<'a> {
    chosen_application_protocol: &'a [u8],
}

#[event("transport:server_name_information")]
/// Server Name was negotiated for the connection
struct ServerNameInformation<'a> {
    chosen_server_name: &'a str,
}

#[event("transport:key_exchange_group")]
/// Key Exchange Group was negotiated for the connection
///
/// `contains_kem` is `true` if the `chosen_group_name`
/// contains a key encapsulation mechanism
struct KeyExchangeGroup<'a> {
    chosen_group_name: &'a str,
    contains_kem: bool,
}

#[event("transport:packet_skipped")]
/// Packet was skipped with a given reason
struct PacketSkipped {
    number: u64,
    space: KeySpace,
    reason: PacketSkipReason,
}

#[event("transport:packet_sent")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.5
/// Packet was sent by a connection
struct PacketSent {
    #[nominal_counter("kind")]
    packet_header: PacketHeader,
    #[measure("bytes", Bytes)]
    #[counter("bytes.total", Bytes)]
    packet_len: usize,
}

#[event("transport:packet_received")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.6
/// Packet was received by a connection
struct PacketReceived {
    #[nominal_counter("kind")]
    packet_header: PacketHeader,
}

#[event("connectivity:active_path_updated")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.1.8
/// Active path was updated
struct ActivePathUpdated<'a> {
    // TODO: many events seem to require PacketHeader. Make it more ergonomic
    // to include this field.
    // packet_header: PacketHeader,
    previous: Path<'a>,
    active: Path<'a>,
}

#[event("transport:path_created")]
/// A new path was created
struct PathCreated<'a> {
    active: Path<'a>,
    new: Path<'a>,
}

#[event("transport:frame_sent")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.5
// This diverges a bit from the qlog spec, which prefers to log data as part of the
// packet events.
/// Frame was sent
struct FrameSent {
    #[nominal_counter("packet")]
    packet_header: PacketHeader,
    path_id: u64,
    #[nominal_counter("frame")]
    frame: Frame,
}

#[event("transport:frame_received")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.6
// This diverges a bit from the qlog spec, which prefers to log data as part of the
// packet events.
/// Frame was received
struct FrameReceived<'a> {
    #[nominal_counter("packet")]
    packet_header: PacketHeader,
    path: Path<'a>,
    #[nominal_counter("frame")]
    frame: Frame,
}

/// A `CONNECTION_CLOSE` frame was received
///
/// This event includes additional details from the frame, particularly the
/// reason (if provided) the peer closed the connection
#[event("transport:connection_close_frame_received")]
struct ConnectionCloseFrameReceived<'a> {
    #[nominal_counter("packet")]
    packet_header: PacketHeader,
    path: Path<'a>,
    frame: ConnectionCloseFrame<'a>,
}

#[event("recovery:packet_lost")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.4.5
/// Packet was lost
struct PacketLost<'a> {
    #[nominal_counter("kind")]
    packet_header: PacketHeader,
    path: Path<'a>,
    #[measure("bytes", Bytes)]
    #[counter("bytes.total", Bytes)]
    bytes_lost: u16,
    #[bool_counter("is_mtu_probe")]
    is_mtu_probe: bool,
}

#[event("recovery:metrics_updated")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.4.2
/// Recovery metrics updated
struct RecoveryMetrics<'a> {
    path: Path<'a>,
    #[measure("min_rtt", Duration)]
    min_rtt: Duration,
    #[measure("smoothed_rtt", Duration)]
    smoothed_rtt: Duration,
    #[measure("latest_rtt", Duration)]
    latest_rtt: Duration,
    #[measure("rtt_variance", Duration)]
    rtt_variance: Duration,
    #[measure("max_ack_delay", Duration)]
    max_ack_delay: Duration,
    #[measure("pto_count")]
    pto_count: u32,
    #[measure("congestion_window", Duration)]
    congestion_window: u32,
    #[measure("bytes_in_flight", Duration)]
    bytes_in_flight: u32,
    #[bool_counter("congestion_limited")]
    congestion_limited: bool,
}

#[event("recovery:congestion")]
/// Congestion (ECN or packet loss) has occurred
struct Congestion<'a> {
    path: Path<'a>,
    #[nominal_counter("source")]
    source: CongestionSource,
}

#[event("recovery:ack_processed")]
#[deprecated(note = "use on_rx_ack_range_dropped event instead")]
/// Events related to ACK processing
struct AckProcessed<'a> {
    #[nominal_counter("action")]
    action: AckAction,
    path: Path<'a>,
}

#[event("recovery:rx_ack_range_dropped")]
/// Ack range for received packets was dropped due to space constraints
///
/// For the purpose of processing Acks, RX packet numbers are stored as
/// packet_number ranges in an IntervalSet; only lower and upper bounds
/// are stored instead of individual packet_numbers. Ranges are merged
/// when possible so only disjointed ranges are stored.
///
/// When at `capacity`, the lowest packet_number range is dropped.
struct RxAckRangeDropped<'a> {
    path: Path<'a>,
    /// The packet number range which was dropped
    packet_number_range: core::ops::RangeInclusive<u64>,
    /// The number of disjoint ranges the IntervalSet can store
    capacity: usize,
    /// The store packet_number range in the IntervalSet
    stored_range: core::ops::RangeInclusive<u64>,
}

#[event("recovery:ack_range_received")]
/// ACK range was received
struct AckRangeReceived<'a> {
    #[nominal_counter("packet")]
    packet_header: PacketHeader,
    path: Path<'a>,
    ack_range: RangeInclusive<u64>,
}

#[event("recovery:ack_range_sent")]
/// ACK range was sent
struct AckRangeSent {
    #[nominal_counter("packet")]
    packet_header: PacketHeader,
    path_id: u64,
    ack_range: RangeInclusive<u64>,
}

#[event("transport:packet_dropped")]
/// Packet was dropped with the given reason
struct PacketDropped<'a> {
    #[nominal_counter("reason")]
    reason: PacketDropReason<'a>,
}

#[event("security:key_update")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.2.1
/// Crypto key updated
struct KeyUpdate {
    #[nominal_counter("key_type")]
    key_type: KeyType,
    #[nominal_counter("cipher_suite")]
    cipher_suite: CipherSuite,
}

#[event("security:key_space_discarded")]
#[checkpoint("initial.latency", |evt| matches!(evt.space, KeySpace::Initial { .. }))]
#[checkpoint("handshake.latency", |evt| matches!(evt.space, KeySpace::Handshake { .. }))]
#[checkpoint("one_rtt.latency", |evt| matches!(evt.space, KeySpace::OneRtt { .. }))]
struct KeySpaceDiscarded {
    #[nominal_counter("space")]
    space: KeySpace,
}

#[event("connectivity:connection_started")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.1.2
/// Connection started
struct ConnectionStarted<'a> {
    path: Path<'a>,
}

#[event("transport:duplicate_packet")]
/// Duplicate packet received
struct DuplicatePacket<'a> {
    #[nominal_counter("kind")]
    packet_header: PacketHeader,
    path: Path<'a>,
    #[nominal_counter("error")]
    error: DuplicatePacketError,
}

#[event("transport:transport_parameters_received")]
/// Transport parameters received by connection
#[checkpoint("latency")]
struct TransportParametersReceived<'a> {
    transport_parameters: TransportParameters<'a>,
}

#[event("transport:datagram_sent")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.10
/// Datagram sent by a connection
struct DatagramSent {
    #[measure("bytes", Bytes)]
    #[counter("bytes.total", Bytes)]
    len: u16,

    /// The GSO offset at which this datagram was written
    ///
    /// If this value is greater than 0, it indicates that this datagram has been sent with other
    /// segments in a single buffer.
    ///
    /// See the [Linux kernel documentation](https://www.kernel.org/doc/html/latest/networking/segmentation-offloads.html#generic-segmentation-offload) for more details.
    #[measure("gso_offset")]
    gso_offset: usize,
}

#[event("transport:datagram_received")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.11
/// Datagram received by a connection
struct DatagramReceived {
    #[measure("bytes", Bytes)]
    #[counter("bytes.total", Bytes)]
    len: u16,
}

#[event("transport:datagram_dropped")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.12
/// Datagram dropped by a connection
struct DatagramDropped<'a> {
    local_addr: SocketAddress<'a>,
    remote_addr: SocketAddress<'a>,
    destination_cid: ConnectionId<'a>,
    source_cid: Option<ConnectionId<'a>>,
    #[measure("bytes", Bytes)]
    #[counter("bytes.total", Bytes)]
    len: u16,
    #[nominal_counter("reason")]
    reason: DatagramDropReason,
}

#[event("transport:handshake_remote_address_change_observed")]
/// The remote address was changed before the handshake was complete
struct HandshakeRemoteAddressChangeObserved<'a> {
    local_addr: SocketAddress<'a>,
    /// The newly observed remote address
    remote_addr: SocketAddress<'a>,
    /// The remote address established from the initial packet
    initial_remote_addr: SocketAddress<'a>,
}

#[event("connectivity:connection_id_updated")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.1.4
/// ConnectionId updated
struct ConnectionIdUpdated<'a> {
    path_id: u64,
    /// The endpoint that updated its connection id
    cid_consumer: crate::endpoint::Location,
    previous: ConnectionId<'a>,
    current: ConnectionId<'a>,
}

#[event("recovery:ecn_state_changed")]
struct EcnStateChanged<'a> {
    path: Path<'a>,
    #[nominal_counter("state")]
    state: EcnState,
}

#[event("connectivity:connection_migration_denied")]
struct ConnectionMigrationDenied {
    #[nominal_counter("reason")]
    reason: MigrationDenyReason,
}

#[event("connectivity:handshake_status_updated")]
#[checkpoint("complete.latency", |evt| matches!(evt.status, HandshakeStatus::Complete { .. }))]
#[checkpoint("confirmed.latency", |evt| matches!(evt.status, HandshakeStatus::Confirmed { .. }))]
#[checkpoint("handshake_done_acked.latency", |evt| matches!(evt.status, HandshakeStatus::HandshakeDoneAcked { .. }))]
struct HandshakeStatusUpdated {
    #[nominal_counter("status")]
    status: HandshakeStatus,
}

#[event("connectivity:tls_exporter_ready")]
struct TlsExporterReady<'a> {
    session: crate::event::TlsSession<'a>,
}

#[event("connectivity:path_challenge_updated")]
/// Path challenge updated
struct PathChallengeUpdated<'a> {
    #[nominal_counter("status")]
    path_challenge_status: PathChallengeStatus,
    path: Path<'a>,
    challenge_data: &'a [u8],
}

#[event("tls:client_hello")]
#[checkpoint("latency")]
struct TlsClientHello<'a> {
    payload: &'a [&'a [u8]],
}

#[event("tls:server_hello")]
#[checkpoint("latency")]
struct TlsServerHello<'a> {
    payload: &'a [&'a [u8]],
}

#[event("transport:rx_stream_progress")]
struct RxStreamProgress {
    #[measure("bytes", Bytes)]
    #[counter("bytes.total", Bytes)]
    bytes: usize,
}

#[event("transport:tx_stream_progress")]
struct TxStreamProgress {
    #[measure("bytes", Bytes)]
    #[counter("bytes.total", Bytes)]
    bytes: usize,
}

#[event("connectivity::keep_alive_timer_expired")]
pub struct KeepAliveTimerExpired {
    timeout: Duration,
}

#[event("connectivity:mtu_updated")]
/// The maximum transmission unit (MTU) and/or MTU probing status for the path has changed
struct MtuUpdated {
    path_id: u64,
    /// The maximum QUIC datagram size, not including UDP and IP headers
    #[measure("mtu", Bytes)]
    mtu: u16,
    #[nominal_counter("cause")]
    cause: MtuUpdatedCause,
    /// The search for the maximum MTU has completed for now
    #[bool_counter("search_complete")]
    search_complete: bool,
}

#[event("recovery:slow_start_exited")]
/// The slow start congestion controller state has been exited
struct SlowStartExited {
    path_id: u64,
    #[nominal_counter("cause")]
    #[nominal_checkpoint("latency")]
    cause: SlowStartExitCause,
    #[measure("congestion_window", Bytes)]
    congestion_window: u32,
}

#[event("recovery:delivery_rate_sampled")]
/// A new delivery rate sample has been generated
/// Note: This event is only recorded for congestion controllers that support
///       bandwidth estimates, such as BBR
struct DeliveryRateSampled {
    path_id: u64,
    rate_sample: RateSample,
}

#[event("recovery:pacing_rate_updated")]
/// The pacing rate has been updated
struct PacingRateUpdated {
    path_id: u64,
    #[measure("bytes_per_second", Bytes)]
    bytes_per_second: u64,
    #[measure("burst_size", Bytes)]
    burst_size: u32,
    #[measure("pacing_gain")]
    pacing_gain: f32,
}

#[event("recovery:bbr_state_changed")]
/// The BBR state has changed
struct BbrStateChanged {
    path_id: u64,
    #[nominal_counter("state")]
    state: BbrState,
}

#[event("transport:dc_state_changed")]
/// The DC state has changed
#[checkpoint("version_negotiated.latency", |evt| matches!(evt.state, DcState::VersionNegotiated { .. }))]
#[checkpoint("no_version_negotiated.latency", |evt| matches!(evt.state, DcState::VersionNegotiated { .. }))]
#[checkpoint("path_secrets.latency", |evt| matches!(evt.state, DcState::PathSecretsReady { .. }))]
#[checkpoint("complete.latency", |evt| matches!(evt.state, DcState::Complete { .. }))]
struct DcStateChanged {
    #[nominal_counter("state")]
    state: DcState,
}

#[event("transport:dc_path_created")]
/// The DC path has been created
struct DcPathCreated<'a> {
    /// This is the dc::Path struct, it's just type-erased. But if an event subscriber knows the
    /// type they can downcast.
    path: &'a (dyn core::any::Any + Send + 'static),
}

// NOTE - This event MUST come last, since connection-level aggregation depends on it
#[event("connectivity:connection_closed")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.1.3
/// Connection closed
#[checkpoint("latency")]
struct ConnectionClosed {
    #[nominal_counter("error")]
    error: crate::connection::Error,
}
