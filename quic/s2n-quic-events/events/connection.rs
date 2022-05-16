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

#[event("transport:packet_sent")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.5
/// Packet was sent by a connection
struct PacketSent {
    packet_header: PacketHeader,
}

#[event("transport:packet_received")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.6
/// Packet was received by a connection
struct PacketReceived {
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
    packet_header: PacketHeader,
    path_id: u64,
    frame: Frame,
}

#[event("transport:frame_received")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.6
// This diverges a bit from the qlog spec, which prefers to log data as part of the
// packet events.
/// Frame was received
struct FrameReceived<'a> {
    packet_header: PacketHeader,
    path: Path<'a>,
    frame: Frame,
}

#[event("recovery:packet_lost")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.4.5
/// Packet was lost
struct PacketLost<'a> {
    packet_header: PacketHeader,
    path: Path<'a>,
    bytes_lost: u16,
    is_mtu_probe: bool,
}

#[event("recovery:metrics_updated")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.4.2
/// Recovery metrics updated
struct RecoveryMetrics<'a> {
    path: Path<'a>,
    min_rtt: Duration,
    smoothed_rtt: Duration,
    latest_rtt: Duration,
    rtt_variance: Duration,
    max_ack_delay: Duration,
    pto_count: u32,
    congestion_window: u32,
    bytes_in_flight: u32,
}

#[event("recovery:congestion")]
/// Congestion (ECN or packet loss) has occurred
struct Congestion<'a> {
    path: Path<'a>,
    source: CongestionSource,
}

#[event("recovery:ack_processed")]
/// Events related to ACK processing
struct AckProcessed<'a> {
    action: AckAction,
    path: Path<'a>,
}

#[event("transport:packet_dropped")]
/// Packet was dropped with the given reason
struct PacketDropped<'a> {
    reason: PacketDropReason<'a>,
}

#[event("security:key_update")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.2.1
/// Crypto key updated
struct KeyUpdate {
    key_type: KeyType,
    cipher_suite: CipherSuite,
}

#[event("security:key_space_discarded")]
struct KeySpaceDiscarded {
    space: KeySpace,
}

#[event("connectivity:connection_started")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.1.2
/// Connection started
struct ConnectionStarted<'a> {
    path: Path<'a>,
}

#[event("connectivity:connection_closed")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.1.3
/// Connection closed
struct ConnectionClosed {
    error: crate::connection::Error,
}

#[event("transport:duplicate_packet")]
/// Duplicate packet received
struct DuplicatePacket<'a> {
    packet_header: PacketHeader,
    path: Path<'a>,
    error: DuplicatePacketError,
}

#[event("transport:transport_parameters_received")]
/// Transport parameters received by connection
struct TransportParametersReceived<'a> {
    transport_parameters: TransportParameters<'a>,
}

#[event("transport:datagram_sent")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.10
/// Datagram sent by a connection
struct DatagramSent {
    len: u16,
    /// The GSO offset at which this datagram was written
    ///
    /// If this value is greater than 0, it indicates that this datagram has been sent with other
    /// segments in a single buffer.
    ///
    /// See the [Linux kernel documentation](https://www.kernel.org/doc/html/latest/networking/segmentation-offloads.html#generic-segmentation-offload) for more details.
    gso_offset: usize,
}

#[event("transport:datagram_received")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.11
/// Datagram received by a connection
struct DatagramReceived {
    len: u16,
}

#[event("transport:datagram_dropped")]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.12
/// Datagram dropped by a connection
struct DatagramDropped {
    len: u16,
    reason: DatagramDropReason,
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
    state: EcnState,
}

#[event("connectivity:connection_migration_denied")]
struct ConnectionMigrationDenied {
    reason: MigrationDenyReason,
}

#[event("connectivity:handshake_status_updated")]
struct HandshakeStatusUpdated {
    status: HandshakeStatus,
}

#[event("connectivity:path_challenge_updated")]
/// Path challenge updated
struct PathChallengeUpdated<'a> {
    path_challenge_status: PathChallengeStatus,
    path: Path<'a>,
    challenge_data: &'a [u8],
}

#[event("tls:client_hello")]
struct TlsClientHello<'a> {
    payload: &'a [&'a [u8]],
}

#[event("tls:server_hello")]
struct TlsServerHello<'a> {
    payload: &'a [&'a [u8]],
}

#[event("transport:rx_stream_progress")]
struct RxStreamProgress {
    bytes: usize,
}

#[event("transport:tx_stream_progress")]
struct TxStreamProgress {
    bytes: usize,
}

#[event("connectivity::keep_alive_timer_expired")]
pub struct KeepAliveTimerExpired {
    timeout: Duration,
}
