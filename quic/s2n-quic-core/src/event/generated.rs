// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use super::*;
pub mod api {
    use super::*;
    pub use traits::Subscriber;
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionMeta {
        pub endpoint_type: EndpointType,
        pub id: u64,
        pub timestamp: crate::event::Timestamp,
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointMeta {
        pub endpoint_type: EndpointType,
        pub timestamp: crate::event::Timestamp,
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionInfo {}
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct PacketHeader {
        pub packet_type: PacketType,
        pub version: Option<u32>,
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct Path<'a> {
        pub local_addr: SocketAddress<'a>,
        pub local_cid: ConnectionId<'a>,
        pub remote_addr: SocketAddress<'a>,
        pub remote_cid: ConnectionId<'a>,
        pub id: u64,
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionId<'a> {
        pub bytes: &'a [u8],
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum SocketAddress<'a> {
        #[non_exhaustive]
        IpV4 { ip: &'a [u8; 4], port: u16 },
        #[non_exhaustive]
        IpV6 { ip: &'a [u8; 16], port: u16 },
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum DuplicatePacketError {
        #[non_exhaustive]
        Duplicate {},
        #[non_exhaustive]
        TooOld {},
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum Frame {
        #[non_exhaustive]
        Padding {},
        #[non_exhaustive]
        Ping {},
        #[non_exhaustive]
        Ack {},
        #[non_exhaustive]
        ResetStream {},
        #[non_exhaustive]
        StopSending {},
        #[non_exhaustive]
        Crypto { offset: u64, len: u16 },
        #[non_exhaustive]
        NewToken {},
        #[non_exhaustive]
        Stream {
            id: u64,
            offset: u64,
            len: u16,
            is_fin: bool,
        },
        #[non_exhaustive]
        MaxData {},
        #[non_exhaustive]
        MaxStreamData {},
        #[non_exhaustive]
        MaxStreams {},
        #[non_exhaustive]
        DataBlocked {},
        #[non_exhaustive]
        StreamDataBlocked {},
        #[non_exhaustive]
        StreamsBlocked {},
        #[non_exhaustive]
        NewConnectionId {},
        #[non_exhaustive]
        RetireConnectionId {},
        #[non_exhaustive]
        PathChallenge {},
        #[non_exhaustive]
        PathResponse {},
        #[non_exhaustive]
        ConnectionClose {},
        #[non_exhaustive]
        HandshakeDone {},
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum PacketType {
        #[non_exhaustive]
        Initial { number: u64 },
        #[non_exhaustive]
        Handshake { number: u64 },
        #[non_exhaustive]
        ZeroRtt { number: u64 },
        #[non_exhaustive]
        OneRtt { number: u64 },
        #[non_exhaustive]
        Retry {},
        #[non_exhaustive]
        VersionNegotiation {},
        #[non_exhaustive]
        StatelessReset {},
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum KeyType {
        #[non_exhaustive]
        Initial {},
        #[non_exhaustive]
        Handshake {},
        #[non_exhaustive]
        ZeroRtt {},
        #[non_exhaustive]
        OneRtt { generation: u16 },
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " A context from which the event is being emitted"]
    #[doc = ""]
    #[doc = " An event can occur in the context of an Endpoint or Connection"]
    pub enum Subject {
        #[non_exhaustive]
        Endpoint {},
        #[non_exhaustive]
        Connection { id: u64 },
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Used to disambiguate events that can occur for the local or the remote endpoint."]
    pub enum Location {
        #[non_exhaustive]
        Local {},
        #[non_exhaustive]
        Remote {},
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum EndpointType {
        #[non_exhaustive]
        Server {},
        #[non_exhaustive]
        Client {},
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum DropReason {
        #[non_exhaustive]
        DecodingFailed {},
        #[non_exhaustive]
        InvalidRetryToken {},
        #[non_exhaustive]
        ConnectionNotAllowed {},
        #[non_exhaustive]
        UnsupportedVersion {},
        #[non_exhaustive]
        InvalidDestinationConnectionId {},
        #[non_exhaustive]
        InvalidSourceConnectionId {},
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Application level protocol"]
    pub struct AlpnInformation<'a> {
        pub chosen_alpn: &'a [u8],
    }
    impl<'a> Event for AlpnInformation<'a> {
        const NAME: &'static str = "transport:alpn_information";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Server Name Indication"]
    pub struct SniInformation<'a> {
        pub chosen_sni: &'a str,
    }
    impl<'a> Event for SniInformation<'a> {
        const NAME: &'static str = "transport:sni_information";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Packet was sent by a connection"]
    pub struct PacketSent {
        pub packet_header: PacketHeader,
    }
    impl Event for PacketSent {
        const NAME: &'static str = "transport:packet_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Packet was received by a connection"]
    pub struct PacketReceived {
        pub packet_header: PacketHeader,
    }
    impl Event for PacketReceived {
        const NAME: &'static str = "transport:packet_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Active path was updated"]
    pub struct ActivePathUpdated<'a> {
        pub previous: Path<'a>,
        pub active: Path<'a>,
    }
    impl<'a> Event for ActivePathUpdated<'a> {
        const NAME: &'static str = "connectivity:active_path_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " A new path was created"]
    pub struct PathCreated<'a> {
        pub active: Path<'a>,
        pub new: Path<'a>,
    }
    impl<'a> Event for PathCreated<'a> {
        const NAME: &'static str = "transport:path_created";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Frame was sent"]
    pub struct FrameSent {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub frame: Frame,
    }
    impl Event for FrameSent {
        const NAME: &'static str = "transport:frame_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Frame was received"]
    pub struct FrameReceived {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub frame: Frame,
    }
    impl Event for FrameReceived {
        const NAME: &'static str = "transport:frame_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Packet was lost"]
    pub struct PacketLost<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub bytes_lost: u16,
        pub is_mtu_probe: bool,
    }
    impl<'a> Event for PacketLost<'a> {
        const NAME: &'static str = "recovery:packet_lost";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Recovery metrics updated"]
    pub struct RecoveryMetrics {
        pub path_id: u64,
        pub min_rtt: Duration,
        pub smoothed_rtt: Duration,
        pub latest_rtt: Duration,
        pub rtt_variance: Duration,
        pub max_ack_delay: Duration,
        pub pto_count: u32,
        pub congestion_window: u32,
        pub bytes_in_flight: u32,
    }
    impl Event for RecoveryMetrics {
        const NAME: &'static str = "recovery:metrics_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Crypto key updated"]
    pub struct KeyUpdate {
        pub key_type: KeyType,
    }
    impl Event for KeyUpdate {
        const NAME: &'static str = "security:key_update";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Connection started"]
    pub struct ConnectionStarted<'a> {
        pub path: Path<'a>,
    }
    impl<'a> Event for ConnectionStarted<'a> {
        const NAME: &'static str = "connectivity:connection_started";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Connection closed"]
    pub struct ConnectionClosed {
        pub error: crate::connection::Error,
    }
    impl Event for ConnectionClosed {
        const NAME: &'static str = "connectivity:connection_closed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Duplicate packet received"]
    pub struct DuplicatePacket {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub error: DuplicatePacketError,
    }
    impl Event for DuplicatePacket {
        const NAME: &'static str = "transport:duplicate_packet";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram sent by a connection"]
    pub struct DatagramSent {
        pub len: u16,
    }
    impl Event for DatagramSent {
        const NAME: &'static str = "transport:datagram_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram received by a connection"]
    pub struct DatagramReceived {
        pub len: u16,
    }
    impl Event for DatagramReceived {
        const NAME: &'static str = "transport:datagram_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram dropped by a connection"]
    pub struct DatagramDropped {
        pub len: u16,
        pub reason: DropReason,
    }
    impl Event for DatagramDropped {
        const NAME: &'static str = "transport:datagram_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " ConnectionId updated"]
    pub struct ConnectionIdUpdated<'a> {
        pub path_id: u64,
        pub cid_consumer: Location,
        pub previous: ConnectionId<'a>,
        pub current: ConnectionId<'a>,
    }
    impl<'a> Event for ConnectionIdUpdated<'a> {
        const NAME: &'static str = "connectivity:connection_id_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " QUIC version"]
    pub struct VersionInformation<'a> {
        pub server_versions: &'a [u32],
        pub client_versions: &'a [u32],
        pub chosen_version: Option<u32>,
    }
    impl<'a> Event for VersionInformation<'a> {
        const NAME: &'static str = "transport::version_information";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Packet was sent by the endpoint"]
    pub struct EndpointPacketSent {
        pub packet_header: PacketHeader,
    }
    impl Event for EndpointPacketSent {
        const NAME: &'static str = "transport:packet_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Packet was received by the endpoint"]
    pub struct EndpointPacketReceived {
        pub packet_header: PacketHeader,
    }
    impl Event for EndpointPacketReceived {
        const NAME: &'static str = "transport:packet_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram sent by the endpoint"]
    pub struct EndpointDatagramSent {
        pub len: u16,
    }
    impl Event for EndpointDatagramSent {
        const NAME: &'static str = "transport:datagram_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram received by the endpoint"]
    pub struct EndpointDatagramReceived {
        pub len: u16,
    }
    impl Event for EndpointDatagramReceived {
        const NAME: &'static str = "transport:datagram_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram dropped by the endpoint"]
    pub struct EndpointDatagramDropped {
        pub len: u16,
        pub reason: DropReason,
    }
    impl Event for EndpointDatagramDropped {
        const NAME: &'static str = "transport:datagram_dropped";
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
    impl IntoEvent<builder::DuplicatePacketError> for crate::packet::number::SlidingWindowError {
        fn into_event(self) -> builder::DuplicatePacketError {
            use crate::packet::number::SlidingWindowError;
            match self {
                SlidingWindowError::TooOld => builder::DuplicatePacketError::TooOld {},
                SlidingWindowError::Duplicate => builder::DuplicatePacketError::Duplicate {},
            }
        }
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
            builder::Frame::MaxStreams {}
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
            builder::Frame::StreamsBlocked {}
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
    impl IntoEvent<builder::PacketType> for crate::packet::number::PacketNumber {
        fn into_event(self) -> builder::PacketType {
            use crate::packet::number::PacketNumberSpace;
            use builder::PacketType;
            match self.space() {
                PacketNumberSpace::Initial => PacketType::Initial {
                    number: self.as_u64(),
                },
                PacketNumberSpace::Handshake => PacketType::Handshake {
                    number: self.as_u64(),
                },
                PacketNumberSpace::ApplicationData => PacketType::OneRtt {
                    number: self.as_u64(),
                },
            }
        }
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
}
pub mod builder {
    use super::*;
    #[derive(Clone, Debug)]
    pub struct ConnectionMeta {
        pub endpoint_type: crate::endpoint::Type,
        pub id: u64,
        pub timestamp: crate::time::Timestamp,
    }
    impl IntoEvent<api::ConnectionMeta> for ConnectionMeta {
        #[inline]
        fn into_event(self) -> api::ConnectionMeta {
            let ConnectionMeta {
                endpoint_type,
                id,
                timestamp,
            } = self;
            api::ConnectionMeta {
                endpoint_type: endpoint_type.into_event(),
                id: id.into_event(),
                timestamp: timestamp.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointMeta {
        pub endpoint_type: crate::endpoint::Type,
        pub timestamp: crate::time::Timestamp,
    }
    impl IntoEvent<api::EndpointMeta> for EndpointMeta {
        #[inline]
        fn into_event(self) -> api::EndpointMeta {
            let EndpointMeta {
                endpoint_type,
                timestamp,
            } = self;
            api::EndpointMeta {
                endpoint_type: endpoint_type.into_event(),
                timestamp: timestamp.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ConnectionInfo {}
    impl IntoEvent<api::ConnectionInfo> for ConnectionInfo {
        #[inline]
        fn into_event(self) -> api::ConnectionInfo {
            let ConnectionInfo {} = self;
            api::ConnectionInfo {}
        }
    }
    #[derive(Clone, Debug)]
    pub struct PacketHeader {
        pub packet_type: PacketType,
        pub version: Option<u32>,
    }
    impl IntoEvent<api::PacketHeader> for PacketHeader {
        #[inline]
        fn into_event(self) -> api::PacketHeader {
            let PacketHeader {
                packet_type,
                version,
            } = self;
            api::PacketHeader {
                packet_type: packet_type.into_event(),
                version: version.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Path<'a> {
        pub local_addr: SocketAddress<'a>,
        pub local_cid: ConnectionId<'a>,
        pub remote_addr: SocketAddress<'a>,
        pub remote_cid: ConnectionId<'a>,
        pub id: u64,
    }
    impl<'a> IntoEvent<api::Path<'a>> for Path<'a> {
        #[inline]
        fn into_event(self) -> api::Path<'a> {
            let Path {
                local_addr,
                local_cid,
                remote_addr,
                remote_cid,
                id,
            } = self;
            api::Path {
                local_addr: local_addr.into_event(),
                local_cid: local_cid.into_event(),
                remote_addr: remote_addr.into_event(),
                remote_cid: remote_cid.into_event(),
                id: id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ConnectionId<'a> {
        pub bytes: &'a [u8],
    }
    impl<'a> IntoEvent<api::ConnectionId<'a>> for ConnectionId<'a> {
        #[inline]
        fn into_event(self) -> api::ConnectionId<'a> {
            let ConnectionId { bytes } = self;
            api::ConnectionId {
                bytes: bytes.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum SocketAddress<'a> {
        IpV4 { ip: &'a [u8; 4], port: u16 },
        IpV6 { ip: &'a [u8; 16], port: u16 },
    }
    impl<'a> IntoEvent<api::SocketAddress<'a>> for SocketAddress<'a> {
        #[inline]
        fn into_event(self) -> api::SocketAddress<'a> {
            use api::SocketAddress::*;
            match self {
                Self::IpV4 { ip, port } => IpV4 {
                    ip: ip.into_event(),
                    port: port.into_event(),
                },
                Self::IpV6 { ip, port } => IpV6 {
                    ip: ip.into_event(),
                    port: port.into_event(),
                },
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum DuplicatePacketError {
        Duplicate,
        TooOld,
    }
    impl IntoEvent<api::DuplicatePacketError> for DuplicatePacketError {
        #[inline]
        fn into_event(self) -> api::DuplicatePacketError {
            use api::DuplicatePacketError::*;
            match self {
                Self::Duplicate => Duplicate {},
                Self::TooOld => TooOld {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum Frame {
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
        MaxStreams,
        DataBlocked,
        StreamDataBlocked,
        StreamsBlocked,
        NewConnectionId,
        RetireConnectionId,
        PathChallenge,
        PathResponse,
        ConnectionClose,
        HandshakeDone,
    }
    impl IntoEvent<api::Frame> for Frame {
        #[inline]
        fn into_event(self) -> api::Frame {
            use api::Frame::*;
            match self {
                Self::Padding => Padding {},
                Self::Ping => Ping {},
                Self::Ack => Ack {},
                Self::ResetStream => ResetStream {},
                Self::StopSending => StopSending {},
                Self::Crypto { offset, len } => Crypto {
                    offset: offset.into_event(),
                    len: len.into_event(),
                },
                Self::NewToken => NewToken {},
                Self::Stream {
                    id,
                    offset,
                    len,
                    is_fin,
                } => Stream {
                    id: id.into_event(),
                    offset: offset.into_event(),
                    len: len.into_event(),
                    is_fin: is_fin.into_event(),
                },
                Self::MaxData => MaxData {},
                Self::MaxStreamData => MaxStreamData {},
                Self::MaxStreams => MaxStreams {},
                Self::DataBlocked => DataBlocked {},
                Self::StreamDataBlocked => StreamDataBlocked {},
                Self::StreamsBlocked => StreamsBlocked {},
                Self::NewConnectionId => NewConnectionId {},
                Self::RetireConnectionId => RetireConnectionId {},
                Self::PathChallenge => PathChallenge {},
                Self::PathResponse => PathResponse {},
                Self::ConnectionClose => ConnectionClose {},
                Self::HandshakeDone => HandshakeDone {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum PacketType {
        Initial { number: u64 },
        Handshake { number: u64 },
        ZeroRtt { number: u64 },
        OneRtt { number: u64 },
        Retry,
        VersionNegotiation,
        StatelessReset,
    }
    impl IntoEvent<api::PacketType> for PacketType {
        #[inline]
        fn into_event(self) -> api::PacketType {
            use api::PacketType::*;
            match self {
                Self::Initial { number } => Initial {
                    number: number.into_event(),
                },
                Self::Handshake { number } => Handshake {
                    number: number.into_event(),
                },
                Self::ZeroRtt { number } => ZeroRtt {
                    number: number.into_event(),
                },
                Self::OneRtt { number } => OneRtt {
                    number: number.into_event(),
                },
                Self::Retry => Retry {},
                Self::VersionNegotiation => VersionNegotiation {},
                Self::StatelessReset => StatelessReset {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum KeyType {
        Initial,
        Handshake,
        ZeroRtt,
        OneRtt { generation: u16 },
    }
    impl IntoEvent<api::KeyType> for KeyType {
        #[inline]
        fn into_event(self) -> api::KeyType {
            use api::KeyType::*;
            match self {
                Self::Initial => Initial {},
                Self::Handshake => Handshake {},
                Self::ZeroRtt => ZeroRtt {},
                Self::OneRtt { generation } => OneRtt {
                    generation: generation.into_event(),
                },
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " A context from which the event is being emitted"]
    #[doc = ""]
    #[doc = " An event can occur in the context of an Endpoint or Connection"]
    pub enum Subject {
        Endpoint,
        Connection { id: u64 },
    }
    impl IntoEvent<api::Subject> for Subject {
        #[inline]
        fn into_event(self) -> api::Subject {
            use api::Subject::*;
            match self {
                Self::Endpoint => Endpoint {},
                Self::Connection { id } => Connection {
                    id: id.into_event(),
                },
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Used to disambiguate events that can occur for the local or the remote endpoint."]
    pub enum Location {
        Local,
        Remote,
    }
    impl IntoEvent<api::Location> for Location {
        #[inline]
        fn into_event(self) -> api::Location {
            use api::Location::*;
            match self {
                Self::Local => Local {},
                Self::Remote => Remote {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum EndpointType {
        Server,
        Client,
    }
    impl IntoEvent<api::EndpointType> for EndpointType {
        #[inline]
        fn into_event(self) -> api::EndpointType {
            use api::EndpointType::*;
            match self {
                Self::Server => Server {},
                Self::Client => Client {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum DropReason {
        DecodingFailed,
        InvalidRetryToken,
        ConnectionNotAllowed,
        UnsupportedVersion,
        InvalidDestinationConnectionId,
        InvalidSourceConnectionId,
    }
    impl IntoEvent<api::DropReason> for DropReason {
        #[inline]
        fn into_event(self) -> api::DropReason {
            use api::DropReason::*;
            match self {
                Self::DecodingFailed => DecodingFailed {},
                Self::InvalidRetryToken => InvalidRetryToken {},
                Self::ConnectionNotAllowed => ConnectionNotAllowed {},
                Self::UnsupportedVersion => UnsupportedVersion {},
                Self::InvalidDestinationConnectionId => InvalidDestinationConnectionId {},
                Self::InvalidSourceConnectionId => InvalidSourceConnectionId {},
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Application level protocol"]
    pub struct AlpnInformation<'a> {
        pub chosen_alpn: &'a [u8],
    }
    impl<'a> IntoEvent<api::AlpnInformation<'a>> for AlpnInformation<'a> {
        #[inline]
        fn into_event(self) -> api::AlpnInformation<'a> {
            let AlpnInformation { chosen_alpn } = self;
            api::AlpnInformation {
                chosen_alpn: chosen_alpn.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Server Name Indication"]
    pub struct SniInformation<'a> {
        pub chosen_sni: &'a str,
    }
    impl<'a> IntoEvent<api::SniInformation<'a>> for SniInformation<'a> {
        #[inline]
        fn into_event(self) -> api::SniInformation<'a> {
            let SniInformation { chosen_sni } = self;
            api::SniInformation {
                chosen_sni: chosen_sni.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Packet was sent by a connection"]
    pub struct PacketSent {
        pub packet_header: PacketHeader,
    }
    impl IntoEvent<api::PacketSent> for PacketSent {
        #[inline]
        fn into_event(self) -> api::PacketSent {
            let PacketSent { packet_header } = self;
            api::PacketSent {
                packet_header: packet_header.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Packet was received by a connection"]
    pub struct PacketReceived {
        pub packet_header: PacketHeader,
    }
    impl IntoEvent<api::PacketReceived> for PacketReceived {
        #[inline]
        fn into_event(self) -> api::PacketReceived {
            let PacketReceived { packet_header } = self;
            api::PacketReceived {
                packet_header: packet_header.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Active path was updated"]
    pub struct ActivePathUpdated<'a> {
        pub previous: Path<'a>,
        pub active: Path<'a>,
    }
    impl<'a> IntoEvent<api::ActivePathUpdated<'a>> for ActivePathUpdated<'a> {
        #[inline]
        fn into_event(self) -> api::ActivePathUpdated<'a> {
            let ActivePathUpdated { previous, active } = self;
            api::ActivePathUpdated {
                previous: previous.into_event(),
                active: active.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " A new path was created"]
    pub struct PathCreated<'a> {
        pub active: Path<'a>,
        pub new: Path<'a>,
    }
    impl<'a> IntoEvent<api::PathCreated<'a>> for PathCreated<'a> {
        #[inline]
        fn into_event(self) -> api::PathCreated<'a> {
            let PathCreated { active, new } = self;
            api::PathCreated {
                active: active.into_event(),
                new: new.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Frame was sent"]
    pub struct FrameSent {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub frame: Frame,
    }
    impl IntoEvent<api::FrameSent> for FrameSent {
        #[inline]
        fn into_event(self) -> api::FrameSent {
            let FrameSent {
                packet_header,
                path_id,
                frame,
            } = self;
            api::FrameSent {
                packet_header: packet_header.into_event(),
                path_id: path_id.into_event(),
                frame: frame.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Frame was received"]
    pub struct FrameReceived {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub frame: Frame,
    }
    impl IntoEvent<api::FrameReceived> for FrameReceived {
        #[inline]
        fn into_event(self) -> api::FrameReceived {
            let FrameReceived {
                packet_header,
                path_id,
                frame,
            } = self;
            api::FrameReceived {
                packet_header: packet_header.into_event(),
                path_id: path_id.into_event(),
                frame: frame.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Packet was lost"]
    pub struct PacketLost<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub bytes_lost: u16,
        pub is_mtu_probe: bool,
    }
    impl<'a> IntoEvent<api::PacketLost<'a>> for PacketLost<'a> {
        #[inline]
        fn into_event(self) -> api::PacketLost<'a> {
            let PacketLost {
                packet_header,
                path,
                bytes_lost,
                is_mtu_probe,
            } = self;
            api::PacketLost {
                packet_header: packet_header.into_event(),
                path: path.into_event(),
                bytes_lost: bytes_lost.into_event(),
                is_mtu_probe: is_mtu_probe.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Recovery metrics updated"]
    pub struct RecoveryMetrics {
        pub path_id: u64,
        pub min_rtt: Duration,
        pub smoothed_rtt: Duration,
        pub latest_rtt: Duration,
        pub rtt_variance: Duration,
        pub max_ack_delay: Duration,
        pub pto_count: u32,
        pub congestion_window: u32,
        pub bytes_in_flight: u32,
    }
    impl IntoEvent<api::RecoveryMetrics> for RecoveryMetrics {
        #[inline]
        fn into_event(self) -> api::RecoveryMetrics {
            let RecoveryMetrics {
                path_id,
                min_rtt,
                smoothed_rtt,
                latest_rtt,
                rtt_variance,
                max_ack_delay,
                pto_count,
                congestion_window,
                bytes_in_flight,
            } = self;
            api::RecoveryMetrics {
                path_id: path_id.into_event(),
                min_rtt: min_rtt.into_event(),
                smoothed_rtt: smoothed_rtt.into_event(),
                latest_rtt: latest_rtt.into_event(),
                rtt_variance: rtt_variance.into_event(),
                max_ack_delay: max_ack_delay.into_event(),
                pto_count: pto_count.into_event(),
                congestion_window: congestion_window.into_event(),
                bytes_in_flight: bytes_in_flight.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Crypto key updated"]
    pub struct KeyUpdate {
        pub key_type: KeyType,
    }
    impl IntoEvent<api::KeyUpdate> for KeyUpdate {
        #[inline]
        fn into_event(self) -> api::KeyUpdate {
            let KeyUpdate { key_type } = self;
            api::KeyUpdate {
                key_type: key_type.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Connection started"]
    pub struct ConnectionStarted<'a> {
        pub path: Path<'a>,
    }
    impl<'a> IntoEvent<api::ConnectionStarted<'a>> for ConnectionStarted<'a> {
        #[inline]
        fn into_event(self) -> api::ConnectionStarted<'a> {
            let ConnectionStarted { path } = self;
            api::ConnectionStarted {
                path: path.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Connection closed"]
    pub struct ConnectionClosed {
        pub error: crate::connection::Error,
    }
    impl IntoEvent<api::ConnectionClosed> for ConnectionClosed {
        #[inline]
        fn into_event(self) -> api::ConnectionClosed {
            let ConnectionClosed { error } = self;
            api::ConnectionClosed {
                error: error.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Duplicate packet received"]
    pub struct DuplicatePacket {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub error: DuplicatePacketError,
    }
    impl IntoEvent<api::DuplicatePacket> for DuplicatePacket {
        #[inline]
        fn into_event(self) -> api::DuplicatePacket {
            let DuplicatePacket {
                packet_header,
                path_id,
                error,
            } = self;
            api::DuplicatePacket {
                packet_header: packet_header.into_event(),
                path_id: path_id.into_event(),
                error: error.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Datagram sent by a connection"]
    pub struct DatagramSent {
        pub len: u16,
    }
    impl IntoEvent<api::DatagramSent> for DatagramSent {
        #[inline]
        fn into_event(self) -> api::DatagramSent {
            let DatagramSent { len } = self;
            api::DatagramSent {
                len: len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Datagram received by a connection"]
    pub struct DatagramReceived {
        pub len: u16,
    }
    impl IntoEvent<api::DatagramReceived> for DatagramReceived {
        #[inline]
        fn into_event(self) -> api::DatagramReceived {
            let DatagramReceived { len } = self;
            api::DatagramReceived {
                len: len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Datagram dropped by a connection"]
    pub struct DatagramDropped {
        pub len: u16,
        pub reason: DropReason,
    }
    impl IntoEvent<api::DatagramDropped> for DatagramDropped {
        #[inline]
        fn into_event(self) -> api::DatagramDropped {
            let DatagramDropped { len, reason } = self;
            api::DatagramDropped {
                len: len.into_event(),
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " ConnectionId updated"]
    pub struct ConnectionIdUpdated<'a> {
        pub path_id: u64,
        pub cid_consumer: crate::endpoint::Location,
        pub previous: ConnectionId<'a>,
        pub current: ConnectionId<'a>,
    }
    impl<'a> IntoEvent<api::ConnectionIdUpdated<'a>> for ConnectionIdUpdated<'a> {
        #[inline]
        fn into_event(self) -> api::ConnectionIdUpdated<'a> {
            let ConnectionIdUpdated {
                path_id,
                cid_consumer,
                previous,
                current,
            } = self;
            api::ConnectionIdUpdated {
                path_id: path_id.into_event(),
                cid_consumer: cid_consumer.into_event(),
                previous: previous.into_event(),
                current: current.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " QUIC version"]
    pub struct VersionInformation<'a> {
        pub server_versions: &'a [u32],
        pub client_versions: &'a [u32],
        pub chosen_version: Option<u32>,
    }
    impl<'a> IntoEvent<api::VersionInformation<'a>> for VersionInformation<'a> {
        #[inline]
        fn into_event(self) -> api::VersionInformation<'a> {
            let VersionInformation {
                server_versions,
                client_versions,
                chosen_version,
            } = self;
            api::VersionInformation {
                server_versions: server_versions.into_event(),
                client_versions: client_versions.into_event(),
                chosen_version: chosen_version.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Packet was sent by the endpoint"]
    pub struct EndpointPacketSent {
        pub packet_header: PacketHeader,
    }
    impl IntoEvent<api::EndpointPacketSent> for EndpointPacketSent {
        #[inline]
        fn into_event(self) -> api::EndpointPacketSent {
            let EndpointPacketSent { packet_header } = self;
            api::EndpointPacketSent {
                packet_header: packet_header.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Packet was received by the endpoint"]
    pub struct EndpointPacketReceived {
        pub packet_header: PacketHeader,
    }
    impl IntoEvent<api::EndpointPacketReceived> for EndpointPacketReceived {
        #[inline]
        fn into_event(self) -> api::EndpointPacketReceived {
            let EndpointPacketReceived { packet_header } = self;
            api::EndpointPacketReceived {
                packet_header: packet_header.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Datagram sent by the endpoint"]
    pub struct EndpointDatagramSent {
        pub len: u16,
    }
    impl IntoEvent<api::EndpointDatagramSent> for EndpointDatagramSent {
        #[inline]
        fn into_event(self) -> api::EndpointDatagramSent {
            let EndpointDatagramSent { len } = self;
            api::EndpointDatagramSent {
                len: len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Datagram received by the endpoint"]
    pub struct EndpointDatagramReceived {
        pub len: u16,
    }
    impl IntoEvent<api::EndpointDatagramReceived> for EndpointDatagramReceived {
        #[inline]
        fn into_event(self) -> api::EndpointDatagramReceived {
            let EndpointDatagramReceived { len } = self;
            api::EndpointDatagramReceived {
                len: len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Datagram dropped by the endpoint"]
    pub struct EndpointDatagramDropped {
        pub len: u16,
        pub reason: DropReason,
    }
    impl IntoEvent<api::EndpointDatagramDropped> for EndpointDatagramDropped {
        #[inline]
        fn into_event(self) -> api::EndpointDatagramDropped {
            let EndpointDatagramDropped { len, reason } = self;
            api::EndpointDatagramDropped {
                len: len.into_event(),
                reason: reason.into_event(),
            }
        }
    }
}
pub use traits::*;
mod traits {
    use super::*;
    use api::*;
    use core::fmt;
    pub trait Meta {
        fn endpoint_type(&self) -> &EndpointType;
        fn subject(&self) -> Subject;
        fn timestamp(&self) -> &crate::event::Timestamp;
    }
    impl Meta for ConnectionMeta {
        fn endpoint_type(&self) -> &EndpointType {
            &self.endpoint_type
        }
        fn subject(&self) -> Subject {
            Subject::Connection { id: self.id }
        }
        fn timestamp(&self) -> &crate::event::Timestamp {
            &self.timestamp
        }
    }
    impl Meta for EndpointMeta {
        fn endpoint_type(&self) -> &EndpointType {
            &self.endpoint_type
        }
        fn subject(&self) -> Subject {
            Subject::Endpoint {}
        }
        fn timestamp(&self) -> &crate::event::Timestamp {
            &self.timestamp
        }
    }
    pub trait Subscriber: 'static + Send {
        #[doc = r" An application provided type associated with each connection."]
        #[doc = r""]
        #[doc = r" The context provides a mechanism for applications to provide a custom type"]
        #[doc = r" and update it on each event, e.g. computing statictics. Each event"]
        #[doc = r" invocation (e.g. [`Subscriber::on_packet_sent`]) also provides mutable"]
        #[doc = r" access to the context `&mut ConnectionContext` and allows for updating the"]
        #[doc = r" context."]
        #[doc = r""]
        #[doc = r" ```no_run"]
        #[doc = r" # mod s2n_quic { pub mod provider { pub mod event {"]
        #[doc = r" #     pub use s2n_quic_core::event::{api as events, api::ConnectionInfo, api::ConnectionMeta, Subscriber};"]
        #[doc = r" # }}}"]
        #[doc = r" use s2n_quic::provider::event::{"]
        #[doc = r"     ConnectionInfo, ConnectionMeta, Subscriber, events::PacketSent"]
        #[doc = r" };"]
        #[doc = r""]
        #[doc = r" pub struct MyEventSubscriber;"]
        #[doc = r""]
        #[doc = r" pub struct MyEventContext {"]
        #[doc = r"     packet_sent: u64,"]
        #[doc = r" }"]
        #[doc = r""]
        #[doc = r" impl Subscriber for MyEventSubscriber {"]
        #[doc = r"     type ConnectionContext = MyEventContext;"]
        #[doc = r""]
        #[doc = r"     fn create_connection_context("]
        #[doc = r"         &mut self, _meta: &ConnectionMeta,"]
        #[doc = r"         _info: &ConnectionInfo,"]
        #[doc = r"     ) -> Self::ConnectionContext {"]
        #[doc = r"         MyEventContext { packet_sent: 0 }"]
        #[doc = r"     }"]
        #[doc = r""]
        #[doc = r"     fn on_packet_sent("]
        #[doc = r"         &mut self,"]
        #[doc = r"         context: &mut Self::ConnectionContext,"]
        #[doc = r"         _meta: &ConnectionMeta,"]
        #[doc = r"         _event: &PacketSent,"]
        #[doc = r"     ) {"]
        #[doc = r"         context.packet_sent += 1;"]
        #[doc = r"     }"]
        #[doc = r" }"]
        #[doc = r"  ```"]
        type ConnectionContext: 'static + Send;
        #[doc = r" Creates a context to be passed to each connection-related event"]
        fn create_connection_context(
            &mut self,
            meta: &ConnectionMeta,
            info: &ConnectionInfo,
        ) -> Self::ConnectionContext;
        #[doc = "Called when the `AlpnInformation` event is triggered"]
        #[inline]
        fn on_alpn_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &AlpnInformation,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `SniInformation` event is triggered"]
        #[inline]
        fn on_sni_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &SniInformation,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PacketSent` event is triggered"]
        #[inline]
        fn on_packet_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &PacketSent,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PacketReceived` event is triggered"]
        #[inline]
        fn on_packet_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &PacketReceived,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ActivePathUpdated` event is triggered"]
        #[inline]
        fn on_active_path_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &ActivePathUpdated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathCreated` event is triggered"]
        #[inline]
        fn on_path_created(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &PathCreated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `FrameSent` event is triggered"]
        #[inline]
        fn on_frame_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &FrameSent,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `FrameReceived` event is triggered"]
        #[inline]
        fn on_frame_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &FrameReceived,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PacketLost` event is triggered"]
        #[inline]
        fn on_packet_lost(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &PacketLost,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `RecoveryMetrics` event is triggered"]
        #[inline]
        fn on_recovery_metrics(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &RecoveryMetrics,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `KeyUpdate` event is triggered"]
        #[inline]
        fn on_key_update(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &KeyUpdate,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ConnectionStarted` event is triggered"]
        #[inline]
        fn on_connection_started(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &ConnectionStarted,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ConnectionClosed` event is triggered"]
        #[inline]
        fn on_connection_closed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &ConnectionClosed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `DuplicatePacket` event is triggered"]
        #[inline]
        fn on_duplicate_packet(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &DuplicatePacket,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `DatagramSent` event is triggered"]
        #[inline]
        fn on_datagram_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &DatagramSent,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `DatagramReceived` event is triggered"]
        #[inline]
        fn on_datagram_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &DatagramReceived,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `DatagramDropped` event is triggered"]
        #[inline]
        fn on_datagram_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &DatagramDropped,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ConnectionIdUpdated` event is triggered"]
        #[inline]
        fn on_connection_id_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &ConnectionIdUpdated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `VersionInformation` event is triggered"]
        #[inline]
        fn on_version_information(&mut self, meta: &EndpointMeta, event: &VersionInformation) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointPacketSent` event is triggered"]
        #[inline]
        fn on_endpoint_packet_sent(&mut self, meta: &EndpointMeta, event: &EndpointPacketSent) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointPacketReceived` event is triggered"]
        #[inline]
        fn on_endpoint_packet_received(
            &mut self,
            meta: &EndpointMeta,
            event: &EndpointPacketReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointDatagramSent` event is triggered"]
        #[inline]
        fn on_endpoint_datagram_sent(&mut self, meta: &EndpointMeta, event: &EndpointDatagramSent) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointDatagramReceived` event is triggered"]
        #[inline]
        fn on_endpoint_datagram_received(
            &mut self,
            meta: &EndpointMeta,
            event: &EndpointDatagramReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointDatagramDropped` event is triggered"]
        #[inline]
        fn on_endpoint_datagram_dropped(
            &mut self,
            meta: &EndpointMeta,
            event: &EndpointDatagramDropped,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Called for each event that relates to the endpoint and all connections"]
        #[inline]
        fn on_event<M: Meta, E: Event>(&mut self, meta: &M, event: &E) {
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Called for each event that relates to a connection"]
        #[inline]
        fn on_connection_event<E: Event>(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &E,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[inline]
        fn query(
            context: &Self::ConnectionContext,
            query: &mut dyn query::Query,
        ) -> query::ControlFlow {
            query.execute(context)
        }
        #[inline]
        fn query_mut(
            context: &mut Self::ConnectionContext,
            query: &mut dyn query::QueryMut,
        ) -> query::ControlFlow {
            query.execute_mut(context)
        }
    }
    #[doc = r" Subscriber is implemented for a 2-element tuple to make it easy to compose multiple"]
    #[doc = r" subscribers."]
    impl<A, B> Subscriber for (A, B)
    where
        A: Subscriber,
        B: Subscriber,
    {
        type ConnectionContext = (A::ConnectionContext, B::ConnectionContext);
        #[inline]
        fn create_connection_context(
            &mut self,
            meta: &ConnectionMeta,
            info: &ConnectionInfo,
        ) -> Self::ConnectionContext {
            (
                self.0.create_connection_context(meta, info),
                self.1.create_connection_context(meta, info),
            )
        }
        #[inline]
        fn on_alpn_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &AlpnInformation,
        ) {
            (self.0).on_alpn_information(&mut context.0, meta, event);
            (self.1).on_alpn_information(&mut context.1, meta, event);
        }
        #[inline]
        fn on_sni_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &SniInformation,
        ) {
            (self.0).on_sni_information(&mut context.0, meta, event);
            (self.1).on_sni_information(&mut context.1, meta, event);
        }
        #[inline]
        fn on_packet_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &PacketSent,
        ) {
            (self.0).on_packet_sent(&mut context.0, meta, event);
            (self.1).on_packet_sent(&mut context.1, meta, event);
        }
        #[inline]
        fn on_packet_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &PacketReceived,
        ) {
            (self.0).on_packet_received(&mut context.0, meta, event);
            (self.1).on_packet_received(&mut context.1, meta, event);
        }
        #[inline]
        fn on_active_path_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &ActivePathUpdated,
        ) {
            (self.0).on_active_path_updated(&mut context.0, meta, event);
            (self.1).on_active_path_updated(&mut context.1, meta, event);
        }
        #[inline]
        fn on_path_created(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &PathCreated,
        ) {
            (self.0).on_path_created(&mut context.0, meta, event);
            (self.1).on_path_created(&mut context.1, meta, event);
        }
        #[inline]
        fn on_frame_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &FrameSent,
        ) {
            (self.0).on_frame_sent(&mut context.0, meta, event);
            (self.1).on_frame_sent(&mut context.1, meta, event);
        }
        #[inline]
        fn on_frame_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &FrameReceived,
        ) {
            (self.0).on_frame_received(&mut context.0, meta, event);
            (self.1).on_frame_received(&mut context.1, meta, event);
        }
        #[inline]
        fn on_packet_lost(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &PacketLost,
        ) {
            (self.0).on_packet_lost(&mut context.0, meta, event);
            (self.1).on_packet_lost(&mut context.1, meta, event);
        }
        #[inline]
        fn on_recovery_metrics(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &RecoveryMetrics,
        ) {
            (self.0).on_recovery_metrics(&mut context.0, meta, event);
            (self.1).on_recovery_metrics(&mut context.1, meta, event);
        }
        #[inline]
        fn on_key_update(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &KeyUpdate,
        ) {
            (self.0).on_key_update(&mut context.0, meta, event);
            (self.1).on_key_update(&mut context.1, meta, event);
        }
        #[inline]
        fn on_connection_started(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &ConnectionStarted,
        ) {
            (self.0).on_connection_started(&mut context.0, meta, event);
            (self.1).on_connection_started(&mut context.1, meta, event);
        }
        #[inline]
        fn on_connection_closed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &ConnectionClosed,
        ) {
            (self.0).on_connection_closed(&mut context.0, meta, event);
            (self.1).on_connection_closed(&mut context.1, meta, event);
        }
        #[inline]
        fn on_duplicate_packet(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &DuplicatePacket,
        ) {
            (self.0).on_duplicate_packet(&mut context.0, meta, event);
            (self.1).on_duplicate_packet(&mut context.1, meta, event);
        }
        #[inline]
        fn on_datagram_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &DatagramSent,
        ) {
            (self.0).on_datagram_sent(&mut context.0, meta, event);
            (self.1).on_datagram_sent(&mut context.1, meta, event);
        }
        #[inline]
        fn on_datagram_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &DatagramReceived,
        ) {
            (self.0).on_datagram_received(&mut context.0, meta, event);
            (self.1).on_datagram_received(&mut context.1, meta, event);
        }
        #[inline]
        fn on_datagram_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &DatagramDropped,
        ) {
            (self.0).on_datagram_dropped(&mut context.0, meta, event);
            (self.1).on_datagram_dropped(&mut context.1, meta, event);
        }
        #[inline]
        fn on_connection_id_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &ConnectionIdUpdated,
        ) {
            (self.0).on_connection_id_updated(&mut context.0, meta, event);
            (self.1).on_connection_id_updated(&mut context.1, meta, event);
        }
        #[inline]
        fn on_version_information(&mut self, meta: &EndpointMeta, event: &VersionInformation) {
            (self.0).on_version_information(meta, event);
            (self.1).on_version_information(meta, event);
        }
        #[inline]
        fn on_endpoint_packet_sent(&mut self, meta: &EndpointMeta, event: &EndpointPacketSent) {
            (self.0).on_endpoint_packet_sent(meta, event);
            (self.1).on_endpoint_packet_sent(meta, event);
        }
        #[inline]
        fn on_endpoint_packet_received(
            &mut self,
            meta: &EndpointMeta,
            event: &EndpointPacketReceived,
        ) {
            (self.0).on_endpoint_packet_received(meta, event);
            (self.1).on_endpoint_packet_received(meta, event);
        }
        #[inline]
        fn on_endpoint_datagram_sent(&mut self, meta: &EndpointMeta, event: &EndpointDatagramSent) {
            (self.0).on_endpoint_datagram_sent(meta, event);
            (self.1).on_endpoint_datagram_sent(meta, event);
        }
        #[inline]
        fn on_endpoint_datagram_received(
            &mut self,
            meta: &EndpointMeta,
            event: &EndpointDatagramReceived,
        ) {
            (self.0).on_endpoint_datagram_received(meta, event);
            (self.1).on_endpoint_datagram_received(meta, event);
        }
        #[inline]
        fn on_endpoint_datagram_dropped(
            &mut self,
            meta: &EndpointMeta,
            event: &EndpointDatagramDropped,
        ) {
            (self.0).on_endpoint_datagram_dropped(meta, event);
            (self.1).on_endpoint_datagram_dropped(meta, event);
        }
        #[inline]
        fn on_event<M: Meta, E: Event>(&mut self, meta: &M, event: &E) {
            self.0.on_event(meta, event);
            self.1.on_event(meta, event);
        }
        #[inline]
        fn on_connection_event<E: Event>(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &E,
        ) {
            self.0.on_connection_event(&mut context.0, meta, event);
            self.1.on_connection_event(&mut context.1, meta, event);
        }
        #[inline]
        fn query(
            context: &Self::ConnectionContext,
            query: &mut dyn query::Query,
        ) -> query::ControlFlow {
            query
                .execute(context)
                .and_then(|| A::query(&context.0, query))
                .and_then(|| B::query(&context.1, query))
        }
        #[inline]
        fn query_mut(
            context: &mut Self::ConnectionContext,
            query: &mut dyn query::QueryMut,
        ) -> query::ControlFlow {
            query
                .execute_mut(context)
                .and_then(|| A::query_mut(&mut context.0, query))
                .and_then(|| B::query_mut(&mut context.1, query))
        }
    }
    pub trait EndpointPublisher {
        #[doc = "Publishes a `VersionInformation` event to the publisher's subscriber"]
        fn on_version_information(&mut self, event: builder::VersionInformation);
        #[doc = "Publishes a `EndpointPacketSent` event to the publisher's subscriber"]
        fn on_endpoint_packet_sent(&mut self, event: builder::EndpointPacketSent);
        #[doc = "Publishes a `EndpointPacketReceived` event to the publisher's subscriber"]
        fn on_endpoint_packet_received(&mut self, event: builder::EndpointPacketReceived);
        #[doc = "Publishes a `EndpointDatagramSent` event to the publisher's subscriber"]
        fn on_endpoint_datagram_sent(&mut self, event: builder::EndpointDatagramSent);
        #[doc = "Publishes a `EndpointDatagramReceived` event to the publisher's subscriber"]
        fn on_endpoint_datagram_received(&mut self, event: builder::EndpointDatagramReceived);
        #[doc = "Publishes a `EndpointDatagramDropped` event to the publisher's subscriber"]
        fn on_endpoint_datagram_dropped(&mut self, event: builder::EndpointDatagramDropped);
        #[doc = r" Returns the QUIC version, if any"]
        fn quic_version(&self) -> Option<u32>;
    }
    pub struct EndpointPublisherSubscriber<'a, Sub: Subscriber> {
        meta: EndpointMeta,
        quic_version: Option<u32>,
        subscriber: &'a mut Sub,
    }
    impl<'a, Sub: Subscriber> fmt::Debug for EndpointPublisherSubscriber<'a, Sub> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("ConnectionPublisherSubscriber")
                .field("meta", &self.meta)
                .field("quic_version", &self.quic_version)
                .finish()
        }
    }
    impl<'a, Sub: Subscriber> EndpointPublisherSubscriber<'a, Sub> {
        #[inline]
        pub fn new(
            meta: builder::EndpointMeta,
            quic_version: Option<u32>,
            subscriber: &'a mut Sub,
        ) -> Self {
            Self {
                meta: meta.into_event(),
                quic_version,
                subscriber,
            }
        }
    }
    impl<'a, Sub: Subscriber> EndpointPublisher for EndpointPublisherSubscriber<'a, Sub> {
        #[inline]
        fn on_version_information(&mut self, event: builder::VersionInformation) {
            let event = event.into_event();
            self.subscriber.on_version_information(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_packet_sent(&mut self, event: builder::EndpointPacketSent) {
            let event = event.into_event();
            self.subscriber.on_endpoint_packet_sent(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_packet_received(&mut self, event: builder::EndpointPacketReceived) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_packet_received(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_datagram_sent(&mut self, event: builder::EndpointDatagramSent) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_datagram_sent(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_datagram_received(&mut self, event: builder::EndpointDatagramReceived) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_datagram_received(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_datagram_dropped(&mut self, event: builder::EndpointDatagramDropped) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_datagram_dropped(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> Option<u32> {
            self.quic_version
        }
    }
    pub trait ConnectionPublisher {
        #[doc = "Publishes a `AlpnInformation` event to the publisher's subscriber"]
        fn on_alpn_information(&mut self, event: builder::AlpnInformation);
        #[doc = "Publishes a `SniInformation` event to the publisher's subscriber"]
        fn on_sni_information(&mut self, event: builder::SniInformation);
        #[doc = "Publishes a `PacketSent` event to the publisher's subscriber"]
        fn on_packet_sent(&mut self, event: builder::PacketSent);
        #[doc = "Publishes a `PacketReceived` event to the publisher's subscriber"]
        fn on_packet_received(&mut self, event: builder::PacketReceived);
        #[doc = "Publishes a `ActivePathUpdated` event to the publisher's subscriber"]
        fn on_active_path_updated(&mut self, event: builder::ActivePathUpdated);
        #[doc = "Publishes a `PathCreated` event to the publisher's subscriber"]
        fn on_path_created(&mut self, event: builder::PathCreated);
        #[doc = "Publishes a `FrameSent` event to the publisher's subscriber"]
        fn on_frame_sent(&mut self, event: builder::FrameSent);
        #[doc = "Publishes a `FrameReceived` event to the publisher's subscriber"]
        fn on_frame_received(&mut self, event: builder::FrameReceived);
        #[doc = "Publishes a `PacketLost` event to the publisher's subscriber"]
        fn on_packet_lost(&mut self, event: builder::PacketLost);
        #[doc = "Publishes a `RecoveryMetrics` event to the publisher's subscriber"]
        fn on_recovery_metrics(&mut self, event: builder::RecoveryMetrics);
        #[doc = "Publishes a `KeyUpdate` event to the publisher's subscriber"]
        fn on_key_update(&mut self, event: builder::KeyUpdate);
        #[doc = "Publishes a `ConnectionStarted` event to the publisher's subscriber"]
        fn on_connection_started(&mut self, event: builder::ConnectionStarted);
        #[doc = "Publishes a `ConnectionClosed` event to the publisher's subscriber"]
        fn on_connection_closed(&mut self, event: builder::ConnectionClosed);
        #[doc = "Publishes a `DuplicatePacket` event to the publisher's subscriber"]
        fn on_duplicate_packet(&mut self, event: builder::DuplicatePacket);
        #[doc = "Publishes a `DatagramSent` event to the publisher's subscriber"]
        fn on_datagram_sent(&mut self, event: builder::DatagramSent);
        #[doc = "Publishes a `DatagramReceived` event to the publisher's subscriber"]
        fn on_datagram_received(&mut self, event: builder::DatagramReceived);
        #[doc = "Publishes a `DatagramDropped` event to the publisher's subscriber"]
        fn on_datagram_dropped(&mut self, event: builder::DatagramDropped);
        #[doc = "Publishes a `ConnectionIdUpdated` event to the publisher's subscriber"]
        fn on_connection_id_updated(&mut self, event: builder::ConnectionIdUpdated);
        #[doc = r" Returns the QUIC version negotiated for the current connection, if any"]
        fn quic_version(&self) -> u32;
    }
    pub struct ConnectionPublisherSubscriber<'a, Sub: Subscriber> {
        meta: ConnectionMeta,
        quic_version: u32,
        subscriber: &'a mut Sub,
        context: &'a mut Sub::ConnectionContext,
    }
    impl<'a, Sub: Subscriber> fmt::Debug for ConnectionPublisherSubscriber<'a, Sub> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("ConnectionPublisherSubscriber")
                .field("meta", &self.meta)
                .field("quic_version", &self.quic_version)
                .finish()
        }
    }
    impl<'a, Sub: Subscriber> ConnectionPublisherSubscriber<'a, Sub> {
        #[inline]
        pub fn new(
            meta: builder::ConnectionMeta,
            quic_version: u32,
            subscriber: &'a mut Sub,
            context: &'a mut Sub::ConnectionContext,
        ) -> Self {
            Self {
                meta: meta.into_event(),
                quic_version,
                subscriber,
                context,
            }
        }
    }
    impl<'a, Sub: Subscriber> ConnectionPublisher for ConnectionPublisherSubscriber<'a, Sub> {
        #[inline]
        fn on_alpn_information(&mut self, event: builder::AlpnInformation) {
            let event = event.into_event();
            self.subscriber
                .on_alpn_information(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_sni_information(&mut self, event: builder::SniInformation) {
            let event = event.into_event();
            self.subscriber
                .on_sni_information(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_packet_sent(&mut self, event: builder::PacketSent) {
            let event = event.into_event();
            self.subscriber
                .on_packet_sent(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_packet_received(&mut self, event: builder::PacketReceived) {
            let event = event.into_event();
            self.subscriber
                .on_packet_received(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_active_path_updated(&mut self, event: builder::ActivePathUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_active_path_updated(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_created(&mut self, event: builder::PathCreated) {
            let event = event.into_event();
            self.subscriber
                .on_path_created(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_frame_sent(&mut self, event: builder::FrameSent) {
            let event = event.into_event();
            self.subscriber
                .on_frame_sent(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_frame_received(&mut self, event: builder::FrameReceived) {
            let event = event.into_event();
            self.subscriber
                .on_frame_received(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_packet_lost(&mut self, event: builder::PacketLost) {
            let event = event.into_event();
            self.subscriber
                .on_packet_lost(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_recovery_metrics(&mut self, event: builder::RecoveryMetrics) {
            let event = event.into_event();
            self.subscriber
                .on_recovery_metrics(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_key_update(&mut self, event: builder::KeyUpdate) {
            let event = event.into_event();
            self.subscriber
                .on_key_update(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_connection_started(&mut self, event: builder::ConnectionStarted) {
            let event = event.into_event();
            self.subscriber
                .on_connection_started(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_connection_closed(&mut self, event: builder::ConnectionClosed) {
            let event = event.into_event();
            self.subscriber
                .on_connection_closed(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_duplicate_packet(&mut self, event: builder::DuplicatePacket) {
            let event = event.into_event();
            self.subscriber
                .on_duplicate_packet(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_datagram_sent(&mut self, event: builder::DatagramSent) {
            let event = event.into_event();
            self.subscriber
                .on_datagram_sent(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_datagram_received(&mut self, event: builder::DatagramReceived) {
            let event = event.into_event();
            self.subscriber
                .on_datagram_received(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_datagram_dropped(&mut self, event: builder::DatagramDropped) {
            let event = event.into_event();
            self.subscriber
                .on_datagram_dropped(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_connection_id_updated(&mut self, event: builder::ConnectionIdUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_connection_id_updated(&mut self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(&mut self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> u32 {
            self.quic_version
        }
    }
}
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    #[derive(Copy, Clone, Debug, Default)]
    pub struct Subscriber {
        pub alpn_information: u32,
        pub sni_information: u32,
        pub packet_sent: u32,
        pub packet_received: u32,
        pub active_path_updated: u32,
        pub path_created: u32,
        pub frame_sent: u32,
        pub frame_received: u32,
        pub packet_lost: u32,
        pub recovery_metrics: u32,
        pub key_update: u32,
        pub connection_started: u32,
        pub connection_closed: u32,
        pub duplicate_packet: u32,
        pub datagram_sent: u32,
        pub datagram_received: u32,
        pub datagram_dropped: u32,
        pub connection_id_updated: u32,
        pub version_information: u32,
        pub endpoint_packet_sent: u32,
        pub endpoint_packet_received: u32,
        pub endpoint_datagram_sent: u32,
        pub endpoint_datagram_received: u32,
        pub endpoint_datagram_dropped: u32,
    }
    impl super::Subscriber for Subscriber {
        type ConnectionContext = ();
        fn create_connection_context(
            &mut self,
            _meta: &api::ConnectionMeta,
            _info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
        }
        fn on_alpn_information(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::AlpnInformation,
        ) {
            self.alpn_information += 1;
        }
        fn on_sni_information(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::SniInformation,
        ) {
            self.sni_information += 1;
        }
        fn on_packet_sent(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::PacketSent,
        ) {
            self.packet_sent += 1;
        }
        fn on_packet_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::PacketReceived,
        ) {
            self.packet_received += 1;
        }
        fn on_active_path_updated(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::ActivePathUpdated,
        ) {
            self.active_path_updated += 1;
        }
        fn on_path_created(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::PathCreated,
        ) {
            self.path_created += 1;
        }
        fn on_frame_sent(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::FrameSent,
        ) {
            self.frame_sent += 1;
        }
        fn on_frame_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::FrameReceived,
        ) {
            self.frame_received += 1;
        }
        fn on_packet_lost(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::PacketLost,
        ) {
            self.packet_lost += 1;
        }
        fn on_recovery_metrics(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::RecoveryMetrics,
        ) {
            self.recovery_metrics += 1;
        }
        fn on_key_update(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::KeyUpdate,
        ) {
            self.key_update += 1;
        }
        fn on_connection_started(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::ConnectionStarted,
        ) {
            self.connection_started += 1;
        }
        fn on_connection_closed(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::ConnectionClosed,
        ) {
            self.connection_closed += 1;
        }
        fn on_duplicate_packet(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::DuplicatePacket,
        ) {
            self.duplicate_packet += 1;
        }
        fn on_datagram_sent(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::DatagramSent,
        ) {
            self.datagram_sent += 1;
        }
        fn on_datagram_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::DatagramReceived,
        ) {
            self.datagram_received += 1;
        }
        fn on_datagram_dropped(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::DatagramDropped,
        ) {
            self.datagram_dropped += 1;
        }
        fn on_connection_id_updated(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            _event: &api::ConnectionIdUpdated,
        ) {
            self.connection_id_updated += 1;
        }
        fn on_version_information(
            &mut self,
            _meta: &api::EndpointMeta,
            _event: &api::VersionInformation,
        ) {
            self.version_information += 1;
        }
        fn on_endpoint_packet_sent(
            &mut self,
            _meta: &api::EndpointMeta,
            _event: &api::EndpointPacketSent,
        ) {
            self.endpoint_packet_sent += 1;
        }
        fn on_endpoint_packet_received(
            &mut self,
            _meta: &api::EndpointMeta,
            _event: &api::EndpointPacketReceived,
        ) {
            self.endpoint_packet_received += 1;
        }
        fn on_endpoint_datagram_sent(
            &mut self,
            _meta: &api::EndpointMeta,
            _event: &api::EndpointDatagramSent,
        ) {
            self.endpoint_datagram_sent += 1;
        }
        fn on_endpoint_datagram_received(
            &mut self,
            _meta: &api::EndpointMeta,
            _event: &api::EndpointDatagramReceived,
        ) {
            self.endpoint_datagram_received += 1;
        }
        fn on_endpoint_datagram_dropped(
            &mut self,
            _meta: &api::EndpointMeta,
            _event: &api::EndpointDatagramDropped,
        ) {
            self.endpoint_datagram_dropped += 1;
        }
    }
    #[derive(Copy, Clone, Debug, Default)]
    pub struct Publisher {
        pub alpn_information: u32,
        pub sni_information: u32,
        pub packet_sent: u32,
        pub packet_received: u32,
        pub active_path_updated: u32,
        pub path_created: u32,
        pub frame_sent: u32,
        pub frame_received: u32,
        pub packet_lost: u32,
        pub recovery_metrics: u32,
        pub key_update: u32,
        pub connection_started: u32,
        pub connection_closed: u32,
        pub duplicate_packet: u32,
        pub datagram_sent: u32,
        pub datagram_received: u32,
        pub datagram_dropped: u32,
        pub connection_id_updated: u32,
        pub version_information: u32,
        pub endpoint_packet_sent: u32,
        pub endpoint_packet_received: u32,
        pub endpoint_datagram_sent: u32,
        pub endpoint_datagram_received: u32,
        pub endpoint_datagram_dropped: u32,
    }
    impl super::EndpointPublisher for Publisher {
        fn on_version_information(&mut self, _event: builder::VersionInformation) {
            self.version_information += 1;
        }
        fn on_endpoint_packet_sent(&mut self, _event: builder::EndpointPacketSent) {
            self.endpoint_packet_sent += 1;
        }
        fn on_endpoint_packet_received(&mut self, _event: builder::EndpointPacketReceived) {
            self.endpoint_packet_received += 1;
        }
        fn on_endpoint_datagram_sent(&mut self, _event: builder::EndpointDatagramSent) {
            self.endpoint_datagram_sent += 1;
        }
        fn on_endpoint_datagram_received(&mut self, _event: builder::EndpointDatagramReceived) {
            self.endpoint_datagram_received += 1;
        }
        fn on_endpoint_datagram_dropped(&mut self, _event: builder::EndpointDatagramDropped) {
            self.endpoint_datagram_dropped += 1;
        }
        fn quic_version(&self) -> Option<u32> {
            Some(1)
        }
    }
    impl super::ConnectionPublisher for Publisher {
        fn on_alpn_information(&mut self, _event: builder::AlpnInformation) {
            self.alpn_information += 1;
        }
        fn on_sni_information(&mut self, _event: builder::SniInformation) {
            self.sni_information += 1;
        }
        fn on_packet_sent(&mut self, _event: builder::PacketSent) {
            self.packet_sent += 1;
        }
        fn on_packet_received(&mut self, _event: builder::PacketReceived) {
            self.packet_received += 1;
        }
        fn on_active_path_updated(&mut self, _event: builder::ActivePathUpdated) {
            self.active_path_updated += 1;
        }
        fn on_path_created(&mut self, _event: builder::PathCreated) {
            self.path_created += 1;
        }
        fn on_frame_sent(&mut self, _event: builder::FrameSent) {
            self.frame_sent += 1;
        }
        fn on_frame_received(&mut self, _event: builder::FrameReceived) {
            self.frame_received += 1;
        }
        fn on_packet_lost(&mut self, _event: builder::PacketLost) {
            self.packet_lost += 1;
        }
        fn on_recovery_metrics(&mut self, _event: builder::RecoveryMetrics) {
            self.recovery_metrics += 1;
        }
        fn on_key_update(&mut self, _event: builder::KeyUpdate) {
            self.key_update += 1;
        }
        fn on_connection_started(&mut self, _event: builder::ConnectionStarted) {
            self.connection_started += 1;
        }
        fn on_connection_closed(&mut self, _event: builder::ConnectionClosed) {
            self.connection_closed += 1;
        }
        fn on_duplicate_packet(&mut self, _event: builder::DuplicatePacket) {
            self.duplicate_packet += 1;
        }
        fn on_datagram_sent(&mut self, _event: builder::DatagramSent) {
            self.datagram_sent += 1;
        }
        fn on_datagram_received(&mut self, _event: builder::DatagramReceived) {
            self.datagram_received += 1;
        }
        fn on_datagram_dropped(&mut self, _event: builder::DatagramDropped) {
            self.datagram_dropped += 1;
        }
        fn on_connection_id_updated(&mut self, _event: builder::ConnectionIdUpdated) {
            self.connection_id_updated += 1;
        }
        fn quic_version(&self) -> u32 {
            1
        }
    }
}
