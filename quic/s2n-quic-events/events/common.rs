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

struct Path<'a> {
    local_addr: SocketAddress<'a>,
    local_cid: ConnectionId<'a>,
    remote_addr: SocketAddress<'a>,
    remote_cid: ConnectionId<'a>,
    id: u64,
    is_active: bool,
}

#[derive(Clone)]
struct ConnectionId<'a> {
    bytes: &'a [u8],
}

impl<'a> core::fmt::Debug for ConnectionId<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "0x")?;
        for byte in self.bytes {
            write!(f, "{:02x}", byte)?;
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
enum SocketAddress<'a> {
    IpV4 { ip: &'a [u8; 4], port: u16 },
    IpV6 { ip: &'a [u8; 16], port: u16 },
}

impl<'a> core::fmt::Debug for SocketAddress<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::IpV4 { ip, port } => {
                let addr = crate::inet::SocketAddressV4::new(**ip, *port);
                write!(f, "{}", addr)?;
            }
            Self::IpV6 { ip, port } => {
                let addr = crate::inet::SocketAddressV6::new(**ip, *port);
                write!(f, "{}", addr)?;
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
            crate::inet::SocketAddress::IpV4(addr) => builder::SocketAddress::IpV4 {
                ip: &addr.ip.octets,
                port: addr.port.into(),
            },
            crate::inet::SocketAddress::IpV6(addr) => builder::SocketAddress::IpV6 {
                ip: &addr.ip.octets,
                port: addr.port.into(),
            },
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
    fn into_event(self) -> builder::DuplicatePacketError {
        use crate::packet::number::SlidingWindowError;
        match self {
            SlidingWindowError::TooOld => builder::DuplicatePacketError::TooOld {},
            SlidingWindowError::Duplicate => builder::DuplicatePacketError::Duplicate {},
        }
    }
}

//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.7
enum Frame {
    Padding,
    Ping,
    Ack,
    ResetStream,
    StopSending,
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
    MaxData,
    MaxStreamData,
    MaxStreams {
        stream_type: StreamType,
    },
    DataBlocked,
    StreamDataBlocked,
    StreamsBlocked {
        stream_type: StreamType,
    },
    NewConnectionId,
    RetireConnectionId,
    PathChallenge,
    PathResponse,
    ConnectionClose,
    HandshakeDone,
}

impl IntoEvent<builder::Frame> for &crate::frame::Padding {
    fn into_event(self) -> builder::Frame {
        builder::Frame::Padding {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::Ping {
    fn into_event(self) -> builder::Frame {
        builder::Frame::Ping {}
    }
}

impl<AckRanges> IntoEvent<builder::Frame> for &crate::frame::Ack<AckRanges> {
    fn into_event(self) -> builder::Frame {
        builder::Frame::Ack {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::ResetStream {
    fn into_event(self) -> builder::Frame {
        builder::Frame::ResetStream {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::StopSending {
    fn into_event(self) -> builder::Frame {
        builder::Frame::ResetStream {}
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::NewToken<'a> {
    fn into_event(self) -> builder::Frame {
        builder::Frame::NewToken {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::MaxData {
    fn into_event(self) -> builder::Frame {
        builder::Frame::MaxData {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::MaxStreamData {
    fn into_event(self) -> builder::Frame {
        builder::Frame::MaxStreamData {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::MaxStreams {
    fn into_event(self) -> builder::Frame {
        builder::Frame::MaxStreams {
            stream_type: self.stream_type.into_event(),
        }
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::DataBlocked {
    fn into_event(self) -> builder::Frame {
        builder::Frame::DataBlocked {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::StreamDataBlocked {
    fn into_event(self) -> builder::Frame {
        builder::Frame::StreamDataBlocked {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::StreamsBlocked {
    fn into_event(self) -> builder::Frame {
        builder::Frame::StreamsBlocked {
            stream_type: self.stream_type.into_event(),
        }
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::NewConnectionId<'a> {
    fn into_event(self) -> builder::Frame {
        builder::Frame::NewConnectionId {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::RetireConnectionId {
    fn into_event(self) -> builder::Frame {
        builder::Frame::RetireConnectionId {}
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::PathChallenge<'a> {
    fn into_event(self) -> builder::Frame {
        builder::Frame::PathChallenge {}
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::PathResponse<'a> {
    fn into_event(self) -> builder::Frame {
        builder::Frame::PathResponse {}
    }
}

impl<'a> IntoEvent<builder::Frame> for &crate::frame::ConnectionClose<'a> {
    fn into_event(self) -> builder::Frame {
        builder::Frame::ConnectionClose {}
    }
}

impl IntoEvent<builder::Frame> for &crate::frame::HandshakeDone {
    fn into_event(self) -> builder::Frame {
        builder::Frame::HandshakeDone {}
    }
}

impl<Data> IntoEvent<builder::Frame> for &crate::frame::Stream<Data>
where
    Data: s2n_codec::EncoderValue,
{
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
    fn into_event(self) -> builder::Frame {
        builder::Frame::Crypto {
            offset: self.offset.as_u64(),
            len: self.data.encoding_size() as _,
        }
    }
}

enum StreamType {
    Bidirectional,
    Unidirectional,
}

impl<'a> IntoEvent<builder::StreamType> for &crate::stream::StreamType {
    fn into_event(self) -> builder::StreamType {
        match self {
            crate::stream::StreamType::Bidirectional => builder::StreamType::Bidirectional {},
            crate::stream::StreamType::Unidirectional => builder::StreamType::Unidirectional {},
        }
    }
}

//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.2
//
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.4
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
    pub fn new(
        packet_number: crate::packet::number::PacketNumber,
        version: u32,
    ) -> builder::PacketHeader {
        use crate::packet::number::PacketNumberSpace;
        use builder::PacketHeader;

        match packet_number.space() {
            PacketNumberSpace::Initial => PacketHeader::Initial {
                number: packet_number.as_u64(),
                version,
            },
            PacketNumberSpace::Handshake => PacketHeader::Handshake {
                number: packet_number.as_u64(),
                version,
            },
            PacketNumberSpace::ApplicationData => PacketHeader::OneRtt {
                number: packet_number.as_u64(),
            },
        }
    }
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

    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#4
    //# it is recommended to use
    //# QUIC's Original Destination Connection ID (ODCID, the CID chosen by
    //# the client when first contacting the server)
    /// This maps to an internal connection id, which is a stable identifier across CID changes.
    Connection {
        id: u64,
    },
}

/// Used to disambiguate events that can occur for the local or the remote endpoint.
enum Location {
    /// The Local endpoint
    Local,
    /// The Remote endpoint
    Remote,
}

impl IntoEvent<api::Location> for crate::endpoint::Location {
    fn into_event(self) -> api::Location {
        match self {
            Self::Local => api::Location::Local {},
            Self::Remote => api::Location::Remote {},
        }
    }
}

impl IntoEvent<builder::Location> for crate::endpoint::Location {
    fn into_event(self) -> builder::Location {
        match self {
            Self::Local => builder::Location::Local {},
            Self::Remote => builder::Location::Remote {},
        }
    }
}

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
    fn into_event(self) -> api::EndpointType {
        match self {
            Self::Client => api::EndpointType::Client {},
            Self::Server => api::EndpointType::Server {},
        }
    }
}

impl IntoEvent<builder::EndpointType> for crate::endpoint::Type {
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
}

enum KeySpace {
    Initial {},
    Handshake {},
    ZeroRtt {},
    OneRtt {},
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
