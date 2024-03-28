// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

struct ConnectionMeta {
    #[builder(crate::endpoint::Type)]
    endpoint_type: EndpointType,

    id: u64,

    #[builder(crate::time::Timestamp)]
    timestamp: crate::event::Timestamp,
}

struct EndpointMeta {
    #[builder(crate::endpoint::Type)]
    endpoint_type: EndpointType,

    #[builder(crate::time::Timestamp)]
    timestamp: crate::event::Timestamp,
}

struct ConnectionInfo {}

// https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.3
struct TransportParameters<'a> {
    original_destination_connection_id: Option<ConnectionId<'a>>,
    initial_source_connection_id: Option<ConnectionId<'a>>,
    retry_source_connection_id: Option<ConnectionId<'a>>,
    stateless_reset_token: Option<&'a [u8]>,
    preferred_address: Option<PreferredAddress<'a>>,
    migration_support: bool,
    max_idle_timeout: Duration,
    ack_delay_exponent: u8,
    max_ack_delay: Duration,
    max_udp_payload_size: u64,
    active_connection_id_limit: u64,
    initial_max_stream_data_bidi_local: u64,
    initial_max_stream_data_bidi_remote: u64,
    initial_max_stream_data_uni: u64,
    initial_max_streams_bidi: u64,
    initial_max_streams_uni: u64,
    max_datagram_frame_size: u64,
}

struct PreferredAddress<'a> {
    ipv4_address: Option<SocketAddress<'a>>,
    ipv6_address: Option<SocketAddress<'a>>,
    connection_id: ConnectionId<'a>,
    stateless_reset_token: &'a [u8],
}

impl<'a> IntoEvent<builder::PreferredAddress<'a>>
    for &'a crate::transport::parameters::PreferredAddress
{
    #[inline]
    fn into_event(self) -> builder::PreferredAddress<'a> {
        builder::PreferredAddress {
            ipv4_address: self.ipv4_address.as_ref().map(|addr| addr.into_event()),
            ipv6_address: self.ipv6_address.as_ref().map(|addr| addr.into_event()),
            connection_id: self.connection_id.into_event(),
            stateless_reset_token: self.stateless_reset_token.as_ref(),
        }
    }
}

impl<'a> IntoEvent<builder::SocketAddress<'a>> for &'a crate::inet::ipv4::SocketAddressV4 {
    #[inline]
    fn into_event(self) -> builder::SocketAddress<'a> {
        builder::SocketAddress::IpV4 {
            ip: &self.ip.octets,
            port: self.port.into(),
        }
    }
}

impl<'a> IntoEvent<builder::SocketAddress<'a>> for &'a crate::inet::ipv6::SocketAddressV6 {
    #[inline]
    fn into_event(self) -> builder::SocketAddress<'a> {
        builder::SocketAddress::IpV6 {
            ip: &self.ip.octets,
            port: self.port.into(),
        }
    }
}

impl IntoEvent<bool> for &crate::transport::parameters::MigrationSupport {
    #[inline]
    fn into_event(self) -> bool {
        match self {
            crate::transport::parameters::MigrationSupport::Enabled => true,
            crate::transport::parameters::MigrationSupport::Disabled => false,
        }
    }
}

#[builder_derive(derive(Copy))]
struct Path<'a> {
    local_addr: SocketAddress<'a>,
    local_cid: ConnectionId<'a>,
    remote_addr: SocketAddress<'a>,
    remote_cid: ConnectionId<'a>,
    id: u64,
    is_active: bool,
}

#[derive(Clone)]
#[builder_derive(derive(Copy))]
struct ConnectionId<'a> {
    bytes: &'a [u8],
}

impl<'a> core::fmt::Debug for ConnectionId<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "0x")?;
        for byte in self.bytes {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

macro_rules! impl_conn_id {
    ($name:ident) => {
        impl<'a> IntoEvent<builder::ConnectionId<'a>> for &'a crate::connection::id::$name {
            #[inline]
            fn into_event(self) -> builder::ConnectionId<'a> {
                builder::ConnectionId {
                    bytes: self.as_bytes(),
                }
            }
        }
    };
}

impl_conn_id!(LocalId);
impl_conn_id!(PeerId);
impl_conn_id!(UnboundedId);
impl_conn_id!(InitialId);

#[derive(Clone)]
#[builder_derive(derive(Copy))]
enum SocketAddress<'a> {
    IpV4 { ip: &'a [u8; 4], port: u16 },
    IpV6 { ip: &'a [u8; 16], port: u16 },
}

impl<'a> core::fmt::Debug for SocketAddress<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::IpV4 { ip, port } => {
                let addr = crate::inet::SocketAddressV4::new(**ip, *port);
                write!(f, "{addr}")?;
            }
            Self::IpV6 { ip, port } => {
                let addr = crate::inet::SocketAddressV6::new(**ip, *port);
                write!(f, "{addr}")?;
            }
        }
        Ok(())
    }
}

impl<'a> core::fmt::Display for SocketAddress<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::IpV4 { ip, port } => {
                let addr = crate::inet::SocketAddressV4::new(**ip, *port);
                addr.fmt(f)?;
            }
            Self::IpV6 { ip, port } => {
                let addr = crate::inet::SocketAddressV6::new(**ip, *port);
                addr.fmt(f)?;
            }
        }
        Ok(())
    }
}

impl<'a> SocketAddress<'a> {
    #[inline]
    pub fn ip(&self) -> &'a [u8] {
        match self {
            Self::IpV4 { ip, .. } => &ip[..],
            Self::IpV6 { ip, .. } => &ip[..],
        }
    }

    #[inline]
    pub fn port(&self) -> u16 {
        match self {
            Self::IpV4 { port, .. } => *port,
            Self::IpV6 { port, .. } => *port,
        }
    }
}

impl<'a> IntoEvent<api::SocketAddress<'a>> for &'a crate::inet::SocketAddress {
    #[inline]
    fn into_event(self) -> api::SocketAddress<'a> {
        match self {
            crate::inet::SocketAddress::IpV4(addr) => api::SocketAddress::IpV4 {
                ip: &addr.ip.octets,
                port: addr.port.into(),
            },
            crate::inet::SocketAddress::IpV6(addr) => api::SocketAddress::IpV6 {
                ip: &addr.ip.octets,
                port: addr.port.into(),
            },
        }
    }
}

impl<'a> IntoEvent<builder::SocketAddress<'a>> for &'a crate::inet::SocketAddress {
    #[inline]
    fn into_event(self) -> builder::SocketAddress<'a> {
        match self {
            crate::inet::SocketAddress::IpV4(addr) => addr.into_event(),
            crate::inet::SocketAddress::IpV6(addr) => addr.into_event(),
        }
    }
}

#[cfg(feature = "std")]
impl From<SocketAddress<'_>> for std::net::SocketAddr {
    #[inline]
    fn from(address: SocketAddress) -> Self {
        use std::net;
        match address {
            SocketAddress::IpV4 { ip, port } => {
                let ip = net::IpAddr::V4(net::Ipv4Addr::from(*ip));
                Self::new(ip, port)
            }
            SocketAddress::IpV6 { ip, port } => {
                let ip = net::IpAddr::V6(net::Ipv6Addr::from(*ip));
                Self::new(ip, port)
            }
        }
    }
}

#[cfg(feature = "std")]
impl From<&SocketAddress<'_>> for std::net::SocketAddr {
    #[inline]
    fn from(address: &SocketAddress) -> Self {
        use std::net;
        match address {
            SocketAddress::IpV4 { ip, port } => {
                let ip = net::IpAddr::V4(net::Ipv4Addr::from(**ip));
                Self::new(ip, *port)
            }
            SocketAddress::IpV6 { ip, port } => {
                let ip = net::IpAddr::V6(net::Ipv6Addr::from(**ip));
                Self::new(ip, *port)
            }
        }
    }
}

enum DuplicatePacketError {
    /// The packet number was already received and is a duplicate.
    Duplicate,

    /// The received packet number was outside the range of tracked packet numbers.
    ///
    /// This can happen when packets are heavily delayed or reordered. Currently, the maximum
    /// amount of reordering is limited to 128 packets. For example, if packet number `142`
    /// is received, the allowed range would be limited to `14-142`. If an endpoint received
    /// packet `< 14`, it would trigger this event.
    TooOld,
}

impl IntoEvent<builder::DuplicatePacketError> for crate::packet::number::SlidingWindowError {
    #[inline]
    fn into_event(self) -> builder::DuplicatePacketError {
        use crate::packet::number::SlidingWindowError;
        match self {
            SlidingWindowError::TooOld => builder::DuplicatePacketError::TooOld {},
            SlidingWindowError::Duplicate => builder::DuplicatePacketError::Duplicate {},
        }
    }
}

struct EcnCounts {
    /// A variable-length integer representing the total number of packets
    /// received with the ECT(0) codepoint.
    ect_0_count: u64,

    /// A variable-length integer representing the total number of packets
    /// received with the ECT(1) codepoint.
    ect_1_count: u64,

    /// A variable-length integer representing the total number of packets
    /// received with the CE codepoint.
    ce_count: u64,
}

impl IntoEvent<builder::EcnCounts> for crate::frame::ack::EcnCounts {
    #[inline]
    fn into_event(self) -> builder::EcnCounts {
        builder::EcnCounts {
            ect_0_count: self.ect_0_count.into_event(),
            ect_1_count: self.ect_1_count.into_event(),
            ce_count: self.ce_count.into_event(),
        }
    }
}

//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#A.7
enum Frame {
    Padding,
    Ping,
    Ack {
        ecn_counts: Option<EcnCounts>,
        largest_acknowledged: u64,
        ack_range_count: u64,
    },
    ResetStream {
        id: u64,
        error_code: u64,
        final_size: u64,
    },
    StopSending {
        id: u64,
        error_code: u64,
    },
    Crypto {
        offset: u64,
        len: u16,
    },
    NewToken,
    Stream {
        id: u64,
        offset: u64,
        len: u16,
        is_fin: bool,
    },
    MaxData {
        value: u64,
    },
    MaxStreamData {
        stream_type: StreamType,
        id: u64,
        value: u64,
    },
    MaxStreams {
        stream_type: StreamType,
        value: u64,
    },
    DataBlocked {
        data_limit: u64,
    },
    StreamDataBlocked {
        stream_id: u64,
        stream_data_limit: u64,
    },
    StreamsBlocked {
        stream_type: StreamType,
        stream_limit: u64,
    },
    NewConnectionId {
        sequence_number: u64,
        retire_prior_to: u64,
    },
    RetireConnectionId,
    PathChallenge,
    PathResponse,
    ConnectionClose,
    HandshakeDone,
    Datagram {
        len: u16,
    },
}

impl IntoEvent<builder::Frame> for &crate::frame::Padding {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::Padding {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::Ping {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::Ping {}
    }
}

impl<AckRanges: crate::frame::ack::AckRanges> IntoEvent<builder::Frame>
    for &crate::frame::Ack<AckRanges>
{
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::Ack {
            ecn_counts: self.ecn_counts.map(|val| val.into_event()),
            largest_acknowledged: self.largest_acknowledged().into_event(),
            ack_range_count: self.ack_ranges().len() as u64,
        }
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::ResetStream {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::ResetStream {
            id: self.stream_id.as_u64(),
            error_code: self.application_error_code.as_u64(),
            final_size: self.final_size.as_u64(),
        }
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::StopSending {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::StopSending {
            id: self.stream_id.as_u64(),
            error_code: self.application_error_code.as_u64(),
        }
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::NewToken<'a> {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::NewToken {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::MaxData {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::MaxData {
            value: self.maximum_data.as_u64(),
        }
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::MaxStreamData {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::MaxStreamData {
            id: self.stream_id.as_u64(),
            stream_type: crate::stream::StreamId::from_varint(self.stream_id)
                .stream_type()
                .into_event(),
            value: self.maximum_stream_data.as_u64(),
        }
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::MaxStreams {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::MaxStreams {
            stream_type: self.stream_type.into_event(),
            value: self.maximum_streams.as_u64(),
        }
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::DataBlocked {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::DataBlocked {
            data_limit: self.data_limit.as_u64(),
        }
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::StreamDataBlocked {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::StreamDataBlocked {
            stream_id: self.stream_id.as_u64(),
            stream_data_limit: self.stream_data_limit.as_u64(),
        }
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::StreamsBlocked {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::StreamsBlocked {
            stream_type: self.stream_type.into_event(),
            stream_limit: self.stream_limit.as_u64(),
        }
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::NewConnectionId<'a> {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::NewConnectionId {
            sequence_number: self.sequence_number.as_u64(),
            retire_prior_to: self.retire_prior_to.as_u64(),
        }
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::RetireConnectionId {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::RetireConnectionId {}
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::PathChallenge<'a> {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::PathChallenge {}
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::PathResponse<'a> {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::PathResponse {}
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::ConnectionClose<'a> {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::ConnectionClose {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::HandshakeDone {
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::HandshakeDone {}
    }
}

impl<Data> IntoEvent<builder::Frame> for &crate::frame::Stream<Data>
where
    Data: s2n_codec::EncoderValue,
{
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::Stream {
            id: self.stream_id.as_u64(),
            offset: self.offset.as_u64(),
            len: self.data.encoding_size() as _,
            is_fin: self.is_fin,
        }
    }
}

impl<Data> IntoEvent<builder::Frame> for &crate::frame::Crypto<Data>
where
    Data: s2n_codec::EncoderValue,
{
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::Crypto {
            offset: self.offset.as_u64(),
            len: self.data.encoding_size() as _,
        }
    }
}

impl<Data> IntoEvent<builder::Frame> for &crate::frame::Datagram<Data>
where
    Data: s2n_codec::EncoderValue,
{
    #[inline]
    fn into_event(self) -> builder::Frame {
        builder::Frame::Datagram {
            len: self.data.encoding_size() as _,
        }
    }
}

enum StreamType {
    Bidirectional,
    Unidirectional,
}

impl IntoEvent<builder::StreamType> for &crate::stream::StreamType {
    #[inline]
    fn into_event(self) -> builder::StreamType {
        match self {
            crate::stream::StreamType::Bidirectional => builder::StreamType::Bidirectional {},
            crate::stream::StreamType::Unidirectional => builder::StreamType::Unidirectional {},
        }
    }
}

//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#A.2
//
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#A.4
enum PacketHeader {
    Initial { number: u64, version: u32 },
    Handshake { number: u64, version: u32 },
    ZeroRtt { number: u64, version: u32 },
    OneRtt { number: u64 },
    Retry { version: u32 },
    // The Version field of a Version Negotiation packet MUST be set to 0x00000000.
    VersionNegotiation,
    StatelessReset,
}

impl builder::PacketHeader {
    #[inline]
    pub fn new(
        packet_number: crate::packet::number::PacketNumber,
        version: u32,
    ) -> builder::PacketHeader {
        use crate::packet::number::PacketNumberSpace;
        use builder::PacketHeader;

        match packet_number.space() {
            PacketNumberSpace::Initial => PacketHeader::Initial {
                number: packet_number.into_event(),
                version,
            },
            PacketNumberSpace::Handshake => PacketHeader::Handshake {
                number: packet_number.into_event(),
                version,
            },
            PacketNumberSpace::ApplicationData => PacketHeader::OneRtt {
                number: packet_number.into_event(),
            },
        }
    }
}

enum PacketType {
    Initial,
    Handshake,
    ZeroRtt,
    OneRtt,
    Retry,
    VersionNegotiation,
    StatelessReset,
}

enum KeyType {
    Initial,
    Handshake,
    ZeroRtt,
    OneRtt { generation: u16 },
}

/// A context from which the event is being emitted
///
/// An event can occur in the context of an Endpoint or Connection
enum Subject {
    Endpoint,

    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#4
    //# it is recommended to use
    //# QUIC's Original Destination Connection ID (ODCID, the CID chosen by
    //# the client when first contacting the server)
    /// This maps to an internal connection id, which is a stable identifier across CID changes.
    Connection {
        id: u64,
    },
}

/// An endpoint may be either a Server or a Client
#[exhaustive]
enum EndpointType {
    Server,
    Client,
}

impl core::fmt::Display for EndpointType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Client {} => write!(f, "client"),
            Self::Server {} => write!(f, "server"),
        }
    }
}

impl IntoEvent<api::EndpointType> for crate::endpoint::Type {
    #[inline]
    fn into_event(self) -> api::EndpointType {
        match self {
            Self::Client => api::EndpointType::Client {},
            Self::Server => api::EndpointType::Server {},
        }
    }
}

impl IntoEvent<builder::EndpointType> for crate::endpoint::Type {
    #[inline]
    fn into_event(self) -> builder::EndpointType {
        match self {
            Self::Client => builder::EndpointType::Client {},
            Self::Server => builder::EndpointType::Server {},
        }
    }
}

enum DatagramDropReason {
    /// There was an error while attempting to decode the datagram.
    DecodingFailed,
    /// There was an error while parsing the Retry token.
    InvalidRetryToken,
    /// The peer specified an unsupported QUIC version.
    UnsupportedVersion,
    /// The peer sent an invalid Destination Connection Id.
    InvalidDestinationConnectionId,
    /// The peer sent an invalid Source Connection Id.
    InvalidSourceConnectionId,
    /// The Destination Connection Id is unknown and does not map to a Connection.
    ///
    /// Connections are mapped to Destination Connections Ids (DCID) and packets
    /// in a Datagram are routed to a connection based on the DCID in the first
    /// packet. If a Connection is not found for the specified DCID then the
    /// datagram can not be processed and is dropped.
    UnknownDestinationConnectionId,
    /// The connection attempt was rejected.
    RejectedConnectionAttempt,
    /// A datagram was received from an unknown server address.
    UnknownServerAddress,
    /// The peer initiated a connection migration before the handshake was confirmed.
    ConnectionMigrationDuringHandshake,
    /// The attempted connection migration was rejected.
    RejectedConnectionMigration,
    /// The maximum number of paths per connection was exceeded.
    PathLimitExceeded,
    /// The peer initiated a connection migration without supplying enough connection IDs to use.
    InsufficientConnectionIds,
}

enum KeySpace {
    Initial {},
    Handshake {},
    ZeroRtt {},
    OneRtt {},
}

enum PacketSkipReason {
    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
    //# If the sender wants to elicit a faster acknowledgement on PTO, it can
    //# skip a packet number to eliminate the acknowledgment delay.
    /// Skipped a packet number to elicit a quicker PTO acknowledgment
    PtoProbe {},

    //= https://www.rfc-editor.org/rfc/rfc9000#section-21.4
    //# An endpoint that acknowledges packets it has not received might cause
    //# a congestion controller to permit sending at rates beyond what the
    //# network supports.  An endpoint MAY skip packet numbers when sending
    //# packets to detect this behavior.
    /// Skipped a packet number to detect an Optimistic Ack attack
    OptimisticAckMitigation {},
}

enum PacketDropReason<'a> {
    /// A connection error occurred and is no longer able to process packets.
    ConnectionError { path: Path<'a> },
    /// The handshake needed to be complete before processing the packet.
    ///
    /// To ensure the connection stays secure, short packets can only be processed
    /// once the handshake has completed.
    HandshakeNotComplete { path: Path<'a> },
    /// The packet contained a version which did not match the version negotiated
    /// during the handshake.
    VersionMismatch { version: u32, path: Path<'a> },
    /// A datagram contained more than one destination connection ID, which is
    /// not allowed.
    ConnectionIdMismatch {
        packet_cid: &'a [u8],
        path: Path<'a>,
    },
    /// There was a failure when attempting to remove header protection.
    UnprotectFailed { space: KeySpace, path: Path<'a> },
    /// There was a failure when attempting to decrypt the packet.
    DecryptionFailed {
        path: Path<'a>,
        packet_header: PacketHeader,
    },
    /// Packet decoding failed.
    ///
    /// The payload is decoded one packet at a time. If decoding fails
    /// then the remaining packets are also discarded.
    DecodingFailed { path: Path<'a> },
    /// The client received a non-empty retry token.
    NonEmptyRetryToken { path: Path<'a> },
    /// A Retry packet was discarded.
    RetryDiscarded {
        reason: RetryDiscardReason<'a>,
        path: Path<'a>,
    },
    /// The received Initial packet was not transported in a datagram of at least 1200 bytes
    UndersizedInitialPacket { path: Path<'a> },
    /// The destination connection ID in the packet was the initial connection ID but was in
    /// a non-initial packet.
    InitialConnectionIdInvalidSpace {
        path: Path<'a>,
        packet_type: PacketType,
    },
}

#[deprecated(note = "use on_rx_ack_range_dropped event instead")]
enum AckAction {
    /// Ack range for received packets was dropped due to space constraints
    ///
    /// For the purpose of processing Acks, RX packet numbers are stored as
    /// packet_number ranges in an IntervalSet; only lower and upper bounds
    /// are stored instead of individual packet_numbers. Ranges are merged
    /// when possible so only disjointed ranges are stored.
    ///
    /// When at `capacity`, the lowest packet_number range is dropped.
    RxAckRangeDropped {
        /// The packet number range which was dropped
        packet_number_range: core::ops::RangeInclusive<u64>,
        /// The number of disjoint ranges the IntervalSet can store
        capacity: usize,
        /// The store packet_number range in the IntervalSet
        stored_range: core::ops::RangeInclusive<u64>,
    },
}

enum RetryDiscardReason<'a> {
    /// Received a Retry packet with SCID field equal to DCID field.
    ScidEqualsDcid { cid: &'a [u8] },
    /// A client only processes at most one Retry packet.
    RetryAlreadyProcessed,
    /// The client discards Retry packets if a valid Initial packet
    /// has been received and processed.
    InitialAlreadyProcessed,
    /// The Retry packet received contained an invalid retry integrity tag
    InvalidIntegrityTag,
}

enum MigrationDenyReason {
    BlockedPort,
    PortScopeChanged,
    IpScopeChange,
    ConnectionMigrationDisabled,
}

/// The current state of the ECN controller for the path
enum EcnState {
    /// ECN capability is being actively tested
    Testing,
    /// ECN capability has been tested, but not validated yet
    Unknown,
    /// ECN capability testing has failed validation
    Failed,
    /// ECN capability has been confirmed
    Capable,
}

/// Events tracking the progress of handshake status
enum HandshakeStatus {
    /// The handshake has completed.
    Complete,
    /// The handshake has been confirmed.
    Confirmed,
    /// A HANDSHAKE_DONE frame was delivered or received.
    ///
    /// A Client endpoint receives a HANDSHAKE_DONE frame and
    /// only a Server is allowed to send the HANDSHAKE_DONE
    /// frame.
    HandshakeDoneAcked,

    /// A HANDSHAKE_DONE frame was declared lost.
    ///
    /// The Server is responsible for re-transmitting the
    /// HANDSHAKE_DONE frame until it is acked by the peer.
    HandshakeDoneLost,
}

/// The source that caused a congestion event
enum CongestionSource {
    /// Explicit Congestion Notification
    Ecn,
    /// One or more packets were detected lost
    PacketLoss,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(non_camel_case_types)] // we prefer to match the standard identifier
enum CipherSuite {
    TLS_AES_128_GCM_SHA256,
    TLS_AES_256_GCM_SHA384,
    TLS_CHACHA20_POLY1305_SHA256,
    Unknown,
}

impl CipherSuite {
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TLS_AES_128_GCM_SHA256 {} => "TLS_AES_128_GCM_SHA256",
            Self::TLS_AES_256_GCM_SHA384 {} => "TLS_AES_256_GCM_SHA384",
            Self::TLS_CHACHA20_POLY1305_SHA256 {} => "TLS_CHACHA20_POLY1305_SHA256",
            Self::Unknown {} => "UNKNOWN",
        }
    }
}

enum PathChallengeStatus {
    Validated,
    Abandoned,
}

/// The reason the slow start congestion controller state has been exited
enum SlowStartExitCause {
    /// A packet was determined lost
    PacketLoss,
    /// An Explicit Congestion Notification: Congestion Experienced marking was received
    Ecn,
    /// The round trip time estimate was updated
    Rtt,
    /// Slow Start exited due to a reason other than those above
    ///
    /// With the Cubic congestion controller, this reason is used after the initial exiting of
    /// Slow Start, when the previously determined Slow Start threshold is exceed by the
    /// congestion window.
    Other,
}

/// The reason the MTU was updated
enum MtuUpdatedCause {
    /// The MTU was initialized with the default value
    NewPath,
    /// An MTU probe was acknowledged by the peer
    ProbeAcknowledged,
    /// A blackhole was detected
    Blackhole,
    /// An early packet using the configured InitialMtu was lost
    InitialMtuPacketLost,
}

/// A bandwidth delivery rate estimate with associated metadata
struct RateSample {
    /// The length of the sampling interval
    interval: Duration,
    /// The amount of data in bytes marked as delivered over the sampling interval
    delivered_bytes: u64,
    /// The amount of data in bytes marked as lost over the sampling interval
    lost_bytes: u64,
    /// The number of packets marked as explicit congestion experienced over the sampling interval
    ecn_ce_count: u64,
    /// PacketInfo::is_app_limited from the most recent acknowledged packet
    is_app_limited: bool,
    /// PacketInfo::delivered_bytes from the most recent acknowledged packet
    prior_delivered_bytes: u64,
    /// PacketInfo::bytes_in_flight from the most recent acknowledged packet
    bytes_in_flight: u32,
    /// PacketInfo::lost_bytes from the most recent acknowledged packet
    prior_lost_bytes: u64,
    /// PacketInfo::ecn_ce_count from the most recent acknowledged packet
    prior_ecn_ce_count: u64,
    /// The delivery rate for this rate sample
    delivery_rate_bytes_per_second: u64,
}

// The BBR congestion controller State
enum BbrState {
    Startup,
    Drain,
    ProbeBwDown,
    ProbeBwCruise,
    ProbeBwRefill,
    ProbeBwUp,
    ProbeRtt,
}
