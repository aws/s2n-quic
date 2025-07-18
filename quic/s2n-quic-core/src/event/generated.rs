// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

#![allow(clippy::needless_lifetimes)]
use super::*;
pub(crate) mod metrics;
pub mod api {
    #![doc = r" This module contains events that are emitted to the [`Subscriber`](crate::event::Subscriber)"]
    use super::*;
    #[allow(unused_imports)]
    use crate::event::metrics::aggregate;
    pub use traits::Subscriber;
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionMeta {
        pub endpoint_type: EndpointType,
        pub id: u64,
        pub timestamp: crate::event::Timestamp,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionMeta {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionMeta");
            fmt.field("endpoint_type", &self.endpoint_type);
            fmt.field("id", &self.id);
            fmt.field("timestamp", &self.timestamp);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointMeta {
        pub endpoint_type: EndpointType,
        pub timestamp: crate::event::Timestamp,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointMeta {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointMeta");
            fmt.field("endpoint_type", &self.endpoint_type);
            fmt.field("timestamp", &self.timestamp);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionInfo {}
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionInfo {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionInfo");
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct TransportParameters<'a> {
        pub original_destination_connection_id: Option<ConnectionId<'a>>,
        pub initial_source_connection_id: Option<ConnectionId<'a>>,
        pub retry_source_connection_id: Option<ConnectionId<'a>>,
        pub stateless_reset_token: Option<&'a [u8]>,
        pub preferred_address: Option<PreferredAddress<'a>>,
        pub migration_support: bool,
        pub max_idle_timeout: Duration,
        pub ack_delay_exponent: u8,
        pub max_ack_delay: Duration,
        pub max_udp_payload_size: u64,
        pub active_connection_id_limit: u64,
        pub initial_max_stream_data_bidi_local: u64,
        pub initial_max_stream_data_bidi_remote: u64,
        pub initial_max_stream_data_uni: u64,
        pub initial_max_streams_bidi: u64,
        pub initial_max_streams_uni: u64,
        pub max_datagram_frame_size: u64,
        pub dc_supported_versions: &'a [u32],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for TransportParameters<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("TransportParameters");
            fmt.field(
                "original_destination_connection_id",
                &self.original_destination_connection_id,
            );
            fmt.field(
                "initial_source_connection_id",
                &self.initial_source_connection_id,
            );
            fmt.field(
                "retry_source_connection_id",
                &self.retry_source_connection_id,
            );
            fmt.field("stateless_reset_token", &self.stateless_reset_token);
            fmt.field("preferred_address", &self.preferred_address);
            fmt.field("migration_support", &self.migration_support);
            fmt.field("max_idle_timeout", &self.max_idle_timeout);
            fmt.field("ack_delay_exponent", &self.ack_delay_exponent);
            fmt.field("max_ack_delay", &self.max_ack_delay);
            fmt.field("max_udp_payload_size", &self.max_udp_payload_size);
            fmt.field(
                "active_connection_id_limit",
                &self.active_connection_id_limit,
            );
            fmt.field(
                "initial_max_stream_data_bidi_local",
                &self.initial_max_stream_data_bidi_local,
            );
            fmt.field(
                "initial_max_stream_data_bidi_remote",
                &self.initial_max_stream_data_bidi_remote,
            );
            fmt.field(
                "initial_max_stream_data_uni",
                &self.initial_max_stream_data_uni,
            );
            fmt.field("initial_max_streams_bidi", &self.initial_max_streams_bidi);
            fmt.field("initial_max_streams_uni", &self.initial_max_streams_uni);
            fmt.field("max_datagram_frame_size", &self.max_datagram_frame_size);
            fmt.field("dc_supported_versions", &self.dc_supported_versions);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct PreferredAddress<'a> {
        pub ipv4_address: Option<SocketAddress<'a>>,
        pub ipv6_address: Option<SocketAddress<'a>>,
        pub connection_id: ConnectionId<'a>,
        pub stateless_reset_token: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PreferredAddress<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PreferredAddress");
            fmt.field("ipv4_address", &self.ipv4_address);
            fmt.field("ipv6_address", &self.ipv6_address);
            fmt.field("connection_id", &self.connection_id);
            fmt.field("stateless_reset_token", &self.stateless_reset_token);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct Path<'a> {
        pub local_addr: SocketAddress<'a>,
        pub local_cid: ConnectionId<'a>,
        pub remote_addr: SocketAddress<'a>,
        pub remote_cid: ConnectionId<'a>,
        pub id: u64,
        pub is_active: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for Path<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("Path");
            fmt.field("local_addr", &self.local_addr);
            fmt.field("local_cid", &self.local_cid);
            fmt.field("remote_addr", &self.remote_addr);
            fmt.field("remote_cid", &self.remote_cid);
            fmt.field("id", &self.id);
            fmt.field("is_active", &self.is_active);
            fmt.finish()
        }
    }
    #[non_exhaustive]
    #[derive(Clone)]
    pub struct ConnectionId<'a> {
        pub bytes: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ConnectionId<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionId");
            fmt.field("bytes", &self.bytes);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EcnCounts {
        #[doc = " A variable-length integer representing the total number of packets"]
        #[doc = " received with the ECT(0) codepoint."]
        pub ect_0_count: u64,
        #[doc = " A variable-length integer representing the total number of packets"]
        #[doc = " received with the ECT(1) codepoint."]
        pub ect_1_count: u64,
        #[doc = " A variable-length integer representing the total number of packets"]
        #[doc = " received with the CE codepoint."]
        pub ce_count: u64,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EcnCounts {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EcnCounts");
            fmt.field("ect_0_count", &self.ect_0_count);
            fmt.field("ect_1_count", &self.ect_1_count);
            fmt.field("ce_count", &self.ce_count);
            fmt.finish()
        }
    }
    #[non_exhaustive]
    #[derive(Clone)]
    pub struct ConnectionCloseFrame<'a> {
        pub error_code: u64,
        pub frame_type: Option<u64>,
        pub reason: Option<&'a [u8]>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ConnectionCloseFrame<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionCloseFrame");
            fmt.field("error_code", &self.error_code);
            fmt.field("frame_type", &self.frame_type);
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct MtuConfig {
        pub initial_mtu: u16,
        pub base_mtu: u16,
        pub max_mtu: u16,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for MtuConfig {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("MtuConfig");
            fmt.field("initial_mtu", &self.initial_mtu);
            fmt.field("base_mtu", &self.base_mtu);
            fmt.field("max_mtu", &self.max_mtu);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " A bandwidth delivery rate estimate with associated metadata"]
    pub struct RateSample {
        #[doc = " The length of the sampling interval"]
        pub interval: Duration,
        #[doc = " The amount of data in bytes marked as delivered over the sampling interval"]
        pub delivered_bytes: u64,
        #[doc = " The amount of data in bytes marked as lost over the sampling interval"]
        pub lost_bytes: u64,
        #[doc = " The number of packets marked as explicit congestion experienced over the sampling interval"]
        pub ecn_ce_count: u64,
        #[doc = " PacketInfo::is_app_limited from the most recent acknowledged packet"]
        pub is_app_limited: bool,
        #[doc = " PacketInfo::delivered_bytes from the most recent acknowledged packet"]
        pub prior_delivered_bytes: u64,
        #[doc = " PacketInfo::bytes_in_flight from the most recent acknowledged packet"]
        pub bytes_in_flight: u32,
        #[doc = " PacketInfo::lost_bytes from the most recent acknowledged packet"]
        pub prior_lost_bytes: u64,
        #[doc = " PacketInfo::ecn_ce_count from the most recent acknowledged packet"]
        pub prior_ecn_ce_count: u64,
        #[doc = " The delivery rate for this rate sample"]
        pub delivery_rate_bytes_per_second: u64,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for RateSample {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("RateSample");
            fmt.field("interval", &self.interval);
            fmt.field("delivered_bytes", &self.delivered_bytes);
            fmt.field("lost_bytes", &self.lost_bytes);
            fmt.field("ecn_ce_count", &self.ecn_ce_count);
            fmt.field("is_app_limited", &self.is_app_limited);
            fmt.field("prior_delivered_bytes", &self.prior_delivered_bytes);
            fmt.field("bytes_in_flight", &self.bytes_in_flight);
            fmt.field("prior_lost_bytes", &self.prior_lost_bytes);
            fmt.field("prior_ecn_ce_count", &self.prior_ecn_ce_count);
            fmt.field(
                "delivery_rate_bytes_per_second",
                &self.delivery_rate_bytes_per_second,
            );
            fmt.finish()
        }
    }
    #[non_exhaustive]
    #[derive(Clone)]
    pub enum SocketAddress<'a> {
        #[non_exhaustive]
        IpV4 { ip: &'a [u8; 4], port: u16 },
        #[non_exhaustive]
        IpV6 { ip: &'a [u8; 16], port: u16 },
    }
    impl<'a> aggregate::AsVariant for SocketAddress<'a> {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("IP_V4\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("IP_V6\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::IpV4 { .. } => 0usize,
                Self::IpV6 { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum DuplicatePacketError {
        #[non_exhaustive]
        #[doc = " The packet number was already received and is a duplicate."]
        Duplicate {},
        #[non_exhaustive]
        #[doc = " The received packet number was outside the range of tracked packet numbers."]
        #[doc = ""]
        #[doc = " This can happen when packets are heavily delayed or reordered. Currently, the maximum"]
        #[doc = " amount of reordering is limited to 128 packets. For example, if packet number `142`"]
        #[doc = " is received, the allowed range would be limited to `14-142`. If an endpoint received"]
        #[doc = " packet `< 14`, it would trigger this event."]
        TooOld {},
    }
    impl aggregate::AsVariant for DuplicatePacketError {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("DUPLICATE\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("TOO_OLD\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Duplicate { .. } => 0usize,
                Self::TooOld { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum Frame {
        #[non_exhaustive]
        Padding {},
        #[non_exhaustive]
        Ping {},
        #[non_exhaustive]
        Ack {
            ecn_counts: Option<EcnCounts>,
            largest_acknowledged: u64,
            ack_range_count: u64,
        },
        #[non_exhaustive]
        ResetStream {
            id: u64,
            error_code: u64,
            final_size: u64,
        },
        #[non_exhaustive]
        StopSending { id: u64, error_code: u64 },
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
        MaxData { value: u64 },
        #[non_exhaustive]
        MaxStreamData {
            stream_type: StreamType,
            id: u64,
            value: u64,
        },
        #[non_exhaustive]
        MaxStreams { stream_type: StreamType, value: u64 },
        #[non_exhaustive]
        DataBlocked { data_limit: u64 },
        #[non_exhaustive]
        StreamDataBlocked {
            stream_id: u64,
            stream_data_limit: u64,
        },
        #[non_exhaustive]
        StreamsBlocked {
            stream_type: StreamType,
            stream_limit: u64,
        },
        #[non_exhaustive]
        NewConnectionId {
            sequence_number: u64,
            retire_prior_to: u64,
        },
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
        #[non_exhaustive]
        Datagram { len: u16 },
        #[non_exhaustive]
        DcStatelessResetTokens {},
    }
    impl aggregate::AsVariant for Frame {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PADDING\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PING\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ACK\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("RESET_STREAM\0"),
                id: 3usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("STOP_SENDING\0"),
                id: 4usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CRYPTO\0"),
                id: 5usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("NEW_TOKEN\0"),
                id: 6usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("STREAM\0"),
                id: 7usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("MAX_DATA\0"),
                id: 8usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("MAX_STREAM_DATA\0"),
                id: 9usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("MAX_STREAMS\0"),
                id: 10usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("DATA_BLOCKED\0"),
                id: 11usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("STREAM_DATA_BLOCKED\0"),
                id: 12usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("STREAMS_BLOCKED\0"),
                id: 13usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("NEW_CONNECTION_ID\0"),
                id: 14usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("RETIRE_CONNECTION_ID\0"),
                id: 15usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PATH_CHALLENGE\0"),
                id: 16usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PATH_RESPONSE\0"),
                id: 17usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CONNECTION_CLOSE\0"),
                id: 18usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("HANDSHAKE_DONE\0"),
                id: 19usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("DATAGRAM\0"),
                id: 20usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("DC_STATELESS_RESET_TOKENS\0"),
                id: 21usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Padding { .. } => 0usize,
                Self::Ping { .. } => 1usize,
                Self::Ack { .. } => 2usize,
                Self::ResetStream { .. } => 3usize,
                Self::StopSending { .. } => 4usize,
                Self::Crypto { .. } => 5usize,
                Self::NewToken { .. } => 6usize,
                Self::Stream { .. } => 7usize,
                Self::MaxData { .. } => 8usize,
                Self::MaxStreamData { .. } => 9usize,
                Self::MaxStreams { .. } => 10usize,
                Self::DataBlocked { .. } => 11usize,
                Self::StreamDataBlocked { .. } => 12usize,
                Self::StreamsBlocked { .. } => 13usize,
                Self::NewConnectionId { .. } => 14usize,
                Self::RetireConnectionId { .. } => 15usize,
                Self::PathChallenge { .. } => 16usize,
                Self::PathResponse { .. } => 17usize,
                Self::ConnectionClose { .. } => 18usize,
                Self::HandshakeDone { .. } => 19usize,
                Self::Datagram { .. } => 20usize,
                Self::DcStatelessResetTokens { .. } => 21usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum StreamType {
        #[non_exhaustive]
        Bidirectional {},
        #[non_exhaustive]
        Unidirectional {},
    }
    impl aggregate::AsVariant for StreamType {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("BIDIRECTIONAL\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNIDIRECTIONAL\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Bidirectional { .. } => 0usize,
                Self::Unidirectional { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum PacketHeader {
        #[non_exhaustive]
        Initial { number: u64, version: u32 },
        #[non_exhaustive]
        Handshake { number: u64, version: u32 },
        #[non_exhaustive]
        ZeroRtt { number: u64, version: u32 },
        #[non_exhaustive]
        OneRtt { number: u64 },
        #[non_exhaustive]
        Retry { version: u32 },
        #[non_exhaustive]
        VersionNegotiation {},
        #[non_exhaustive]
        StatelessReset {},
    }
    impl aggregate::AsVariant for PacketHeader {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INITIAL\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("HANDSHAKE\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ZERO_RTT\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ONE_RTT\0"),
                id: 3usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("RETRY\0"),
                id: 4usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("VERSION_NEGOTIATION\0"),
                id: 5usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("STATELESS_RESET\0"),
                id: 6usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Initial { .. } => 0usize,
                Self::Handshake { .. } => 1usize,
                Self::ZeroRtt { .. } => 2usize,
                Self::OneRtt { .. } => 3usize,
                Self::Retry { .. } => 4usize,
                Self::VersionNegotiation { .. } => 5usize,
                Self::StatelessReset { .. } => 6usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum PacketType {
        #[non_exhaustive]
        Initial {},
        #[non_exhaustive]
        Handshake {},
        #[non_exhaustive]
        ZeroRtt {},
        #[non_exhaustive]
        OneRtt {},
        #[non_exhaustive]
        Retry {},
        #[non_exhaustive]
        VersionNegotiation {},
        #[non_exhaustive]
        StatelessReset {},
    }
    impl aggregate::AsVariant for PacketType {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INITIAL\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("HANDSHAKE\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ZERO_RTT\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ONE_RTT\0"),
                id: 3usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("RETRY\0"),
                id: 4usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("VERSION_NEGOTIATION\0"),
                id: 5usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("STATELESS_RESET\0"),
                id: 6usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Initial { .. } => 0usize,
                Self::Handshake { .. } => 1usize,
                Self::ZeroRtt { .. } => 2usize,
                Self::OneRtt { .. } => 3usize,
                Self::Retry { .. } => 4usize,
                Self::VersionNegotiation { .. } => 5usize,
                Self::StatelessReset { .. } => 6usize,
            }
        }
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
    impl aggregate::AsVariant for KeyType {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INITIAL\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("HANDSHAKE\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ZERO_RTT\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ONE_RTT\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Initial { .. } => 0usize,
                Self::Handshake { .. } => 1usize,
                Self::ZeroRtt { .. } => 2usize,
                Self::OneRtt { .. } => 3usize,
            }
        }
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
        #[doc = " This maps to an internal connection id, which is a stable identifier across CID changes."]
        Connection { id: u64 },
    }
    impl aggregate::AsVariant for Subject {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ENDPOINT\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CONNECTION\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Endpoint { .. } => 0usize,
                Self::Connection { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " An endpoint may be either a Server or a Client"]
    pub enum EndpointType {
        #[non_exhaustive]
        Server {},
        #[non_exhaustive]
        Client {},
    }
    impl aggregate::AsVariant for EndpointType {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("SERVER\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CLIENT\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Server { .. } => 0usize,
                Self::Client { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum DatagramDropReason {
        #[non_exhaustive]
        #[doc = " There was an error while attempting to decode the datagram."]
        DecodingFailed {},
        #[non_exhaustive]
        #[doc = " There was an error while parsing the Retry token."]
        InvalidRetryToken {},
        #[non_exhaustive]
        #[doc = " The peer specified an unsupported QUIC version."]
        UnsupportedVersion {},
        #[non_exhaustive]
        #[doc = " The peer sent an invalid Destination Connection Id."]
        InvalidDestinationConnectionId {},
        #[non_exhaustive]
        #[doc = " The peer sent an invalid Source Connection Id."]
        InvalidSourceConnectionId {},
        #[non_exhaustive]
        #[doc = " Application provided invalid MTU configuration."]
        InvalidMtuConfiguration {
            #[doc = " MTU configuration for the endpoint"]
            endpoint_mtu_config: MtuConfig,
        },
        #[non_exhaustive]
        #[doc = " The Destination Connection Id is unknown and does not map to a Connection."]
        #[doc = ""]
        #[doc = " Connections are mapped to Destination Connections Ids (DCID) and packets"]
        #[doc = " in a Datagram are routed to a connection based on the DCID in the first"]
        #[doc = " packet. If a Connection is not found for the specified DCID then the"]
        #[doc = " datagram can not be processed and is dropped."]
        UnknownDestinationConnectionId {},
        #[non_exhaustive]
        #[doc = " The connection attempt was rejected."]
        RejectedConnectionAttempt {},
        #[non_exhaustive]
        #[doc = " A datagram was received from an unknown server address."]
        UnknownServerAddress {},
        #[non_exhaustive]
        #[doc = " The peer initiated a connection migration before the handshake was confirmed."]
        #[doc = ""]
        #[doc = " Note: This drop reason is no longer emitted"]
        ConnectionMigrationDuringHandshake {},
        #[non_exhaustive]
        #[doc = " The attempted connection migration was rejected."]
        RejectedConnectionMigration { reason: MigrationDenyReason },
        #[non_exhaustive]
        #[doc = " The maximum number of paths per connection was exceeded."]
        PathLimitExceeded {},
        #[non_exhaustive]
        #[doc = " The peer initiated a connection migration without supplying enough connection IDs to use."]
        #[doc = ""]
        #[doc = " Note: This drop reason is no longer emitted"]
        InsufficientConnectionIds {},
    }
    impl aggregate::AsVariant for DatagramDropReason {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("DECODING_FAILED\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INVALID_RETRY_TOKEN\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNSUPPORTED_VERSION\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INVALID_DESTINATION_CONNECTION_ID\0"),
                id: 3usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INVALID_SOURCE_CONNECTION_ID\0"),
                id: 4usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INVALID_MTU_CONFIGURATION\0"),
                id: 5usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNKNOWN_DESTINATION_CONNECTION_ID\0"),
                id: 6usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("REJECTED_CONNECTION_ATTEMPT\0"),
                id: 7usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNKNOWN_SERVER_ADDRESS\0"),
                id: 8usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CONNECTION_MIGRATION_DURING_HANDSHAKE\0"),
                id: 9usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("REJECTED_CONNECTION_MIGRATION\0"),
                id: 10usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PATH_LIMIT_EXCEEDED\0"),
                id: 11usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INSUFFICIENT_CONNECTION_IDS\0"),
                id: 12usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::DecodingFailed { .. } => 0usize,
                Self::InvalidRetryToken { .. } => 1usize,
                Self::UnsupportedVersion { .. } => 2usize,
                Self::InvalidDestinationConnectionId { .. } => 3usize,
                Self::InvalidSourceConnectionId { .. } => 4usize,
                Self::InvalidMtuConfiguration { .. } => 5usize,
                Self::UnknownDestinationConnectionId { .. } => 6usize,
                Self::RejectedConnectionAttempt { .. } => 7usize,
                Self::UnknownServerAddress { .. } => 8usize,
                Self::ConnectionMigrationDuringHandshake { .. } => 9usize,
                Self::RejectedConnectionMigration { .. } => 10usize,
                Self::PathLimitExceeded { .. } => 11usize,
                Self::InsufficientConnectionIds { .. } => 12usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum KeySpace {
        #[non_exhaustive]
        Initial {},
        #[non_exhaustive]
        Handshake {},
        #[non_exhaustive]
        ZeroRtt {},
        #[non_exhaustive]
        OneRtt {},
    }
    impl aggregate::AsVariant for KeySpace {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INITIAL\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("HANDSHAKE\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ZERO_RTT\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ONE_RTT\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Initial { .. } => 0usize,
                Self::Handshake { .. } => 1usize,
                Self::ZeroRtt { .. } => 2usize,
                Self::OneRtt { .. } => 3usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum PacketSkipReason {
        #[non_exhaustive]
        #[doc = " Skipped a packet number to elicit a quicker PTO acknowledgment"]
        PtoProbe {},
        #[non_exhaustive]
        #[doc = " Skipped a packet number to detect an Optimistic Ack attack"]
        OptimisticAckMitigation {},
    }
    impl aggregate::AsVariant for PacketSkipReason {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PTO_PROBE\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("OPTIMISTIC_ACK_MITIGATION\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::PtoProbe { .. } => 0usize,
                Self::OptimisticAckMitigation { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum PacketDropReason<'a> {
        #[non_exhaustive]
        #[doc = " A connection error occurred and is no longer able to process packets."]
        ConnectionError { path: Path<'a> },
        #[non_exhaustive]
        #[doc = " The handshake needed to be complete before processing the packet."]
        #[doc = ""]
        #[doc = " To ensure the connection stays secure, short packets can only be processed"]
        #[doc = " once the handshake has completed."]
        HandshakeNotComplete { path: Path<'a> },
        #[non_exhaustive]
        #[doc = " The packet contained a version which did not match the version negotiated"]
        #[doc = " during the handshake."]
        VersionMismatch { version: u32, path: Path<'a> },
        #[non_exhaustive]
        #[doc = " A datagram contained more than one destination connection ID, which is"]
        #[doc = " not allowed."]
        ConnectionIdMismatch {
            packet_cid: &'a [u8],
            path: Path<'a>,
        },
        #[non_exhaustive]
        #[doc = " There was a failure when attempting to remove header protection."]
        UnprotectFailed { space: KeySpace, path: Path<'a> },
        #[non_exhaustive]
        #[doc = " There was a failure when attempting to decrypt the packet."]
        DecryptionFailed {
            path: Path<'a>,
            packet_header: PacketHeader,
        },
        #[non_exhaustive]
        #[doc = " Packet decoding failed."]
        #[doc = ""]
        #[doc = " The payload is decoded one packet at a time. If decoding fails"]
        #[doc = " then the remaining packets are also discarded."]
        DecodingFailed { path: Path<'a> },
        #[non_exhaustive]
        #[doc = " The client received a non-empty retry token."]
        NonEmptyRetryToken { path: Path<'a> },
        #[non_exhaustive]
        #[doc = " A Retry packet was discarded."]
        RetryDiscarded {
            reason: RetryDiscardReason<'a>,
            path: Path<'a>,
        },
        #[non_exhaustive]
        #[doc = " The received Initial packet was not transported in a datagram of at least 1200 bytes"]
        UndersizedInitialPacket { path: Path<'a> },
        #[non_exhaustive]
        #[doc = " The destination connection ID in the packet was the initial connection ID but was in"]
        #[doc = " a non-initial packet."]
        InitialConnectionIdInvalidSpace {
            path: Path<'a>,
            packet_type: PacketType,
        },
        #[non_exhaustive]
        #[doc = " The packet space for a received packet did not exist"]
        PacketSpaceDoesNotExist {
            path: Path<'a>,
            packet_type: PacketType,
        },
    }
    impl<'a> aggregate::AsVariant for PacketDropReason<'a> {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CONNECTION_ERROR\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("HANDSHAKE_NOT_COMPLETE\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("VERSION_MISMATCH\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CONNECTION_ID_MISMATCH\0"),
                id: 3usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNPROTECT_FAILED\0"),
                id: 4usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("DECRYPTION_FAILED\0"),
                id: 5usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("DECODING_FAILED\0"),
                id: 6usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("NON_EMPTY_RETRY_TOKEN\0"),
                id: 7usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("RETRY_DISCARDED\0"),
                id: 8usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNDERSIZED_INITIAL_PACKET\0"),
                id: 9usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INITIAL_CONNECTION_ID_INVALID_SPACE\0"),
                id: 10usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PACKET_SPACE_DOES_NOT_EXIST\0"),
                id: 11usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::ConnectionError { .. } => 0usize,
                Self::HandshakeNotComplete { .. } => 1usize,
                Self::VersionMismatch { .. } => 2usize,
                Self::ConnectionIdMismatch { .. } => 3usize,
                Self::UnprotectFailed { .. } => 4usize,
                Self::DecryptionFailed { .. } => 5usize,
                Self::DecodingFailed { .. } => 6usize,
                Self::NonEmptyRetryToken { .. } => 7usize,
                Self::RetryDiscarded { .. } => 8usize,
                Self::UndersizedInitialPacket { .. } => 9usize,
                Self::InitialConnectionIdInvalidSpace { .. } => 10usize,
                Self::PacketSpaceDoesNotExist { .. } => 11usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[deprecated(note = "use on_rx_ack_range_dropped event instead")]
    pub enum AckAction {
        #[non_exhaustive]
        #[doc = " Ack range for received packets was dropped due to space constraints"]
        #[doc = ""]
        #[doc = " For the purpose of processing Acks, RX packet numbers are stored as"]
        #[doc = " packet_number ranges in an IntervalSet; only lower and upper bounds"]
        #[doc = " are stored instead of individual packet_numbers. Ranges are merged"]
        #[doc = " when possible so only disjointed ranges are stored."]
        #[doc = ""]
        #[doc = " When at `capacity`, the lowest packet_number range is dropped."]
        RxAckRangeDropped {
            #[doc = " The packet number range which was dropped"]
            packet_number_range: core::ops::RangeInclusive<u64>,
            #[doc = " The number of disjoint ranges the IntervalSet can store"]
            capacity: usize,
            #[doc = " The store packet_number range in the IntervalSet"]
            stored_range: core::ops::RangeInclusive<u64>,
        },
    }
    #[allow(deprecated)]
    impl aggregate::AsVariant for AckAction {
        const VARIANTS: &'static [aggregate::info::Variant] =
            &[aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("RX_ACK_RANGE_DROPPED\0"),
                id: 0usize,
            }
            .build()];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::RxAckRangeDropped { .. } => 0usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum RetryDiscardReason<'a> {
        #[non_exhaustive]
        #[doc = " Received a Retry packet with SCID field equal to DCID field."]
        ScidEqualsDcid { cid: &'a [u8] },
        #[non_exhaustive]
        #[doc = " A client only processes at most one Retry packet."]
        RetryAlreadyProcessed {},
        #[non_exhaustive]
        #[doc = " The client discards Retry packets if a valid Initial packet"]
        #[doc = " has been received and processed."]
        InitialAlreadyProcessed {},
        #[non_exhaustive]
        #[doc = " The Retry packet received contained an invalid retry integrity tag"]
        InvalidIntegrityTag {},
    }
    impl<'a> aggregate::AsVariant for RetryDiscardReason<'a> {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("SCID_EQUALS_DCID\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("RETRY_ALREADY_PROCESSED\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INITIAL_ALREADY_PROCESSED\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INVALID_INTEGRITY_TAG\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::ScidEqualsDcid { .. } => 0usize,
                Self::RetryAlreadyProcessed { .. } => 1usize,
                Self::InitialAlreadyProcessed { .. } => 2usize,
                Self::InvalidIntegrityTag { .. } => 3usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum MigrationDenyReason {
        #[non_exhaustive]
        BlockedPort {},
        #[non_exhaustive]
        PortScopeChanged {},
        #[non_exhaustive]
        IpScopeChange {},
        #[non_exhaustive]
        ConnectionMigrationDisabled {},
    }
    impl aggregate::AsVariant for MigrationDenyReason {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("BLOCKED_PORT\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PORT_SCOPE_CHANGED\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("IP_SCOPE_CHANGE\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CONNECTION_MIGRATION_DISABLED\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::BlockedPort { .. } => 0usize,
                Self::PortScopeChanged { .. } => 1usize,
                Self::IpScopeChange { .. } => 2usize,
                Self::ConnectionMigrationDisabled { .. } => 3usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The current state of the ECN controller for the path"]
    pub enum EcnState {
        #[non_exhaustive]
        #[doc = " ECN capability is being actively tested"]
        Testing {},
        #[non_exhaustive]
        #[doc = " ECN capability has been tested, but not validated yet"]
        Unknown {},
        #[non_exhaustive]
        #[doc = " ECN capability testing has failed validation"]
        Failed {},
        #[non_exhaustive]
        #[doc = " ECN capability has been confirmed"]
        Capable {},
    }
    impl aggregate::AsVariant for EcnState {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("TESTING\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNKNOWN\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("FAILED\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CAPABLE\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Testing { .. } => 0usize,
                Self::Unknown { .. } => 1usize,
                Self::Failed { .. } => 2usize,
                Self::Capable { .. } => 3usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Events tracking the progress of handshake status"]
    pub enum HandshakeStatus {
        #[non_exhaustive]
        #[doc = " The handshake has completed."]
        Complete {},
        #[non_exhaustive]
        #[doc = " The handshake has been confirmed."]
        Confirmed {},
        #[non_exhaustive]
        #[doc = " A HANDSHAKE_DONE frame was delivered or received."]
        #[doc = ""]
        #[doc = " A Client endpoint receives a HANDSHAKE_DONE frame and"]
        #[doc = " only a Server is allowed to send the HANDSHAKE_DONE"]
        #[doc = " frame."]
        HandshakeDoneAcked {},
        #[non_exhaustive]
        #[doc = " A HANDSHAKE_DONE frame was declared lost."]
        #[doc = ""]
        #[doc = " The Server is responsible for re-transmitting the"]
        #[doc = " HANDSHAKE_DONE frame until it is acked by the peer."]
        HandshakeDoneLost {},
    }
    impl aggregate::AsVariant for HandshakeStatus {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("COMPLETE\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CONFIRMED\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("HANDSHAKE_DONE_ACKED\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("HANDSHAKE_DONE_LOST\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Complete { .. } => 0usize,
                Self::Confirmed { .. } => 1usize,
                Self::HandshakeDoneAcked { .. } => 2usize,
                Self::HandshakeDoneLost { .. } => 3usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The source that caused a congestion event"]
    pub enum CongestionSource {
        #[non_exhaustive]
        #[doc = " Explicit Congestion Notification"]
        Ecn {},
        #[non_exhaustive]
        #[doc = " One or more packets were detected lost"]
        PacketLoss {},
    }
    impl aggregate::AsVariant for CongestionSource {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ECN\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PACKET_LOSS\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Ecn { .. } => 0usize,
                Self::PacketLoss { .. } => 1usize,
            }
        }
    }
    #[non_exhaustive]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[allow(non_camel_case_types)]
    pub enum CipherSuite {
        #[non_exhaustive]
        TLS_AES_128_GCM_SHA256 {},
        #[non_exhaustive]
        TLS_AES_256_GCM_SHA384 {},
        #[non_exhaustive]
        TLS_CHACHA20_POLY1305_SHA256 {},
        #[non_exhaustive]
        Unknown {},
    }
    impl aggregate::AsVariant for CipherSuite {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("TLS_AES_128_GCM_SHA256\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("TLS_AES_256_GCM_SHA384\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("TLS_CHACHA20_POLY1305_SHA256\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNKNOWN\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::TLS_AES_128_GCM_SHA256 { .. } => 0usize,
                Self::TLS_AES_256_GCM_SHA384 { .. } => 1usize,
                Self::TLS_CHACHA20_POLY1305_SHA256 { .. } => 2usize,
                Self::Unknown { .. } => 3usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum PathChallengeStatus {
        #[non_exhaustive]
        Validated {},
        #[non_exhaustive]
        Abandoned {},
    }
    impl aggregate::AsVariant for PathChallengeStatus {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("VALIDATED\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ABANDONED\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Validated { .. } => 0usize,
                Self::Abandoned { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The reason the slow start congestion controller state has been exited"]
    pub enum SlowStartExitCause {
        #[non_exhaustive]
        #[doc = " A packet was determined lost"]
        PacketLoss {},
        #[non_exhaustive]
        #[doc = " An Explicit Congestion Notification: Congestion Experienced marking was received"]
        Ecn {},
        #[non_exhaustive]
        #[doc = " The round trip time estimate was updated"]
        Rtt {},
        #[non_exhaustive]
        #[doc = " Slow Start exited due to a reason other than those above"]
        #[doc = ""]
        #[doc = " With the Cubic congestion controller, this reason is used after the initial exiting of"]
        #[doc = " Slow Start, when the previously determined Slow Start threshold is exceed by the"]
        #[doc = " congestion window."]
        Other {},
    }
    impl aggregate::AsVariant for SlowStartExitCause {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PACKET_LOSS\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ECN\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("RTT\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("OTHER\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::PacketLoss { .. } => 0usize,
                Self::Ecn { .. } => 1usize,
                Self::Rtt { .. } => 2usize,
                Self::Other { .. } => 3usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The reason the MTU was updated"]
    pub enum MtuUpdatedCause {
        #[non_exhaustive]
        #[doc = " The MTU was initialized with the default value"]
        NewPath {},
        #[non_exhaustive]
        #[doc = " An MTU probe was acknowledged by the peer"]
        ProbeAcknowledged {},
        #[non_exhaustive]
        #[doc = " A blackhole was detected"]
        Blackhole {},
        #[non_exhaustive]
        #[doc = " An early packet using the configured InitialMtu was lost"]
        InitialMtuPacketLost {},
        #[non_exhaustive]
        #[doc = " An early packet using the configured InitialMtu was acknowledged by the peer"]
        InitialMtuPacketAcknowledged {},
        #[non_exhaustive]
        #[doc = " MTU probes larger than the current MTU were not acknowledged"]
        LargerProbesLost {},
    }
    impl aggregate::AsVariant for MtuUpdatedCause {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("NEW_PATH\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PROBE_ACKNOWLEDGED\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("BLACKHOLE\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INITIAL_MTU_PACKET_LOST\0"),
                id: 3usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INITIAL_MTU_PACKET_ACKNOWLEDGED\0"),
                id: 4usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("LARGER_PROBES_LOST\0"),
                id: 5usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::NewPath { .. } => 0usize,
                Self::ProbeAcknowledged { .. } => 1usize,
                Self::Blackhole { .. } => 2usize,
                Self::InitialMtuPacketLost { .. } => 3usize,
                Self::InitialMtuPacketAcknowledged { .. } => 4usize,
                Self::LargerProbesLost { .. } => 5usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum BbrState {
        #[non_exhaustive]
        Startup {},
        #[non_exhaustive]
        Drain {},
        #[non_exhaustive]
        ProbeBwDown {},
        #[non_exhaustive]
        ProbeBwCruise {},
        #[non_exhaustive]
        ProbeBwRefill {},
        #[non_exhaustive]
        ProbeBwUp {},
        #[non_exhaustive]
        ProbeRtt {},
    }
    impl aggregate::AsVariant for BbrState {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("STARTUP\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("DRAIN\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PROBE_BW_DOWN\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PROBE_BW_CRUISE\0"),
                id: 3usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PROBE_BW_REFILL\0"),
                id: 4usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PROBE_BW_UP\0"),
                id: 5usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PROBE_RTT\0"),
                id: 6usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Startup { .. } => 0usize,
                Self::Drain { .. } => 1usize,
                Self::ProbeBwDown { .. } => 2usize,
                Self::ProbeBwCruise { .. } => 3usize,
                Self::ProbeBwRefill { .. } => 4usize,
                Self::ProbeBwUp { .. } => 5usize,
                Self::ProbeRtt { .. } => 6usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum DcState {
        #[non_exhaustive]
        VersionNegotiated { version: u32 },
        #[non_exhaustive]
        NoVersionNegotiated {},
        #[non_exhaustive]
        PathSecretsReady {},
        #[non_exhaustive]
        Complete {},
    }
    impl aggregate::AsVariant for DcState {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("VERSION_NEGOTIATED\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("NO_VERSION_NEGOTIATED\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("PATH_SECRETS_READY\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("COMPLETE\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::VersionNegotiated { .. } => 0usize,
                Self::NoVersionNegotiated { .. } => 1usize,
                Self::PathSecretsReady { .. } => 2usize,
                Self::Complete { .. } => 3usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Application level protocol"]
    pub struct ApplicationProtocolInformation<'a> {
        pub chosen_application_protocol: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ApplicationProtocolInformation<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ApplicationProtocolInformation");
            fmt.field(
                "chosen_application_protocol",
                &self.chosen_application_protocol,
            );
            fmt.finish()
        }
    }
    impl<'a> Event for ApplicationProtocolInformation<'a> {
        const NAME: &'static str = "transport:application_protocol_information";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Server Name was negotiated for the connection"]
    pub struct ServerNameInformation<'a> {
        pub chosen_server_name: &'a str,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ServerNameInformation<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ServerNameInformation");
            fmt.field("chosen_server_name", &self.chosen_server_name);
            fmt.finish()
        }
    }
    impl<'a> Event for ServerNameInformation<'a> {
        const NAME: &'static str = "transport:server_name_information";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Key Exchange Group was negotiated for the connection"]
    #[doc = ""]
    #[doc = " `contains_kem` is `true` if the `chosen_group_name`"]
    #[doc = " contains a key encapsulation mechanism"]
    pub struct KeyExchangeGroup<'a> {
        pub chosen_group_name: &'a str,
        pub contains_kem: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for KeyExchangeGroup<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("KeyExchangeGroup");
            fmt.field("chosen_group_name", &self.chosen_group_name);
            fmt.field("contains_kem", &self.contains_kem);
            fmt.finish()
        }
    }
    impl<'a> Event for KeyExchangeGroup<'a> {
        const NAME: &'static str = "transport:key_exchange_group";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Packet was skipped with a given reason"]
    pub struct PacketSkipped {
        pub number: u64,
        pub space: KeySpace,
        pub reason: PacketSkipReason,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PacketSkipped {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PacketSkipped");
            fmt.field("number", &self.number);
            fmt.field("space", &self.space);
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    impl Event for PacketSkipped {
        const NAME: &'static str = "transport:packet_skipped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Packet was sent by a connection"]
    pub struct PacketSent {
        pub packet_header: PacketHeader,
        pub packet_len: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PacketSent {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PacketSent");
            fmt.field("packet_header", &self.packet_header);
            fmt.field("packet_len", &self.packet_len);
            fmt.finish()
        }
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
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PacketReceived {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PacketReceived");
            fmt.field("packet_header", &self.packet_header);
            fmt.finish()
        }
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
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ActivePathUpdated<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ActivePathUpdated");
            fmt.field("previous", &self.previous);
            fmt.field("active", &self.active);
            fmt.finish()
        }
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
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathCreated<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathCreated");
            fmt.field("active", &self.active);
            fmt.field("new", &self.new);
            fmt.finish()
        }
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
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for FrameSent {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("FrameSent");
            fmt.field("packet_header", &self.packet_header);
            fmt.field("path_id", &self.path_id);
            fmt.field("frame", &self.frame);
            fmt.finish()
        }
    }
    impl Event for FrameSent {
        const NAME: &'static str = "transport:frame_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Frame was received"]
    pub struct FrameReceived<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub frame: Frame,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for FrameReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("FrameReceived");
            fmt.field("packet_header", &self.packet_header);
            fmt.field("path", &self.path);
            fmt.field("frame", &self.frame);
            fmt.finish()
        }
    }
    impl<'a> Event for FrameReceived<'a> {
        const NAME: &'static str = "transport:frame_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " A `CONNECTION_CLOSE` frame was received"]
    #[doc = ""]
    #[doc = " This event includes additional details from the frame, particularly the"]
    #[doc = " reason (if provided) the peer closed the connection"]
    pub struct ConnectionCloseFrameReceived<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub frame: ConnectionCloseFrame<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ConnectionCloseFrameReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionCloseFrameReceived");
            fmt.field("packet_header", &self.packet_header);
            fmt.field("path", &self.path);
            fmt.field("frame", &self.frame);
            fmt.finish()
        }
    }
    impl<'a> Event for ConnectionCloseFrameReceived<'a> {
        const NAME: &'static str = "transport:connection_close_frame_received";
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
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PacketLost<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PacketLost");
            fmt.field("packet_header", &self.packet_header);
            fmt.field("path", &self.path);
            fmt.field("bytes_lost", &self.bytes_lost);
            fmt.field("is_mtu_probe", &self.is_mtu_probe);
            fmt.finish()
        }
    }
    impl<'a> Event for PacketLost<'a> {
        const NAME: &'static str = "recovery:packet_lost";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Recovery metrics updated"]
    pub struct RecoveryMetrics<'a> {
        pub path: Path<'a>,
        pub min_rtt: Duration,
        pub smoothed_rtt: Duration,
        pub latest_rtt: Duration,
        pub rtt_variance: Duration,
        pub max_ack_delay: Duration,
        pub pto_count: u32,
        pub congestion_window: u32,
        pub bytes_in_flight: u32,
        pub congestion_limited: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for RecoveryMetrics<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("RecoveryMetrics");
            fmt.field("path", &self.path);
            fmt.field("min_rtt", &self.min_rtt);
            fmt.field("smoothed_rtt", &self.smoothed_rtt);
            fmt.field("latest_rtt", &self.latest_rtt);
            fmt.field("rtt_variance", &self.rtt_variance);
            fmt.field("max_ack_delay", &self.max_ack_delay);
            fmt.field("pto_count", &self.pto_count);
            fmt.field("congestion_window", &self.congestion_window);
            fmt.field("bytes_in_flight", &self.bytes_in_flight);
            fmt.field("congestion_limited", &self.congestion_limited);
            fmt.finish()
        }
    }
    impl<'a> Event for RecoveryMetrics<'a> {
        const NAME: &'static str = "recovery:metrics_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Congestion (ECN or packet loss) has occurred"]
    pub struct Congestion<'a> {
        pub path: Path<'a>,
        pub source: CongestionSource,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for Congestion<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("Congestion");
            fmt.field("path", &self.path);
            fmt.field("source", &self.source);
            fmt.finish()
        }
    }
    impl<'a> Event for Congestion<'a> {
        const NAME: &'static str = "recovery:congestion";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Events related to ACK processing"]
    #[deprecated(note = "use on_rx_ack_range_dropped event instead")]
    #[allow(deprecated)]
    pub struct AckProcessed<'a> {
        pub action: AckAction,
        pub path: Path<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    #[allow(deprecated)]
    impl<'a> crate::event::snapshot::Fmt for AckProcessed<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AckProcessed");
            fmt.field("action", &self.action);
            fmt.field("path", &self.path);
            fmt.finish()
        }
    }
    #[allow(deprecated)]
    impl<'a> Event for AckProcessed<'a> {
        const NAME: &'static str = "recovery:ack_processed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Ack range for received packets was dropped due to space constraints"]
    #[doc = ""]
    #[doc = " For the purpose of processing Acks, RX packet numbers are stored as"]
    #[doc = " packet_number ranges in an IntervalSet; only lower and upper bounds"]
    #[doc = " are stored instead of individual packet_numbers. Ranges are merged"]
    #[doc = " when possible so only disjointed ranges are stored."]
    #[doc = ""]
    #[doc = " When at `capacity`, the lowest packet_number range is dropped."]
    pub struct RxAckRangeDropped<'a> {
        pub path: Path<'a>,
        #[doc = " The packet number range which was dropped"]
        pub packet_number_range: core::ops::RangeInclusive<u64>,
        #[doc = " The number of disjoint ranges the IntervalSet can store"]
        pub capacity: usize,
        #[doc = " The store packet_number range in the IntervalSet"]
        pub stored_range: core::ops::RangeInclusive<u64>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for RxAckRangeDropped<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("RxAckRangeDropped");
            fmt.field("path", &self.path);
            fmt.field("packet_number_range", &self.packet_number_range);
            fmt.field("capacity", &self.capacity);
            fmt.field("stored_range", &self.stored_range);
            fmt.finish()
        }
    }
    impl<'a> Event for RxAckRangeDropped<'a> {
        const NAME: &'static str = "recovery:rx_ack_range_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " ACK range was received"]
    pub struct AckRangeReceived<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub ack_range: RangeInclusive<u64>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AckRangeReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AckRangeReceived");
            fmt.field("packet_header", &self.packet_header);
            fmt.field("path", &self.path);
            fmt.field("ack_range", &self.ack_range);
            fmt.finish()
        }
    }
    impl<'a> Event for AckRangeReceived<'a> {
        const NAME: &'static str = "recovery:ack_range_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " ACK range was sent"]
    pub struct AckRangeSent {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub ack_range: RangeInclusive<u64>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for AckRangeSent {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AckRangeSent");
            fmt.field("packet_header", &self.packet_header);
            fmt.field("path_id", &self.path_id);
            fmt.field("ack_range", &self.ack_range);
            fmt.finish()
        }
    }
    impl Event for AckRangeSent {
        const NAME: &'static str = "recovery:ack_range_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Packet was dropped with the given reason"]
    pub struct PacketDropped<'a> {
        pub reason: PacketDropReason<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PacketDropped<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PacketDropped");
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    impl<'a> Event for PacketDropped<'a> {
        const NAME: &'static str = "transport:packet_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Crypto key updated"]
    pub struct KeyUpdate {
        pub key_type: KeyType,
        pub cipher_suite: CipherSuite,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for KeyUpdate {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("KeyUpdate");
            fmt.field("key_type", &self.key_type);
            fmt.field("cipher_suite", &self.cipher_suite);
            fmt.finish()
        }
    }
    impl Event for KeyUpdate {
        const NAME: &'static str = "security:key_update";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct KeySpaceDiscarded {
        pub space: KeySpace,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for KeySpaceDiscarded {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("KeySpaceDiscarded");
            fmt.field("space", &self.space);
            fmt.finish()
        }
    }
    impl Event for KeySpaceDiscarded {
        const NAME: &'static str = "security:key_space_discarded";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Connection started"]
    pub struct ConnectionStarted<'a> {
        pub path: Path<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ConnectionStarted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionStarted");
            fmt.field("path", &self.path);
            fmt.finish()
        }
    }
    impl<'a> Event for ConnectionStarted<'a> {
        const NAME: &'static str = "connectivity:connection_started";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Duplicate packet received"]
    pub struct DuplicatePacket<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub error: DuplicatePacketError,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for DuplicatePacket<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("DuplicatePacket");
            fmt.field("packet_header", &self.packet_header);
            fmt.field("path", &self.path);
            fmt.field("error", &self.error);
            fmt.finish()
        }
    }
    impl<'a> Event for DuplicatePacket<'a> {
        const NAME: &'static str = "transport:duplicate_packet";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Transport parameters received by connection"]
    pub struct TransportParametersReceived<'a> {
        pub transport_parameters: TransportParameters<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for TransportParametersReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("TransportParametersReceived");
            fmt.field("transport_parameters", &self.transport_parameters);
            fmt.finish()
        }
    }
    impl<'a> Event for TransportParametersReceived<'a> {
        const NAME: &'static str = "transport:transport_parameters_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram sent by a connection"]
    pub struct DatagramSent {
        pub len: u16,
        #[doc = " The GSO offset at which this datagram was written"]
        #[doc = ""]
        #[doc = " If this value is greater than 0, it indicates that this datagram has been sent with other"]
        #[doc = " segments in a single buffer."]
        #[doc = ""]
        #[doc = " See the [Linux kernel documentation](https://www.kernel.org/doc/html/latest/networking/segmentation-offloads.html#generic-segmentation-offload) for more details."]
        pub gso_offset: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for DatagramSent {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("DatagramSent");
            fmt.field("len", &self.len);
            fmt.field("gso_offset", &self.gso_offset);
            fmt.finish()
        }
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
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for DatagramReceived {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("DatagramReceived");
            fmt.field("len", &self.len);
            fmt.finish()
        }
    }
    impl Event for DatagramReceived {
        const NAME: &'static str = "transport:datagram_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram dropped by a connection"]
    pub struct DatagramDropped<'a> {
        pub local_addr: SocketAddress<'a>,
        pub remote_addr: SocketAddress<'a>,
        pub destination_cid: ConnectionId<'a>,
        pub source_cid: Option<ConnectionId<'a>>,
        pub len: u16,
        pub reason: DatagramDropReason,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for DatagramDropped<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("DatagramDropped");
            fmt.field("local_addr", &self.local_addr);
            fmt.field("remote_addr", &self.remote_addr);
            fmt.field("destination_cid", &self.destination_cid);
            fmt.field("source_cid", &self.source_cid);
            fmt.field("len", &self.len);
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    impl<'a> Event for DatagramDropped<'a> {
        const NAME: &'static str = "transport:datagram_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The remote address was changed before the handshake was complete"]
    pub struct HandshakeRemoteAddressChangeObserved<'a> {
        pub local_addr: SocketAddress<'a>,
        #[doc = " The newly observed remote address"]
        pub remote_addr: SocketAddress<'a>,
        #[doc = " The remote address established from the initial packet"]
        pub initial_remote_addr: SocketAddress<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for HandshakeRemoteAddressChangeObserved<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("HandshakeRemoteAddressChangeObserved");
            fmt.field("local_addr", &self.local_addr);
            fmt.field("remote_addr", &self.remote_addr);
            fmt.field("initial_remote_addr", &self.initial_remote_addr);
            fmt.finish()
        }
    }
    impl<'a> Event for HandshakeRemoteAddressChangeObserved<'a> {
        const NAME: &'static str = "transport:handshake_remote_address_change_observed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " ConnectionId updated"]
    pub struct ConnectionIdUpdated<'a> {
        pub path_id: u64,
        #[doc = " The endpoint that updated its connection id"]
        pub cid_consumer: crate::endpoint::Location,
        pub previous: ConnectionId<'a>,
        pub current: ConnectionId<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ConnectionIdUpdated<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionIdUpdated");
            fmt.field("path_id", &self.path_id);
            fmt.field("cid_consumer", &self.cid_consumer);
            fmt.field("previous", &self.previous);
            fmt.field("current", &self.current);
            fmt.finish()
        }
    }
    impl<'a> Event for ConnectionIdUpdated<'a> {
        const NAME: &'static str = "connectivity:connection_id_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EcnStateChanged<'a> {
        pub path: Path<'a>,
        pub state: EcnState,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for EcnStateChanged<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EcnStateChanged");
            fmt.field("path", &self.path);
            fmt.field("state", &self.state);
            fmt.finish()
        }
    }
    impl<'a> Event for EcnStateChanged<'a> {
        const NAME: &'static str = "recovery:ecn_state_changed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionMigrationDenied {
        pub reason: MigrationDenyReason,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionMigrationDenied {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionMigrationDenied");
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    impl Event for ConnectionMigrationDenied {
        const NAME: &'static str = "connectivity:connection_migration_denied";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct HandshakeStatusUpdated {
        pub status: HandshakeStatus,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for HandshakeStatusUpdated {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("HandshakeStatusUpdated");
            fmt.field("status", &self.status);
            fmt.finish()
        }
    }
    impl Event for HandshakeStatusUpdated {
        const NAME: &'static str = "connectivity:handshake_status_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct TlsExporterReady<'a> {
        pub session: crate::event::TlsSession<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for TlsExporterReady<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("TlsExporterReady");
            fmt.field("session", &self.session);
            fmt.finish()
        }
    }
    impl<'a> Event for TlsExporterReady<'a> {
        const NAME: &'static str = "connectivity:tls_exporter_ready";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct TlsHandshakeFailed<'a> {
        pub session: crate::event::TlsSession<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for TlsHandshakeFailed<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("TlsHandshakeFailed");
            fmt.field("session", &self.session);
            fmt.finish()
        }
    }
    impl<'a> Event for TlsHandshakeFailed<'a> {
        const NAME: &'static str = "connectivity:tls_handshake_failed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Path challenge updated"]
    pub struct PathChallengeUpdated<'a> {
        pub path_challenge_status: PathChallengeStatus,
        pub path: Path<'a>,
        pub challenge_data: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathChallengeUpdated<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathChallengeUpdated");
            fmt.field("path_challenge_status", &self.path_challenge_status);
            fmt.field("path", &self.path);
            fmt.field("challenge_data", &self.challenge_data);
            fmt.finish()
        }
    }
    impl<'a> Event for PathChallengeUpdated<'a> {
        const NAME: &'static str = "connectivity:path_challenge_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct TlsClientHello<'a> {
        pub payload: &'a [&'a [u8]],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for TlsClientHello<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("TlsClientHello");
            fmt.field("payload", &self.payload);
            fmt.finish()
        }
    }
    impl<'a> Event for TlsClientHello<'a> {
        const NAME: &'static str = "tls:client_hello";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct TlsServerHello<'a> {
        pub payload: &'a [&'a [u8]],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for TlsServerHello<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("TlsServerHello");
            fmt.field("payload", &self.payload);
            fmt.finish()
        }
    }
    impl<'a> Event for TlsServerHello<'a> {
        const NAME: &'static str = "tls:server_hello";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct RxStreamProgress {
        pub bytes: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for RxStreamProgress {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("RxStreamProgress");
            fmt.field("bytes", &self.bytes);
            fmt.finish()
        }
    }
    impl Event for RxStreamProgress {
        const NAME: &'static str = "transport:rx_stream_progress";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct TxStreamProgress {
        pub bytes: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for TxStreamProgress {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("TxStreamProgress");
            fmt.field("bytes", &self.bytes);
            fmt.finish()
        }
    }
    impl Event for TxStreamProgress {
        const NAME: &'static str = "transport:tx_stream_progress";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct KeepAliveTimerExpired {
        pub timeout: Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for KeepAliveTimerExpired {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("KeepAliveTimerExpired");
            fmt.field("timeout", &self.timeout);
            fmt.finish()
        }
    }
    impl Event for KeepAliveTimerExpired {
        const NAME: &'static str = "connectivity::keep_alive_timer_expired";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The maximum transmission unit (MTU) and/or MTU probing status for the path has changed"]
    pub struct MtuUpdated {
        pub path_id: u64,
        #[doc = " The maximum QUIC datagram size, not including UDP and IP headers"]
        pub mtu: u16,
        pub cause: MtuUpdatedCause,
        #[doc = " The search for the maximum MTU has completed for now"]
        pub search_complete: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for MtuUpdated {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("MtuUpdated");
            fmt.field("path_id", &self.path_id);
            fmt.field("mtu", &self.mtu);
            fmt.field("cause", &self.cause);
            fmt.field("search_complete", &self.search_complete);
            fmt.finish()
        }
    }
    impl Event for MtuUpdated {
        const NAME: &'static str = "connectivity:mtu_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The slow start congestion controller state has been exited"]
    pub struct SlowStartExited {
        pub path_id: u64,
        pub cause: SlowStartExitCause,
        pub congestion_window: u32,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for SlowStartExited {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("SlowStartExited");
            fmt.field("path_id", &self.path_id);
            fmt.field("cause", &self.cause);
            fmt.field("congestion_window", &self.congestion_window);
            fmt.finish()
        }
    }
    impl Event for SlowStartExited {
        const NAME: &'static str = "recovery:slow_start_exited";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " A new delivery rate sample has been generated"]
    #[doc = " Note: This event is only recorded for congestion controllers that support"]
    #[doc = "       bandwidth estimates, such as BBR"]
    pub struct DeliveryRateSampled {
        pub path_id: u64,
        pub rate_sample: RateSample,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for DeliveryRateSampled {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("DeliveryRateSampled");
            fmt.field("path_id", &self.path_id);
            fmt.field("rate_sample", &self.rate_sample);
            fmt.finish()
        }
    }
    impl Event for DeliveryRateSampled {
        const NAME: &'static str = "recovery:delivery_rate_sampled";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The pacing rate has been updated"]
    pub struct PacingRateUpdated {
        pub path_id: u64,
        pub bytes_per_second: u64,
        pub burst_size: u32,
        pub pacing_gain: f32,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PacingRateUpdated {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PacingRateUpdated");
            fmt.field("path_id", &self.path_id);
            fmt.field("bytes_per_second", &self.bytes_per_second);
            fmt.field("burst_size", &self.burst_size);
            fmt.field("pacing_gain", &self.pacing_gain);
            fmt.finish()
        }
    }
    impl Event for PacingRateUpdated {
        const NAME: &'static str = "recovery:pacing_rate_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The BBR state has changed"]
    pub struct BbrStateChanged {
        pub path_id: u64,
        pub state: BbrState,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for BbrStateChanged {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("BbrStateChanged");
            fmt.field("path_id", &self.path_id);
            fmt.field("state", &self.state);
            fmt.finish()
        }
    }
    impl Event for BbrStateChanged {
        const NAME: &'static str = "recovery:bbr_state_changed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The DC state has changed"]
    pub struct DcStateChanged {
        pub state: DcState,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for DcStateChanged {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("DcStateChanged");
            fmt.field("state", &self.state);
            fmt.finish()
        }
    }
    impl Event for DcStateChanged {
        const NAME: &'static str = "transport:dc_state_changed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " The DC path has been created"]
    pub struct DcPathCreated<'a> {
        #[doc = " This is the dc::Path struct, it's just type-erased. But if an event subscriber knows the"]
        #[doc = " type they can downcast."]
        pub path: &'a (dyn core::any::Any + Send + 'static),
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for DcPathCreated<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("DcPathCreated");
            fmt.field("path", &self.path);
            fmt.finish()
        }
    }
    impl<'a> Event for DcPathCreated<'a> {
        const NAME: &'static str = "transport:dc_path_created";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Connection closed"]
    pub struct ConnectionClosed {
        pub error: crate::connection::Error,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionClosed {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionClosed");
            fmt.field("error", &self.error);
            fmt.finish()
        }
    }
    impl Event for ConnectionClosed {
        const NAME: &'static str = "connectivity:connection_closed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " QUIC version"]
    pub struct VersionInformation<'a> {
        pub server_versions: &'a [u32],
        pub client_versions: &'a [u32],
        pub chosen_version: Option<u32>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for VersionInformation<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("VersionInformation");
            fmt.field("server_versions", &self.server_versions);
            fmt.field("client_versions", &self.client_versions);
            fmt.field("chosen_version", &self.chosen_version);
            fmt.finish()
        }
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
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointPacketSent {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointPacketSent");
            fmt.field("packet_header", &self.packet_header);
            fmt.finish()
        }
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
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointPacketReceived {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointPacketReceived");
            fmt.field("packet_header", &self.packet_header);
            fmt.finish()
        }
    }
    impl Event for EndpointPacketReceived {
        const NAME: &'static str = "transport:packet_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram sent by the endpoint"]
    pub struct EndpointDatagramSent {
        pub len: u16,
        #[doc = " The GSO offset at which this datagram was written"]
        #[doc = ""]
        #[doc = " If this value is greater than 0, it indicates that this datagram has been sent with other"]
        #[doc = " segments in a single buffer."]
        #[doc = ""]
        #[doc = " See the [Linux kernel documentation](https://www.kernel.org/doc/html/latest/networking/segmentation-offloads.html#generic-segmentation-offload) for more details."]
        pub gso_offset: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointDatagramSent {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointDatagramSent");
            fmt.field("len", &self.len);
            fmt.field("gso_offset", &self.gso_offset);
            fmt.finish()
        }
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
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointDatagramReceived {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointDatagramReceived");
            fmt.field("len", &self.len);
            fmt.finish()
        }
    }
    impl Event for EndpointDatagramReceived {
        const NAME: &'static str = "transport:datagram_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Datagram dropped by the endpoint"]
    pub struct EndpointDatagramDropped {
        pub len: u16,
        pub reason: DatagramDropReason,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointDatagramDropped {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointDatagramDropped");
            fmt.field("len", &self.len);
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    impl Event for EndpointDatagramDropped {
        const NAME: &'static str = "transport:datagram_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointConnectionAttemptFailed {
        pub error: crate::connection::Error,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointConnectionAttemptFailed {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointConnectionAttemptFailed");
            fmt.field("error", &self.error);
            fmt.finish()
        }
    }
    impl Event for EndpointConnectionAttemptFailed {
        const NAME: &'static str = "transport:connection_attempt_failed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointConnectionAttemptDeduplicated {
        #[doc = " The internal connection ID this deduplicated with."]
        pub connection_id: u64,
        pub already_open: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointConnectionAttemptDeduplicated {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointConnectionAttemptDeduplicated");
            fmt.field("connection_id", &self.connection_id);
            fmt.field("already_open", &self.already_open);
            fmt.finish()
        }
    }
    impl Event for EndpointConnectionAttemptDeduplicated {
        const NAME: &'static str = "endpoint:connection_attempt_deduplicated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the platform sends at least one packet"]
    pub struct PlatformTx {
        #[doc = " The number of packets sent"]
        pub count: usize,
        #[doc = " The number of syscalls performed"]
        pub syscalls: usize,
        #[doc = " The number of syscalls that got blocked"]
        pub blocked_syscalls: usize,
        #[doc = " The total number of errors encountered since the last event"]
        pub total_errors: usize,
        #[doc = " The number of specific error codes dropped"]
        #[doc = ""]
        #[doc = " This can happen when a burst of errors exceeds the capacity of the recorder"]
        pub dropped_errors: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PlatformTx {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PlatformTx");
            fmt.field("count", &self.count);
            fmt.field("syscalls", &self.syscalls);
            fmt.field("blocked_syscalls", &self.blocked_syscalls);
            fmt.field("total_errors", &self.total_errors);
            fmt.field("dropped_errors", &self.dropped_errors);
            fmt.finish()
        }
    }
    impl Event for PlatformTx {
        const NAME: &'static str = "platform:tx";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the platform returns an error while sending datagrams"]
    pub struct PlatformTxError {
        #[doc = " The error code returned by the platform"]
        pub errno: i32,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PlatformTxError {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PlatformTxError");
            fmt.field("errno", &self.errno);
            fmt.finish()
        }
    }
    impl Event for PlatformTxError {
        const NAME: &'static str = "platform:tx_error";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the platform receives at least one packet"]
    pub struct PlatformRx {
        #[doc = " The number of packets received"]
        pub count: usize,
        #[doc = " The number of syscalls performed"]
        pub syscalls: usize,
        #[doc = " The number of syscalls that got blocked"]
        pub blocked_syscalls: usize,
        #[doc = " The total number of errors encountered since the last event"]
        pub total_errors: usize,
        #[doc = " The number of specific error codes dropped"]
        #[doc = ""]
        #[doc = " This can happen when a burst of errors exceeds the capacity of the recorder"]
        pub dropped_errors: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PlatformRx {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PlatformRx");
            fmt.field("count", &self.count);
            fmt.field("syscalls", &self.syscalls);
            fmt.field("blocked_syscalls", &self.blocked_syscalls);
            fmt.field("total_errors", &self.total_errors);
            fmt.field("dropped_errors", &self.dropped_errors);
            fmt.finish()
        }
    }
    impl Event for PlatformRx {
        const NAME: &'static str = "platform:rx";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the platform returns an error while receiving datagrams"]
    pub struct PlatformRxError {
        #[doc = " The error code returned by the platform"]
        pub errno: i32,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PlatformRxError {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PlatformRxError");
            fmt.field("errno", &self.errno);
            fmt.finish()
        }
    }
    impl Event for PlatformRxError {
        const NAME: &'static str = "platform:rx_error";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a platform feature is configured"]
    pub struct PlatformFeatureConfigured {
        pub configuration: PlatformFeatureConfiguration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PlatformFeatureConfigured {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PlatformFeatureConfigured");
            fmt.field("configuration", &self.configuration);
            fmt.finish()
        }
    }
    impl Event for PlatformFeatureConfigured {
        const NAME: &'static str = "platform:feature_configured";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct PlatformEventLoopWakeup {
        pub timeout_expired: bool,
        pub rx_ready: bool,
        pub tx_ready: bool,
        pub application_wakeup: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PlatformEventLoopWakeup {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PlatformEventLoopWakeup");
            fmt.field("timeout_expired", &self.timeout_expired);
            fmt.field("rx_ready", &self.rx_ready);
            fmt.field("tx_ready", &self.tx_ready);
            fmt.field("application_wakeup", &self.application_wakeup);
            fmt.finish()
        }
    }
    impl Event for PlatformEventLoopWakeup {
        const NAME: &'static str = "platform:event_loop_wakeup";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct PlatformEventLoopSleep {
        #[doc = " The next time at which the event loop will wake"]
        pub timeout: Option<core::time::Duration>,
        #[doc = " The amount of time spent processing endpoint events in a single event loop"]
        pub processing_duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PlatformEventLoopSleep {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PlatformEventLoopSleep");
            fmt.field("timeout", &self.timeout);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.finish()
        }
    }
    impl Event for PlatformEventLoopSleep {
        const NAME: &'static str = "platform:event_loop_sleep";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct PlatformEventLoopStarted<'a> {
        #[doc = " The local address of the socket"]
        pub local_address: SocketAddress<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PlatformEventLoopStarted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PlatformEventLoopStarted");
            fmt.field("local_address", &self.local_address);
            fmt.finish()
        }
    }
    impl<'a> Event for PlatformEventLoopStarted<'a> {
        const NAME: &'static str = "platform:started";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum PlatformFeatureConfiguration {
        #[non_exhaustive]
        #[doc = " Emitted when segment offload was configured"]
        Gso {
            #[doc = " The maximum number of segments that can be sent in a single GSO packet"]
            #[doc = ""]
            #[doc = " If this value not greater than 1, GSO is disabled."]
            max_segments: usize,
        },
        #[non_exhaustive]
        #[doc = " Emitted when receive segment offload was configured"]
        Gro { enabled: bool },
        #[non_exhaustive]
        #[doc = " Emitted when ECN support is configured"]
        Ecn { enabled: bool },
        #[non_exhaustive]
        #[doc = " Emitted when the base maximum transmission unit is configured"]
        BaseMtu { mtu: u16 },
        #[non_exhaustive]
        #[doc = " Emitted when the initial maximum transmission unit is configured"]
        InitialMtu { mtu: u16 },
        #[non_exhaustive]
        #[doc = " Emitted when the max maximum transmission unit is configured"]
        MaxMtu { mtu: u16 },
    }
    impl aggregate::AsVariant for PlatformFeatureConfiguration {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("GSO\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("GRO\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ECN\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("BASE_MTU\0"),
                id: 3usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INITIAL_MTU\0"),
                id: 4usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("MAX_MTU\0"),
                id: 5usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Gso { .. } => 0usize,
                Self::Gro { .. } => 1usize,
                Self::Ecn { .. } => 2usize,
                Self::BaseMtu { .. } => 3usize,
                Self::InitialMtu { .. } => 4usize,
                Self::MaxMtu { .. } => 5usize,
            }
        }
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
    impl<'a> IntoEvent<builder::Frame> for &crate::frame::DcStatelessResetTokens<'a> {
        #[inline]
        fn into_event(self) -> builder::Frame {
            builder::Frame::DcStatelessResetTokens {}
        }
    }
    #[cfg(feature = "alloc")]
    impl<'a> ConnectionCloseFrame<'a> {
        #[doc = " Converts the reason to a UTF-8 `str`, including invalid characters"]
        pub fn reason_lossy_utf8(&self) -> Option<alloc::borrow::Cow<'a, str>> {
            self.reason
                .map(|reason| alloc::string::String::from_utf8_lossy(reason))
        }
    }
    impl<'a> IntoEvent<builder::ConnectionCloseFrame<'a>> for &crate::frame::ConnectionClose<'a> {
        #[inline]
        fn into_event(self) -> builder::ConnectionCloseFrame<'a> {
            builder::ConnectionCloseFrame {
                error_code: self.error_code.as_u64(),
                frame_type: self.frame_type.into_event(),
                reason: self.reason.into_event(),
            }
        }
    }
    #[cfg(feature = "alloc")]
    impl<'a> core::fmt::Debug for ConnectionCloseFrame<'a> {
        fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
            f.debug_struct("ConnectionCloseFrame")
                .field("error_code", &self.error_code)
                .field("frame_type", &self.frame_type)
                .field("reason", &self.reason_lossy_utf8())
                .finish()
        }
    }
    #[cfg(not(feature = "alloc"))]
    impl<'a> core::fmt::Debug for ConnectionCloseFrame<'a> {
        fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
            write!(f, "{:?}", self)
        }
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
    impl<'a> IntoEvent<builder::MtuConfig> for &'a crate::path::mtu::Config {
        #[inline]
        fn into_event(self) -> builder::MtuConfig {
            builder::MtuConfig {
                initial_mtu: self.initial_mtu().into(),
                base_mtu: self.base_mtu().into(),
                max_mtu: self.max_mtu().into(),
            }
        }
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
    #[cfg(feature = "std")]
    impl From<PlatformTxError> for std::io::Error {
        fn from(error: PlatformTxError) -> Self {
            Self::from_raw_os_error(error.errno)
        }
    }
    #[cfg(feature = "std")]
    impl From<PlatformRxError> for std::io::Error {
        fn from(error: PlatformRxError) -> Self {
            Self::from_raw_os_error(error.errno)
        }
    }
}
#[cfg(feature = "event-tracing")]
pub mod tracing {
    #![doc = r" This module contains event integration with [`tracing`](https://docs.rs/tracing)"]
    use super::api;
    #[doc = r" Emits events with [`tracing`](https://docs.rs/tracing)"]
    #[derive(Clone, Debug)]
    pub struct Subscriber {
        client: tracing::Span,
        server: tracing::Span,
    }
    impl Default for Subscriber {
        fn default() -> Self {
            let root =
                tracing :: span ! (target : "s2n_quic" , tracing :: Level :: DEBUG , "s2n_quic");
            let client =
                tracing :: span ! (parent : root . id () , tracing :: Level :: DEBUG , "client");
            let server =
                tracing :: span ! (parent : root . id () , tracing :: Level :: DEBUG , "server");
            Self { client, server }
        }
    }
    impl Subscriber {
        fn parent<M: crate::event::Meta>(&self, meta: &M) -> Option<tracing::Id> {
            match meta.endpoint_type() {
                api::EndpointType::Client { .. } => self.client.id(),
                api::EndpointType::Server { .. } => self.server.id(),
            }
        }
    }
    impl super::Subscriber for Subscriber {
        type ConnectionContext = tracing::Span;
        fn create_connection_context(
            &mut self,
            meta: &api::ConnectionMeta,
            _info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            let parent = self.parent(meta);
            tracing :: span ! (target : "s2n_quic" , parent : parent , tracing :: Level :: DEBUG , "conn" , id = meta . id)
        }
        #[inline]
        fn on_application_protocol_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ApplicationProtocolInformation,
        ) {
            let id = context.id();
            let api::ApplicationProtocolInformation {
                chosen_application_protocol,
            } = event;
            tracing :: event ! (target : "application_protocol_information" , parent : id , tracing :: Level :: DEBUG , { chosen_application_protocol = tracing :: field :: debug (chosen_application_protocol) });
        }
        #[inline]
        fn on_server_name_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ServerNameInformation,
        ) {
            let id = context.id();
            let api::ServerNameInformation { chosen_server_name } = event;
            tracing :: event ! (target : "server_name_information" , parent : id , tracing :: Level :: DEBUG , { chosen_server_name = tracing :: field :: debug (chosen_server_name) });
        }
        #[inline]
        fn on_key_exchange_group(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::KeyExchangeGroup,
        ) {
            let id = context.id();
            let api::KeyExchangeGroup {
                chosen_group_name,
                contains_kem,
            } = event;
            tracing :: event ! (target : "key_exchange_group" , parent : id , tracing :: Level :: DEBUG , { chosen_group_name = tracing :: field :: debug (chosen_group_name) , contains_kem = tracing :: field :: debug (contains_kem) });
        }
        #[inline]
        fn on_packet_skipped(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::PacketSkipped,
        ) {
            let id = context.id();
            let api::PacketSkipped {
                number,
                space,
                reason,
            } = event;
            tracing :: event ! (target : "packet_skipped" , parent : id , tracing :: Level :: DEBUG , { number = tracing :: field :: debug (number) , space = tracing :: field :: debug (space) , reason = tracing :: field :: debug (reason) });
        }
        #[inline]
        fn on_packet_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::PacketSent,
        ) {
            let id = context.id();
            let api::PacketSent {
                packet_header,
                packet_len,
            } = event;
            tracing :: event ! (target : "packet_sent" , parent : id , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) , packet_len = tracing :: field :: debug (packet_len) });
        }
        #[inline]
        fn on_packet_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::PacketReceived,
        ) {
            let id = context.id();
            let api::PacketReceived { packet_header } = event;
            tracing :: event ! (target : "packet_received" , parent : id , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) });
        }
        #[inline]
        fn on_active_path_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ActivePathUpdated,
        ) {
            let id = context.id();
            let api::ActivePathUpdated { previous, active } = event;
            tracing :: event ! (target : "active_path_updated" , parent : id , tracing :: Level :: DEBUG , { previous = tracing :: field :: debug (previous) , active = tracing :: field :: debug (active) });
        }
        #[inline]
        fn on_path_created(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::PathCreated,
        ) {
            let id = context.id();
            let api::PathCreated { active, new } = event;
            tracing :: event ! (target : "path_created" , parent : id , tracing :: Level :: DEBUG , { active = tracing :: field :: debug (active) , new = tracing :: field :: debug (new) });
        }
        #[inline]
        fn on_frame_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::FrameSent,
        ) {
            let id = context.id();
            let api::FrameSent {
                packet_header,
                path_id,
                frame,
            } = event;
            tracing :: event ! (target : "frame_sent" , parent : id , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) , path_id = tracing :: field :: debug (path_id) , frame = tracing :: field :: debug (frame) });
        }
        #[inline]
        fn on_frame_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::FrameReceived,
        ) {
            let id = context.id();
            let api::FrameReceived {
                packet_header,
                path,
                frame,
            } = event;
            tracing :: event ! (target : "frame_received" , parent : id , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) , path = tracing :: field :: debug (path) , frame = tracing :: field :: debug (frame) });
        }
        #[inline]
        fn on_connection_close_frame_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ConnectionCloseFrameReceived,
        ) {
            let id = context.id();
            let api::ConnectionCloseFrameReceived {
                packet_header,
                path,
                frame,
            } = event;
            tracing :: event ! (target : "connection_close_frame_received" , parent : id , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) , path = tracing :: field :: debug (path) , frame = tracing :: field :: debug (frame) });
        }
        #[inline]
        fn on_packet_lost(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::PacketLost,
        ) {
            let id = context.id();
            let api::PacketLost {
                packet_header,
                path,
                bytes_lost,
                is_mtu_probe,
            } = event;
            tracing :: event ! (target : "packet_lost" , parent : id , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) , path = tracing :: field :: debug (path) , bytes_lost = tracing :: field :: debug (bytes_lost) , is_mtu_probe = tracing :: field :: debug (is_mtu_probe) });
        }
        #[inline]
        fn on_recovery_metrics(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::RecoveryMetrics,
        ) {
            let id = context.id();
            let api::RecoveryMetrics {
                path,
                min_rtt,
                smoothed_rtt,
                latest_rtt,
                rtt_variance,
                max_ack_delay,
                pto_count,
                congestion_window,
                bytes_in_flight,
                congestion_limited,
            } = event;
            tracing :: event ! (target : "recovery_metrics" , parent : id , tracing :: Level :: DEBUG , { path = tracing :: field :: debug (path) , min_rtt = tracing :: field :: debug (min_rtt) , smoothed_rtt = tracing :: field :: debug (smoothed_rtt) , latest_rtt = tracing :: field :: debug (latest_rtt) , rtt_variance = tracing :: field :: debug (rtt_variance) , max_ack_delay = tracing :: field :: debug (max_ack_delay) , pto_count = tracing :: field :: debug (pto_count) , congestion_window = tracing :: field :: debug (congestion_window) , bytes_in_flight = tracing :: field :: debug (bytes_in_flight) , congestion_limited = tracing :: field :: debug (congestion_limited) });
        }
        #[inline]
        fn on_congestion(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::Congestion,
        ) {
            let id = context.id();
            let api::Congestion { path, source } = event;
            tracing :: event ! (target : "congestion" , parent : id , tracing :: Level :: DEBUG , { path = tracing :: field :: debug (path) , source = tracing :: field :: debug (source) });
        }
        #[inline]
        #[allow(deprecated)]
        fn on_ack_processed(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::AckProcessed,
        ) {
            let id = context.id();
            let api::AckProcessed { action, path } = event;
            tracing :: event ! (target : "ack_processed" , parent : id , tracing :: Level :: DEBUG , { action = tracing :: field :: debug (action) , path = tracing :: field :: debug (path) });
        }
        #[inline]
        fn on_rx_ack_range_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::RxAckRangeDropped,
        ) {
            let id = context.id();
            let api::RxAckRangeDropped {
                path,
                packet_number_range,
                capacity,
                stored_range,
            } = event;
            tracing :: event ! (target : "rx_ack_range_dropped" , parent : id , tracing :: Level :: DEBUG , { path = tracing :: field :: debug (path) , packet_number_range = tracing :: field :: debug (packet_number_range) , capacity = tracing :: field :: debug (capacity) , stored_range = tracing :: field :: debug (stored_range) });
        }
        #[inline]
        fn on_ack_range_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::AckRangeReceived,
        ) {
            let id = context.id();
            let api::AckRangeReceived {
                packet_header,
                path,
                ack_range,
            } = event;
            tracing :: event ! (target : "ack_range_received" , parent : id , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) , path = tracing :: field :: debug (path) , ack_range = tracing :: field :: debug (ack_range) });
        }
        #[inline]
        fn on_ack_range_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::AckRangeSent,
        ) {
            let id = context.id();
            let api::AckRangeSent {
                packet_header,
                path_id,
                ack_range,
            } = event;
            tracing :: event ! (target : "ack_range_sent" , parent : id , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) , path_id = tracing :: field :: debug (path_id) , ack_range = tracing :: field :: debug (ack_range) });
        }
        #[inline]
        fn on_packet_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::PacketDropped,
        ) {
            let id = context.id();
            let api::PacketDropped { reason } = event;
            tracing :: event ! (target : "packet_dropped" , parent : id , tracing :: Level :: DEBUG , { reason = tracing :: field :: debug (reason) });
        }
        #[inline]
        fn on_key_update(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::KeyUpdate,
        ) {
            let id = context.id();
            let api::KeyUpdate {
                key_type,
                cipher_suite,
            } = event;
            tracing :: event ! (target : "key_update" , parent : id , tracing :: Level :: DEBUG , { key_type = tracing :: field :: debug (key_type) , cipher_suite = tracing :: field :: debug (cipher_suite) });
        }
        #[inline]
        fn on_key_space_discarded(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::KeySpaceDiscarded,
        ) {
            let id = context.id();
            let api::KeySpaceDiscarded { space } = event;
            tracing :: event ! (target : "key_space_discarded" , parent : id , tracing :: Level :: DEBUG , { space = tracing :: field :: debug (space) });
        }
        #[inline]
        fn on_connection_started(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ConnectionStarted,
        ) {
            let id = context.id();
            let api::ConnectionStarted { path } = event;
            tracing :: event ! (target : "connection_started" , parent : id , tracing :: Level :: DEBUG , { path = tracing :: field :: debug (path) });
        }
        #[inline]
        fn on_duplicate_packet(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::DuplicatePacket,
        ) {
            let id = context.id();
            let api::DuplicatePacket {
                packet_header,
                path,
                error,
            } = event;
            tracing :: event ! (target : "duplicate_packet" , parent : id , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) , path = tracing :: field :: debug (path) , error = tracing :: field :: debug (error) });
        }
        #[inline]
        fn on_transport_parameters_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::TransportParametersReceived,
        ) {
            let id = context.id();
            let api::TransportParametersReceived {
                transport_parameters,
            } = event;
            tracing :: event ! (target : "transport_parameters_received" , parent : id , tracing :: Level :: DEBUG , { transport_parameters = tracing :: field :: debug (transport_parameters) });
        }
        #[inline]
        fn on_datagram_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::DatagramSent,
        ) {
            let id = context.id();
            let api::DatagramSent { len, gso_offset } = event;
            tracing :: event ! (target : "datagram_sent" , parent : id , tracing :: Level :: DEBUG , { len = tracing :: field :: debug (len) , gso_offset = tracing :: field :: debug (gso_offset) });
        }
        #[inline]
        fn on_datagram_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::DatagramReceived,
        ) {
            let id = context.id();
            let api::DatagramReceived { len } = event;
            tracing :: event ! (target : "datagram_received" , parent : id , tracing :: Level :: DEBUG , { len = tracing :: field :: debug (len) });
        }
        #[inline]
        fn on_datagram_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::DatagramDropped,
        ) {
            let id = context.id();
            let api::DatagramDropped {
                local_addr,
                remote_addr,
                destination_cid,
                source_cid,
                len,
                reason,
            } = event;
            tracing :: event ! (target : "datagram_dropped" , parent : id , tracing :: Level :: DEBUG , { local_addr = tracing :: field :: debug (local_addr) , remote_addr = tracing :: field :: debug (remote_addr) , destination_cid = tracing :: field :: debug (destination_cid) , source_cid = tracing :: field :: debug (source_cid) , len = tracing :: field :: debug (len) , reason = tracing :: field :: debug (reason) });
        }
        #[inline]
        fn on_handshake_remote_address_change_observed(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::HandshakeRemoteAddressChangeObserved,
        ) {
            let id = context.id();
            let api::HandshakeRemoteAddressChangeObserved {
                local_addr,
                remote_addr,
                initial_remote_addr,
            } = event;
            tracing :: event ! (target : "handshake_remote_address_change_observed" , parent : id , tracing :: Level :: DEBUG , { local_addr = tracing :: field :: debug (local_addr) , remote_addr = tracing :: field :: debug (remote_addr) , initial_remote_addr = tracing :: field :: debug (initial_remote_addr) });
        }
        #[inline]
        fn on_connection_id_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ConnectionIdUpdated,
        ) {
            let id = context.id();
            let api::ConnectionIdUpdated {
                path_id,
                cid_consumer,
                previous,
                current,
            } = event;
            tracing :: event ! (target : "connection_id_updated" , parent : id , tracing :: Level :: DEBUG , { path_id = tracing :: field :: debug (path_id) , cid_consumer = tracing :: field :: debug (cid_consumer) , previous = tracing :: field :: debug (previous) , current = tracing :: field :: debug (current) });
        }
        #[inline]
        fn on_ecn_state_changed(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::EcnStateChanged,
        ) {
            let id = context.id();
            let api::EcnStateChanged { path, state } = event;
            tracing :: event ! (target : "ecn_state_changed" , parent : id , tracing :: Level :: DEBUG , { path = tracing :: field :: debug (path) , state = tracing :: field :: debug (state) });
        }
        #[inline]
        fn on_connection_migration_denied(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ConnectionMigrationDenied,
        ) {
            let id = context.id();
            let api::ConnectionMigrationDenied { reason } = event;
            tracing :: event ! (target : "connection_migration_denied" , parent : id , tracing :: Level :: DEBUG , { reason = tracing :: field :: debug (reason) });
        }
        #[inline]
        fn on_handshake_status_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::HandshakeStatusUpdated,
        ) {
            let id = context.id();
            let api::HandshakeStatusUpdated { status } = event;
            tracing :: event ! (target : "handshake_status_updated" , parent : id , tracing :: Level :: DEBUG , { status = tracing :: field :: debug (status) });
        }
        #[inline]
        fn on_tls_exporter_ready(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::TlsExporterReady,
        ) {
            let id = context.id();
            let api::TlsExporterReady { session } = event;
            tracing :: event ! (target : "tls_exporter_ready" , parent : id , tracing :: Level :: DEBUG , { session = tracing :: field :: debug (session) });
        }
        #[inline]
        fn on_tls_handshake_failed(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::TlsHandshakeFailed,
        ) {
            let id = context.id();
            let api::TlsHandshakeFailed { session } = event;
            tracing :: event ! (target : "tls_handshake_failed" , parent : id , tracing :: Level :: DEBUG , { session = tracing :: field :: debug (session) });
        }
        #[inline]
        fn on_path_challenge_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::PathChallengeUpdated,
        ) {
            let id = context.id();
            let api::PathChallengeUpdated {
                path_challenge_status,
                path,
                challenge_data,
            } = event;
            tracing :: event ! (target : "path_challenge_updated" , parent : id , tracing :: Level :: DEBUG , { path_challenge_status = tracing :: field :: debug (path_challenge_status) , path = tracing :: field :: debug (path) , challenge_data = tracing :: field :: debug (challenge_data) });
        }
        #[inline]
        fn on_tls_client_hello(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::TlsClientHello,
        ) {
            let id = context.id();
            let api::TlsClientHello { payload } = event;
            tracing :: event ! (target : "tls_client_hello" , parent : id , tracing :: Level :: DEBUG , { payload = tracing :: field :: debug (payload) });
        }
        #[inline]
        fn on_tls_server_hello(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::TlsServerHello,
        ) {
            let id = context.id();
            let api::TlsServerHello { payload } = event;
            tracing :: event ! (target : "tls_server_hello" , parent : id , tracing :: Level :: DEBUG , { payload = tracing :: field :: debug (payload) });
        }
        #[inline]
        fn on_rx_stream_progress(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::RxStreamProgress,
        ) {
            let id = context.id();
            let api::RxStreamProgress { bytes } = event;
            tracing :: event ! (target : "rx_stream_progress" , parent : id , tracing :: Level :: DEBUG , { bytes = tracing :: field :: debug (bytes) });
        }
        #[inline]
        fn on_tx_stream_progress(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::TxStreamProgress,
        ) {
            let id = context.id();
            let api::TxStreamProgress { bytes } = event;
            tracing :: event ! (target : "tx_stream_progress" , parent : id , tracing :: Level :: DEBUG , { bytes = tracing :: field :: debug (bytes) });
        }
        #[inline]
        fn on_keep_alive_timer_expired(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::KeepAliveTimerExpired,
        ) {
            let id = context.id();
            let api::KeepAliveTimerExpired { timeout } = event;
            tracing :: event ! (target : "keep_alive_timer_expired" , parent : id , tracing :: Level :: DEBUG , { timeout = tracing :: field :: debug (timeout) });
        }
        #[inline]
        fn on_mtu_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::MtuUpdated,
        ) {
            let id = context.id();
            let api::MtuUpdated {
                path_id,
                mtu,
                cause,
                search_complete,
            } = event;
            tracing :: event ! (target : "mtu_updated" , parent : id , tracing :: Level :: DEBUG , { path_id = tracing :: field :: debug (path_id) , mtu = tracing :: field :: debug (mtu) , cause = tracing :: field :: debug (cause) , search_complete = tracing :: field :: debug (search_complete) });
        }
        #[inline]
        fn on_slow_start_exited(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::SlowStartExited,
        ) {
            let id = context.id();
            let api::SlowStartExited {
                path_id,
                cause,
                congestion_window,
            } = event;
            tracing :: event ! (target : "slow_start_exited" , parent : id , tracing :: Level :: DEBUG , { path_id = tracing :: field :: debug (path_id) , cause = tracing :: field :: debug (cause) , congestion_window = tracing :: field :: debug (congestion_window) });
        }
        #[inline]
        fn on_delivery_rate_sampled(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::DeliveryRateSampled,
        ) {
            let id = context.id();
            let api::DeliveryRateSampled {
                path_id,
                rate_sample,
            } = event;
            tracing :: event ! (target : "delivery_rate_sampled" , parent : id , tracing :: Level :: DEBUG , { path_id = tracing :: field :: debug (path_id) , rate_sample = tracing :: field :: debug (rate_sample) });
        }
        #[inline]
        fn on_pacing_rate_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::PacingRateUpdated,
        ) {
            let id = context.id();
            let api::PacingRateUpdated {
                path_id,
                bytes_per_second,
                burst_size,
                pacing_gain,
            } = event;
            tracing :: event ! (target : "pacing_rate_updated" , parent : id , tracing :: Level :: DEBUG , { path_id = tracing :: field :: debug (path_id) , bytes_per_second = tracing :: field :: debug (bytes_per_second) , burst_size = tracing :: field :: debug (burst_size) , pacing_gain = tracing :: field :: debug (pacing_gain) });
        }
        #[inline]
        fn on_bbr_state_changed(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::BbrStateChanged,
        ) {
            let id = context.id();
            let api::BbrStateChanged { path_id, state } = event;
            tracing :: event ! (target : "bbr_state_changed" , parent : id , tracing :: Level :: DEBUG , { path_id = tracing :: field :: debug (path_id) , state = tracing :: field :: debug (state) });
        }
        #[inline]
        fn on_dc_state_changed(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::DcStateChanged,
        ) {
            let id = context.id();
            let api::DcStateChanged { state } = event;
            tracing :: event ! (target : "dc_state_changed" , parent : id , tracing :: Level :: DEBUG , { state = tracing :: field :: debug (state) });
        }
        #[inline]
        fn on_dc_path_created(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::DcPathCreated,
        ) {
            let id = context.id();
            let api::DcPathCreated { path } = event;
            tracing :: event ! (target : "dc_path_created" , parent : id , tracing :: Level :: DEBUG , { path = tracing :: field :: debug (path) });
        }
        #[inline]
        fn on_connection_closed(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ConnectionClosed,
        ) {
            let id = context.id();
            let api::ConnectionClosed { error } = event;
            tracing :: event ! (target : "connection_closed" , parent : id , tracing :: Level :: DEBUG , { error = tracing :: field :: debug (error) });
        }
        #[inline]
        fn on_version_information(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::VersionInformation,
        ) {
            let parent = self.parent(meta);
            let api::VersionInformation {
                server_versions,
                client_versions,
                chosen_version,
            } = event;
            tracing :: event ! (target : "version_information" , parent : parent , tracing :: Level :: DEBUG , { server_versions = tracing :: field :: debug (server_versions) , client_versions = tracing :: field :: debug (client_versions) , chosen_version = tracing :: field :: debug (chosen_version) });
        }
        #[inline]
        fn on_endpoint_packet_sent(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointPacketSent,
        ) {
            let parent = self.parent(meta);
            let api::EndpointPacketSent { packet_header } = event;
            tracing :: event ! (target : "endpoint_packet_sent" , parent : parent , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) });
        }
        #[inline]
        fn on_endpoint_packet_received(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointPacketReceived,
        ) {
            let parent = self.parent(meta);
            let api::EndpointPacketReceived { packet_header } = event;
            tracing :: event ! (target : "endpoint_packet_received" , parent : parent , tracing :: Level :: DEBUG , { packet_header = tracing :: field :: debug (packet_header) });
        }
        #[inline]
        fn on_endpoint_datagram_sent(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramSent,
        ) {
            let parent = self.parent(meta);
            let api::EndpointDatagramSent { len, gso_offset } = event;
            tracing :: event ! (target : "endpoint_datagram_sent" , parent : parent , tracing :: Level :: DEBUG , { len = tracing :: field :: debug (len) , gso_offset = tracing :: field :: debug (gso_offset) });
        }
        #[inline]
        fn on_endpoint_datagram_received(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramReceived,
        ) {
            let parent = self.parent(meta);
            let api::EndpointDatagramReceived { len } = event;
            tracing :: event ! (target : "endpoint_datagram_received" , parent : parent , tracing :: Level :: DEBUG , { len = tracing :: field :: debug (len) });
        }
        #[inline]
        fn on_endpoint_datagram_dropped(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramDropped,
        ) {
            let parent = self.parent(meta);
            let api::EndpointDatagramDropped { len, reason } = event;
            tracing :: event ! (target : "endpoint_datagram_dropped" , parent : parent , tracing :: Level :: DEBUG , { len = tracing :: field :: debug (len) , reason = tracing :: field :: debug (reason) });
        }
        #[inline]
        fn on_endpoint_connection_attempt_failed(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointConnectionAttemptFailed,
        ) {
            let parent = self.parent(meta);
            let api::EndpointConnectionAttemptFailed { error } = event;
            tracing :: event ! (target : "endpoint_connection_attempt_failed" , parent : parent , tracing :: Level :: DEBUG , { error = tracing :: field :: debug (error) });
        }
        #[inline]
        fn on_endpoint_connection_attempt_deduplicated(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointConnectionAttemptDeduplicated,
        ) {
            let parent = self.parent(meta);
            let api::EndpointConnectionAttemptDeduplicated {
                connection_id,
                already_open,
            } = event;
            tracing :: event ! (target : "endpoint_connection_attempt_deduplicated" , parent : parent , tracing :: Level :: DEBUG , { connection_id = tracing :: field :: debug (connection_id) , already_open = tracing :: field :: debug (already_open) });
        }
        #[inline]
        fn on_platform_tx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTx) {
            let parent = self.parent(meta);
            let api::PlatformTx {
                count,
                syscalls,
                blocked_syscalls,
                total_errors,
                dropped_errors,
            } = event;
            tracing :: event ! (target : "platform_tx" , parent : parent , tracing :: Level :: DEBUG , { count = tracing :: field :: debug (count) , syscalls = tracing :: field :: debug (syscalls) , blocked_syscalls = tracing :: field :: debug (blocked_syscalls) , total_errors = tracing :: field :: debug (total_errors) , dropped_errors = tracing :: field :: debug (dropped_errors) });
        }
        #[inline]
        fn on_platform_tx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTxError) {
            let parent = self.parent(meta);
            let api::PlatformTxError { errno } = event;
            tracing :: event ! (target : "platform_tx_error" , parent : parent , tracing :: Level :: DEBUG , { errno = tracing :: field :: debug (errno) });
        }
        #[inline]
        fn on_platform_rx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRx) {
            let parent = self.parent(meta);
            let api::PlatformRx {
                count,
                syscalls,
                blocked_syscalls,
                total_errors,
                dropped_errors,
            } = event;
            tracing :: event ! (target : "platform_rx" , parent : parent , tracing :: Level :: DEBUG , { count = tracing :: field :: debug (count) , syscalls = tracing :: field :: debug (syscalls) , blocked_syscalls = tracing :: field :: debug (blocked_syscalls) , total_errors = tracing :: field :: debug (total_errors) , dropped_errors = tracing :: field :: debug (dropped_errors) });
        }
        #[inline]
        fn on_platform_rx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRxError) {
            let parent = self.parent(meta);
            let api::PlatformRxError { errno } = event;
            tracing :: event ! (target : "platform_rx_error" , parent : parent , tracing :: Level :: DEBUG , { errno = tracing :: field :: debug (errno) });
        }
        #[inline]
        fn on_platform_feature_configured(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformFeatureConfigured,
        ) {
            let parent = self.parent(meta);
            let api::PlatformFeatureConfigured { configuration } = event;
            tracing :: event ! (target : "platform_feature_configured" , parent : parent , tracing :: Level :: DEBUG , { configuration = tracing :: field :: debug (configuration) });
        }
        #[inline]
        fn on_platform_event_loop_wakeup(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopWakeup,
        ) {
            let parent = self.parent(meta);
            let api::PlatformEventLoopWakeup {
                timeout_expired,
                rx_ready,
                tx_ready,
                application_wakeup,
            } = event;
            tracing :: event ! (target : "platform_event_loop_wakeup" , parent : parent , tracing :: Level :: DEBUG , { timeout_expired = tracing :: field :: debug (timeout_expired) , rx_ready = tracing :: field :: debug (rx_ready) , tx_ready = tracing :: field :: debug (tx_ready) , application_wakeup = tracing :: field :: debug (application_wakeup) });
        }
        #[inline]
        fn on_platform_event_loop_sleep(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopSleep,
        ) {
            let parent = self.parent(meta);
            let api::PlatformEventLoopSleep {
                timeout,
                processing_duration,
            } = event;
            tracing :: event ! (target : "platform_event_loop_sleep" , parent : parent , tracing :: Level :: DEBUG , { timeout = tracing :: field :: debug (timeout) , processing_duration = tracing :: field :: debug (processing_duration) });
        }
        #[inline]
        fn on_platform_event_loop_started(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopStarted,
        ) {
            let parent = self.parent(meta);
            let api::PlatformEventLoopStarted { local_address } = event;
            tracing :: event ! (target : "platform_event_loop_started" , parent : parent , tracing :: Level :: DEBUG , { local_address = tracing :: field :: debug (local_address) });
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
    pub struct TransportParameters<'a> {
        pub original_destination_connection_id: Option<ConnectionId<'a>>,
        pub initial_source_connection_id: Option<ConnectionId<'a>>,
        pub retry_source_connection_id: Option<ConnectionId<'a>>,
        pub stateless_reset_token: Option<&'a [u8]>,
        pub preferred_address: Option<PreferredAddress<'a>>,
        pub migration_support: bool,
        pub max_idle_timeout: Duration,
        pub ack_delay_exponent: u8,
        pub max_ack_delay: Duration,
        pub max_udp_payload_size: u64,
        pub active_connection_id_limit: u64,
        pub initial_max_stream_data_bidi_local: u64,
        pub initial_max_stream_data_bidi_remote: u64,
        pub initial_max_stream_data_uni: u64,
        pub initial_max_streams_bidi: u64,
        pub initial_max_streams_uni: u64,
        pub max_datagram_frame_size: u64,
        pub dc_supported_versions: &'a [u32],
    }
    impl<'a> IntoEvent<api::TransportParameters<'a>> for TransportParameters<'a> {
        #[inline]
        fn into_event(self) -> api::TransportParameters<'a> {
            let TransportParameters {
                original_destination_connection_id,
                initial_source_connection_id,
                retry_source_connection_id,
                stateless_reset_token,
                preferred_address,
                migration_support,
                max_idle_timeout,
                ack_delay_exponent,
                max_ack_delay,
                max_udp_payload_size,
                active_connection_id_limit,
                initial_max_stream_data_bidi_local,
                initial_max_stream_data_bidi_remote,
                initial_max_stream_data_uni,
                initial_max_streams_bidi,
                initial_max_streams_uni,
                max_datagram_frame_size,
                dc_supported_versions,
            } = self;
            api::TransportParameters {
                original_destination_connection_id: original_destination_connection_id.into_event(),
                initial_source_connection_id: initial_source_connection_id.into_event(),
                retry_source_connection_id: retry_source_connection_id.into_event(),
                stateless_reset_token: stateless_reset_token.into_event(),
                preferred_address: preferred_address.into_event(),
                migration_support: migration_support.into_event(),
                max_idle_timeout: max_idle_timeout.into_event(),
                ack_delay_exponent: ack_delay_exponent.into_event(),
                max_ack_delay: max_ack_delay.into_event(),
                max_udp_payload_size: max_udp_payload_size.into_event(),
                active_connection_id_limit: active_connection_id_limit.into_event(),
                initial_max_stream_data_bidi_local: initial_max_stream_data_bidi_local.into_event(),
                initial_max_stream_data_bidi_remote: initial_max_stream_data_bidi_remote
                    .into_event(),
                initial_max_stream_data_uni: initial_max_stream_data_uni.into_event(),
                initial_max_streams_bidi: initial_max_streams_bidi.into_event(),
                initial_max_streams_uni: initial_max_streams_uni.into_event(),
                max_datagram_frame_size: max_datagram_frame_size.into_event(),
                dc_supported_versions: dc_supported_versions.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PreferredAddress<'a> {
        pub ipv4_address: Option<SocketAddress<'a>>,
        pub ipv6_address: Option<SocketAddress<'a>>,
        pub connection_id: ConnectionId<'a>,
        pub stateless_reset_token: &'a [u8],
    }
    impl<'a> IntoEvent<api::PreferredAddress<'a>> for PreferredAddress<'a> {
        #[inline]
        fn into_event(self) -> api::PreferredAddress<'a> {
            let PreferredAddress {
                ipv4_address,
                ipv6_address,
                connection_id,
                stateless_reset_token,
            } = self;
            api::PreferredAddress {
                ipv4_address: ipv4_address.into_event(),
                ipv6_address: ipv6_address.into_event(),
                connection_id: connection_id.into_event(),
                stateless_reset_token: stateless_reset_token.into_event(),
            }
        }
    }
    #[derive(Copy, Clone, Debug)]
    pub struct Path<'a> {
        pub local_addr: SocketAddress<'a>,
        pub local_cid: ConnectionId<'a>,
        pub remote_addr: SocketAddress<'a>,
        pub remote_cid: ConnectionId<'a>,
        pub id: u64,
        pub is_active: bool,
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
                is_active,
            } = self;
            api::Path {
                local_addr: local_addr.into_event(),
                local_cid: local_cid.into_event(),
                remote_addr: remote_addr.into_event(),
                remote_cid: remote_cid.into_event(),
                id: id.into_event(),
                is_active: is_active.into_event(),
            }
        }
    }
    #[derive(Copy, Clone, Debug)]
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
    pub struct EcnCounts {
        #[doc = " A variable-length integer representing the total number of packets"]
        #[doc = " received with the ECT(0) codepoint."]
        pub ect_0_count: u64,
        #[doc = " A variable-length integer representing the total number of packets"]
        #[doc = " received with the ECT(1) codepoint."]
        pub ect_1_count: u64,
        #[doc = " A variable-length integer representing the total number of packets"]
        #[doc = " received with the CE codepoint."]
        pub ce_count: u64,
    }
    impl IntoEvent<api::EcnCounts> for EcnCounts {
        #[inline]
        fn into_event(self) -> api::EcnCounts {
            let EcnCounts {
                ect_0_count,
                ect_1_count,
                ce_count,
            } = self;
            api::EcnCounts {
                ect_0_count: ect_0_count.into_event(),
                ect_1_count: ect_1_count.into_event(),
                ce_count: ce_count.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ConnectionCloseFrame<'a> {
        pub error_code: u64,
        pub frame_type: Option<u64>,
        pub reason: Option<&'a [u8]>,
    }
    impl<'a> IntoEvent<api::ConnectionCloseFrame<'a>> for ConnectionCloseFrame<'a> {
        #[inline]
        fn into_event(self) -> api::ConnectionCloseFrame<'a> {
            let ConnectionCloseFrame {
                error_code,
                frame_type,
                reason,
            } = self;
            api::ConnectionCloseFrame {
                error_code: error_code.into_event(),
                frame_type: frame_type.into_event(),
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct MtuConfig {
        pub initial_mtu: u16,
        pub base_mtu: u16,
        pub max_mtu: u16,
    }
    impl IntoEvent<api::MtuConfig> for MtuConfig {
        #[inline]
        fn into_event(self) -> api::MtuConfig {
            let MtuConfig {
                initial_mtu,
                base_mtu,
                max_mtu,
            } = self;
            api::MtuConfig {
                initial_mtu: initial_mtu.into_event(),
                base_mtu: base_mtu.into_event(),
                max_mtu: max_mtu.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " A bandwidth delivery rate estimate with associated metadata"]
    pub struct RateSample {
        #[doc = " The length of the sampling interval"]
        pub interval: Duration,
        #[doc = " The amount of data in bytes marked as delivered over the sampling interval"]
        pub delivered_bytes: u64,
        #[doc = " The amount of data in bytes marked as lost over the sampling interval"]
        pub lost_bytes: u64,
        #[doc = " The number of packets marked as explicit congestion experienced over the sampling interval"]
        pub ecn_ce_count: u64,
        #[doc = " PacketInfo::is_app_limited from the most recent acknowledged packet"]
        pub is_app_limited: bool,
        #[doc = " PacketInfo::delivered_bytes from the most recent acknowledged packet"]
        pub prior_delivered_bytes: u64,
        #[doc = " PacketInfo::bytes_in_flight from the most recent acknowledged packet"]
        pub bytes_in_flight: u32,
        #[doc = " PacketInfo::lost_bytes from the most recent acknowledged packet"]
        pub prior_lost_bytes: u64,
        #[doc = " PacketInfo::ecn_ce_count from the most recent acknowledged packet"]
        pub prior_ecn_ce_count: u64,
        #[doc = " The delivery rate for this rate sample"]
        pub delivery_rate_bytes_per_second: u64,
    }
    impl IntoEvent<api::RateSample> for RateSample {
        #[inline]
        fn into_event(self) -> api::RateSample {
            let RateSample {
                interval,
                delivered_bytes,
                lost_bytes,
                ecn_ce_count,
                is_app_limited,
                prior_delivered_bytes,
                bytes_in_flight,
                prior_lost_bytes,
                prior_ecn_ce_count,
                delivery_rate_bytes_per_second,
            } = self;
            api::RateSample {
                interval: interval.into_event(),
                delivered_bytes: delivered_bytes.into_event(),
                lost_bytes: lost_bytes.into_event(),
                ecn_ce_count: ecn_ce_count.into_event(),
                is_app_limited: is_app_limited.into_event(),
                prior_delivered_bytes: prior_delivered_bytes.into_event(),
                bytes_in_flight: bytes_in_flight.into_event(),
                prior_lost_bytes: prior_lost_bytes.into_event(),
                prior_ecn_ce_count: prior_ecn_ce_count.into_event(),
                delivery_rate_bytes_per_second: delivery_rate_bytes_per_second.into_event(),
            }
        }
    }
    #[derive(Copy, Clone, Debug)]
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
        #[doc = " The packet number was already received and is a duplicate."]
        Duplicate,
        #[doc = " The received packet number was outside the range of tracked packet numbers."]
        #[doc = ""]
        #[doc = " This can happen when packets are heavily delayed or reordered. Currently, the maximum"]
        #[doc = " amount of reordering is limited to 128 packets. For example, if packet number `142`"]
        #[doc = " is received, the allowed range would be limited to `14-142`. If an endpoint received"]
        #[doc = " packet `< 14`, it would trigger this event."]
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
        DcStatelessResetTokens,
    }
    impl IntoEvent<api::Frame> for Frame {
        #[inline]
        fn into_event(self) -> api::Frame {
            use api::Frame::*;
            match self {
                Self::Padding => Padding {},
                Self::Ping => Ping {},
                Self::Ack {
                    ecn_counts,
                    largest_acknowledged,
                    ack_range_count,
                } => Ack {
                    ecn_counts: ecn_counts.into_event(),
                    largest_acknowledged: largest_acknowledged.into_event(),
                    ack_range_count: ack_range_count.into_event(),
                },
                Self::ResetStream {
                    id,
                    error_code,
                    final_size,
                } => ResetStream {
                    id: id.into_event(),
                    error_code: error_code.into_event(),
                    final_size: final_size.into_event(),
                },
                Self::StopSending { id, error_code } => StopSending {
                    id: id.into_event(),
                    error_code: error_code.into_event(),
                },
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
                Self::MaxData { value } => MaxData {
                    value: value.into_event(),
                },
                Self::MaxStreamData {
                    stream_type,
                    id,
                    value,
                } => MaxStreamData {
                    stream_type: stream_type.into_event(),
                    id: id.into_event(),
                    value: value.into_event(),
                },
                Self::MaxStreams { stream_type, value } => MaxStreams {
                    stream_type: stream_type.into_event(),
                    value: value.into_event(),
                },
                Self::DataBlocked { data_limit } => DataBlocked {
                    data_limit: data_limit.into_event(),
                },
                Self::StreamDataBlocked {
                    stream_id,
                    stream_data_limit,
                } => StreamDataBlocked {
                    stream_id: stream_id.into_event(),
                    stream_data_limit: stream_data_limit.into_event(),
                },
                Self::StreamsBlocked {
                    stream_type,
                    stream_limit,
                } => StreamsBlocked {
                    stream_type: stream_type.into_event(),
                    stream_limit: stream_limit.into_event(),
                },
                Self::NewConnectionId {
                    sequence_number,
                    retire_prior_to,
                } => NewConnectionId {
                    sequence_number: sequence_number.into_event(),
                    retire_prior_to: retire_prior_to.into_event(),
                },
                Self::RetireConnectionId => RetireConnectionId {},
                Self::PathChallenge => PathChallenge {},
                Self::PathResponse => PathResponse {},
                Self::ConnectionClose => ConnectionClose {},
                Self::HandshakeDone => HandshakeDone {},
                Self::Datagram { len } => Datagram {
                    len: len.into_event(),
                },
                Self::DcStatelessResetTokens => DcStatelessResetTokens {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum StreamType {
        Bidirectional,
        Unidirectional,
    }
    impl IntoEvent<api::StreamType> for StreamType {
        #[inline]
        fn into_event(self) -> api::StreamType {
            use api::StreamType::*;
            match self {
                Self::Bidirectional => Bidirectional {},
                Self::Unidirectional => Unidirectional {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum PacketHeader {
        Initial { number: u64, version: u32 },
        Handshake { number: u64, version: u32 },
        ZeroRtt { number: u64, version: u32 },
        OneRtt { number: u64 },
        Retry { version: u32 },
        VersionNegotiation,
        StatelessReset,
    }
    impl IntoEvent<api::PacketHeader> for PacketHeader {
        #[inline]
        fn into_event(self) -> api::PacketHeader {
            use api::PacketHeader::*;
            match self {
                Self::Initial { number, version } => Initial {
                    number: number.into_event(),
                    version: version.into_event(),
                },
                Self::Handshake { number, version } => Handshake {
                    number: number.into_event(),
                    version: version.into_event(),
                },
                Self::ZeroRtt { number, version } => ZeroRtt {
                    number: number.into_event(),
                    version: version.into_event(),
                },
                Self::OneRtt { number } => OneRtt {
                    number: number.into_event(),
                },
                Self::Retry { version } => Retry {
                    version: version.into_event(),
                },
                Self::VersionNegotiation => VersionNegotiation {},
                Self::StatelessReset => StatelessReset {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum PacketType {
        Initial,
        Handshake,
        ZeroRtt,
        OneRtt,
        Retry,
        VersionNegotiation,
        StatelessReset,
    }
    impl IntoEvent<api::PacketType> for PacketType {
        #[inline]
        fn into_event(self) -> api::PacketType {
            use api::PacketType::*;
            match self {
                Self::Initial => Initial {},
                Self::Handshake => Handshake {},
                Self::ZeroRtt => ZeroRtt {},
                Self::OneRtt => OneRtt {},
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
        #[doc = " This maps to an internal connection id, which is a stable identifier across CID changes."]
        Connection {
            id: u64,
        },
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
    #[doc = " An endpoint may be either a Server or a Client"]
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
    pub enum DatagramDropReason {
        #[doc = " There was an error while attempting to decode the datagram."]
        DecodingFailed,
        #[doc = " There was an error while parsing the Retry token."]
        InvalidRetryToken,
        #[doc = " The peer specified an unsupported QUIC version."]
        UnsupportedVersion,
        #[doc = " The peer sent an invalid Destination Connection Id."]
        InvalidDestinationConnectionId,
        #[doc = " The peer sent an invalid Source Connection Id."]
        InvalidSourceConnectionId,
        #[doc = " Application provided invalid MTU configuration."]
        InvalidMtuConfiguration {
            #[doc = " MTU configuration for the endpoint"]
            endpoint_mtu_config: MtuConfig,
        },
        #[doc = " The Destination Connection Id is unknown and does not map to a Connection."]
        #[doc = ""]
        #[doc = " Connections are mapped to Destination Connections Ids (DCID) and packets"]
        #[doc = " in a Datagram are routed to a connection based on the DCID in the first"]
        #[doc = " packet. If a Connection is not found for the specified DCID then the"]
        #[doc = " datagram can not be processed and is dropped."]
        UnknownDestinationConnectionId,
        #[doc = " The connection attempt was rejected."]
        RejectedConnectionAttempt,
        #[doc = " A datagram was received from an unknown server address."]
        UnknownServerAddress,
        #[doc = " The peer initiated a connection migration before the handshake was confirmed."]
        #[doc = ""]
        #[doc = " Note: This drop reason is no longer emitted"]
        ConnectionMigrationDuringHandshake,
        #[doc = " The attempted connection migration was rejected."]
        RejectedConnectionMigration { reason: MigrationDenyReason },
        #[doc = " The maximum number of paths per connection was exceeded."]
        PathLimitExceeded,
        #[doc = " The peer initiated a connection migration without supplying enough connection IDs to use."]
        #[doc = ""]
        #[doc = " Note: This drop reason is no longer emitted"]
        InsufficientConnectionIds,
    }
    impl IntoEvent<api::DatagramDropReason> for DatagramDropReason {
        #[inline]
        fn into_event(self) -> api::DatagramDropReason {
            use api::DatagramDropReason::*;
            match self {
                Self::DecodingFailed => DecodingFailed {},
                Self::InvalidRetryToken => InvalidRetryToken {},
                Self::UnsupportedVersion => UnsupportedVersion {},
                Self::InvalidDestinationConnectionId => InvalidDestinationConnectionId {},
                Self::InvalidSourceConnectionId => InvalidSourceConnectionId {},
                Self::InvalidMtuConfiguration {
                    endpoint_mtu_config,
                } => InvalidMtuConfiguration {
                    endpoint_mtu_config: endpoint_mtu_config.into_event(),
                },
                Self::UnknownDestinationConnectionId => UnknownDestinationConnectionId {},
                Self::RejectedConnectionAttempt => RejectedConnectionAttempt {},
                Self::UnknownServerAddress => UnknownServerAddress {},
                Self::ConnectionMigrationDuringHandshake => ConnectionMigrationDuringHandshake {},
                Self::RejectedConnectionMigration { reason } => RejectedConnectionMigration {
                    reason: reason.into_event(),
                },
                Self::PathLimitExceeded => PathLimitExceeded {},
                Self::InsufficientConnectionIds => InsufficientConnectionIds {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum KeySpace {
        Initial,
        Handshake,
        ZeroRtt,
        OneRtt,
    }
    impl IntoEvent<api::KeySpace> for KeySpace {
        #[inline]
        fn into_event(self) -> api::KeySpace {
            use api::KeySpace::*;
            match self {
                Self::Initial => Initial {},
                Self::Handshake => Handshake {},
                Self::ZeroRtt => ZeroRtt {},
                Self::OneRtt => OneRtt {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum PacketSkipReason {
        #[doc = " Skipped a packet number to elicit a quicker PTO acknowledgment"]
        PtoProbe,
        #[doc = " Skipped a packet number to detect an Optimistic Ack attack"]
        OptimisticAckMitigation,
    }
    impl IntoEvent<api::PacketSkipReason> for PacketSkipReason {
        #[inline]
        fn into_event(self) -> api::PacketSkipReason {
            use api::PacketSkipReason::*;
            match self {
                Self::PtoProbe => PtoProbe {},
                Self::OptimisticAckMitigation => OptimisticAckMitigation {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum PacketDropReason<'a> {
        #[doc = " A connection error occurred and is no longer able to process packets."]
        ConnectionError { path: Path<'a> },
        #[doc = " The handshake needed to be complete before processing the packet."]
        #[doc = ""]
        #[doc = " To ensure the connection stays secure, short packets can only be processed"]
        #[doc = " once the handshake has completed."]
        HandshakeNotComplete { path: Path<'a> },
        #[doc = " The packet contained a version which did not match the version negotiated"]
        #[doc = " during the handshake."]
        VersionMismatch { version: u32, path: Path<'a> },
        #[doc = " A datagram contained more than one destination connection ID, which is"]
        #[doc = " not allowed."]
        ConnectionIdMismatch {
            packet_cid: &'a [u8],
            path: Path<'a>,
        },
        #[doc = " There was a failure when attempting to remove header protection."]
        UnprotectFailed { space: KeySpace, path: Path<'a> },
        #[doc = " There was a failure when attempting to decrypt the packet."]
        DecryptionFailed {
            path: Path<'a>,
            packet_header: PacketHeader,
        },
        #[doc = " Packet decoding failed."]
        #[doc = ""]
        #[doc = " The payload is decoded one packet at a time. If decoding fails"]
        #[doc = " then the remaining packets are also discarded."]
        DecodingFailed { path: Path<'a> },
        #[doc = " The client received a non-empty retry token."]
        NonEmptyRetryToken { path: Path<'a> },
        #[doc = " A Retry packet was discarded."]
        RetryDiscarded {
            reason: RetryDiscardReason<'a>,
            path: Path<'a>,
        },
        #[doc = " The received Initial packet was not transported in a datagram of at least 1200 bytes"]
        UndersizedInitialPacket { path: Path<'a> },
        #[doc = " The destination connection ID in the packet was the initial connection ID but was in"]
        #[doc = " a non-initial packet."]
        InitialConnectionIdInvalidSpace {
            path: Path<'a>,
            packet_type: PacketType,
        },
        #[doc = " The packet space for a received packet did not exist"]
        PacketSpaceDoesNotExist {
            path: Path<'a>,
            packet_type: PacketType,
        },
    }
    impl<'a> IntoEvent<api::PacketDropReason<'a>> for PacketDropReason<'a> {
        #[inline]
        fn into_event(self) -> api::PacketDropReason<'a> {
            use api::PacketDropReason::*;
            match self {
                Self::ConnectionError { path } => ConnectionError {
                    path: path.into_event(),
                },
                Self::HandshakeNotComplete { path } => HandshakeNotComplete {
                    path: path.into_event(),
                },
                Self::VersionMismatch { version, path } => VersionMismatch {
                    version: version.into_event(),
                    path: path.into_event(),
                },
                Self::ConnectionIdMismatch { packet_cid, path } => ConnectionIdMismatch {
                    packet_cid: packet_cid.into_event(),
                    path: path.into_event(),
                },
                Self::UnprotectFailed { space, path } => UnprotectFailed {
                    space: space.into_event(),
                    path: path.into_event(),
                },
                Self::DecryptionFailed {
                    path,
                    packet_header,
                } => DecryptionFailed {
                    path: path.into_event(),
                    packet_header: packet_header.into_event(),
                },
                Self::DecodingFailed { path } => DecodingFailed {
                    path: path.into_event(),
                },
                Self::NonEmptyRetryToken { path } => NonEmptyRetryToken {
                    path: path.into_event(),
                },
                Self::RetryDiscarded { reason, path } => RetryDiscarded {
                    reason: reason.into_event(),
                    path: path.into_event(),
                },
                Self::UndersizedInitialPacket { path } => UndersizedInitialPacket {
                    path: path.into_event(),
                },
                Self::InitialConnectionIdInvalidSpace { path, packet_type } => {
                    InitialConnectionIdInvalidSpace {
                        path: path.into_event(),
                        packet_type: packet_type.into_event(),
                    }
                }
                Self::PacketSpaceDoesNotExist { path, packet_type } => PacketSpaceDoesNotExist {
                    path: path.into_event(),
                    packet_type: packet_type.into_event(),
                },
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum AckAction {
        #[doc = " Ack range for received packets was dropped due to space constraints"]
        #[doc = ""]
        #[doc = " For the purpose of processing Acks, RX packet numbers are stored as"]
        #[doc = " packet_number ranges in an IntervalSet; only lower and upper bounds"]
        #[doc = " are stored instead of individual packet_numbers. Ranges are merged"]
        #[doc = " when possible so only disjointed ranges are stored."]
        #[doc = ""]
        #[doc = " When at `capacity`, the lowest packet_number range is dropped."]
        RxAckRangeDropped {
            #[doc = " The packet number range which was dropped"]
            packet_number_range: core::ops::RangeInclusive<u64>,
            #[doc = " The number of disjoint ranges the IntervalSet can store"]
            capacity: usize,
            #[doc = " The store packet_number range in the IntervalSet"]
            stored_range: core::ops::RangeInclusive<u64>,
        },
    }
    #[allow(deprecated)]
    impl IntoEvent<api::AckAction> for AckAction {
        #[inline]
        fn into_event(self) -> api::AckAction {
            use api::AckAction::*;
            match self {
                Self::RxAckRangeDropped {
                    packet_number_range,
                    capacity,
                    stored_range,
                } => RxAckRangeDropped {
                    packet_number_range: packet_number_range.into_event(),
                    capacity: capacity.into_event(),
                    stored_range: stored_range.into_event(),
                },
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum RetryDiscardReason<'a> {
        #[doc = " Received a Retry packet with SCID field equal to DCID field."]
        ScidEqualsDcid { cid: &'a [u8] },
        #[doc = " A client only processes at most one Retry packet."]
        RetryAlreadyProcessed,
        #[doc = " The client discards Retry packets if a valid Initial packet"]
        #[doc = " has been received and processed."]
        InitialAlreadyProcessed,
        #[doc = " The Retry packet received contained an invalid retry integrity tag"]
        InvalidIntegrityTag,
    }
    impl<'a> IntoEvent<api::RetryDiscardReason<'a>> for RetryDiscardReason<'a> {
        #[inline]
        fn into_event(self) -> api::RetryDiscardReason<'a> {
            use api::RetryDiscardReason::*;
            match self {
                Self::ScidEqualsDcid { cid } => ScidEqualsDcid {
                    cid: cid.into_event(),
                },
                Self::RetryAlreadyProcessed => RetryAlreadyProcessed {},
                Self::InitialAlreadyProcessed => InitialAlreadyProcessed {},
                Self::InvalidIntegrityTag => InvalidIntegrityTag {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum MigrationDenyReason {
        BlockedPort,
        PortScopeChanged,
        IpScopeChange,
        ConnectionMigrationDisabled,
    }
    impl IntoEvent<api::MigrationDenyReason> for MigrationDenyReason {
        #[inline]
        fn into_event(self) -> api::MigrationDenyReason {
            use api::MigrationDenyReason::*;
            match self {
                Self::BlockedPort => BlockedPort {},
                Self::PortScopeChanged => PortScopeChanged {},
                Self::IpScopeChange => IpScopeChange {},
                Self::ConnectionMigrationDisabled => ConnectionMigrationDisabled {},
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The current state of the ECN controller for the path"]
    pub enum EcnState {
        #[doc = " ECN capability is being actively tested"]
        Testing,
        #[doc = " ECN capability has been tested, but not validated yet"]
        Unknown,
        #[doc = " ECN capability testing has failed validation"]
        Failed,
        #[doc = " ECN capability has been confirmed"]
        Capable,
    }
    impl IntoEvent<api::EcnState> for EcnState {
        #[inline]
        fn into_event(self) -> api::EcnState {
            use api::EcnState::*;
            match self {
                Self::Testing => Testing {},
                Self::Unknown => Unknown {},
                Self::Failed => Failed {},
                Self::Capable => Capable {},
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Events tracking the progress of handshake status"]
    pub enum HandshakeStatus {
        #[doc = " The handshake has completed."]
        Complete,
        #[doc = " The handshake has been confirmed."]
        Confirmed,
        #[doc = " A HANDSHAKE_DONE frame was delivered or received."]
        #[doc = ""]
        #[doc = " A Client endpoint receives a HANDSHAKE_DONE frame and"]
        #[doc = " only a Server is allowed to send the HANDSHAKE_DONE"]
        #[doc = " frame."]
        HandshakeDoneAcked,
        #[doc = " A HANDSHAKE_DONE frame was declared lost."]
        #[doc = ""]
        #[doc = " The Server is responsible for re-transmitting the"]
        #[doc = " HANDSHAKE_DONE frame until it is acked by the peer."]
        HandshakeDoneLost,
    }
    impl IntoEvent<api::HandshakeStatus> for HandshakeStatus {
        #[inline]
        fn into_event(self) -> api::HandshakeStatus {
            use api::HandshakeStatus::*;
            match self {
                Self::Complete => Complete {},
                Self::Confirmed => Confirmed {},
                Self::HandshakeDoneAcked => HandshakeDoneAcked {},
                Self::HandshakeDoneLost => HandshakeDoneLost {},
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The source that caused a congestion event"]
    pub enum CongestionSource {
        #[doc = " Explicit Congestion Notification"]
        Ecn,
        #[doc = " One or more packets were detected lost"]
        PacketLoss,
    }
    impl IntoEvent<api::CongestionSource> for CongestionSource {
        #[inline]
        fn into_event(self) -> api::CongestionSource {
            use api::CongestionSource::*;
            match self {
                Self::Ecn => Ecn {},
                Self::PacketLoss => PacketLoss {},
            }
        }
    }
    #[derive(Clone, Debug)]
    #[allow(non_camel_case_types)]
    pub enum CipherSuite {
        TLS_AES_128_GCM_SHA256,
        TLS_AES_256_GCM_SHA384,
        TLS_CHACHA20_POLY1305_SHA256,
        Unknown,
    }
    impl IntoEvent<api::CipherSuite> for CipherSuite {
        #[inline]
        fn into_event(self) -> api::CipherSuite {
            use api::CipherSuite::*;
            match self {
                Self::TLS_AES_128_GCM_SHA256 => TLS_AES_128_GCM_SHA256 {},
                Self::TLS_AES_256_GCM_SHA384 => TLS_AES_256_GCM_SHA384 {},
                Self::TLS_CHACHA20_POLY1305_SHA256 => TLS_CHACHA20_POLY1305_SHA256 {},
                Self::Unknown => Unknown {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum PathChallengeStatus {
        Validated,
        Abandoned,
    }
    impl IntoEvent<api::PathChallengeStatus> for PathChallengeStatus {
        #[inline]
        fn into_event(self) -> api::PathChallengeStatus {
            use api::PathChallengeStatus::*;
            match self {
                Self::Validated => Validated {},
                Self::Abandoned => Abandoned {},
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The reason the slow start congestion controller state has been exited"]
    pub enum SlowStartExitCause {
        #[doc = " A packet was determined lost"]
        PacketLoss,
        #[doc = " An Explicit Congestion Notification: Congestion Experienced marking was received"]
        Ecn,
        #[doc = " The round trip time estimate was updated"]
        Rtt,
        #[doc = " Slow Start exited due to a reason other than those above"]
        #[doc = ""]
        #[doc = " With the Cubic congestion controller, this reason is used after the initial exiting of"]
        #[doc = " Slow Start, when the previously determined Slow Start threshold is exceed by the"]
        #[doc = " congestion window."]
        Other,
    }
    impl IntoEvent<api::SlowStartExitCause> for SlowStartExitCause {
        #[inline]
        fn into_event(self) -> api::SlowStartExitCause {
            use api::SlowStartExitCause::*;
            match self {
                Self::PacketLoss => PacketLoss {},
                Self::Ecn => Ecn {},
                Self::Rtt => Rtt {},
                Self::Other => Other {},
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The reason the MTU was updated"]
    pub enum MtuUpdatedCause {
        #[doc = " The MTU was initialized with the default value"]
        NewPath,
        #[doc = " An MTU probe was acknowledged by the peer"]
        ProbeAcknowledged,
        #[doc = " A blackhole was detected"]
        Blackhole,
        #[doc = " An early packet using the configured InitialMtu was lost"]
        InitialMtuPacketLost,
        #[doc = " An early packet using the configured InitialMtu was acknowledged by the peer"]
        InitialMtuPacketAcknowledged,
        #[doc = " MTU probes larger than the current MTU were not acknowledged"]
        LargerProbesLost,
    }
    impl IntoEvent<api::MtuUpdatedCause> for MtuUpdatedCause {
        #[inline]
        fn into_event(self) -> api::MtuUpdatedCause {
            use api::MtuUpdatedCause::*;
            match self {
                Self::NewPath => NewPath {},
                Self::ProbeAcknowledged => ProbeAcknowledged {},
                Self::Blackhole => Blackhole {},
                Self::InitialMtuPacketLost => InitialMtuPacketLost {},
                Self::InitialMtuPacketAcknowledged => InitialMtuPacketAcknowledged {},
                Self::LargerProbesLost => LargerProbesLost {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum BbrState {
        Startup,
        Drain,
        ProbeBwDown,
        ProbeBwCruise,
        ProbeBwRefill,
        ProbeBwUp,
        ProbeRtt,
    }
    impl IntoEvent<api::BbrState> for BbrState {
        #[inline]
        fn into_event(self) -> api::BbrState {
            use api::BbrState::*;
            match self {
                Self::Startup => Startup {},
                Self::Drain => Drain {},
                Self::ProbeBwDown => ProbeBwDown {},
                Self::ProbeBwCruise => ProbeBwCruise {},
                Self::ProbeBwRefill => ProbeBwRefill {},
                Self::ProbeBwUp => ProbeBwUp {},
                Self::ProbeRtt => ProbeRtt {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum DcState {
        VersionNegotiated { version: u32 },
        NoVersionNegotiated,
        PathSecretsReady,
        Complete,
    }
    impl IntoEvent<api::DcState> for DcState {
        #[inline]
        fn into_event(self) -> api::DcState {
            use api::DcState::*;
            match self {
                Self::VersionNegotiated { version } => VersionNegotiated {
                    version: version.into_event(),
                },
                Self::NoVersionNegotiated => NoVersionNegotiated {},
                Self::PathSecretsReady => PathSecretsReady {},
                Self::Complete => Complete {},
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Application level protocol"]
    pub struct ApplicationProtocolInformation<'a> {
        pub chosen_application_protocol: &'a [u8],
    }
    impl<'a> IntoEvent<api::ApplicationProtocolInformation<'a>> for ApplicationProtocolInformation<'a> {
        #[inline]
        fn into_event(self) -> api::ApplicationProtocolInformation<'a> {
            let ApplicationProtocolInformation {
                chosen_application_protocol,
            } = self;
            api::ApplicationProtocolInformation {
                chosen_application_protocol: chosen_application_protocol.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Server Name was negotiated for the connection"]
    pub struct ServerNameInformation<'a> {
        pub chosen_server_name: &'a str,
    }
    impl<'a> IntoEvent<api::ServerNameInformation<'a>> for ServerNameInformation<'a> {
        #[inline]
        fn into_event(self) -> api::ServerNameInformation<'a> {
            let ServerNameInformation { chosen_server_name } = self;
            api::ServerNameInformation {
                chosen_server_name: chosen_server_name.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Key Exchange Group was negotiated for the connection"]
    #[doc = ""]
    #[doc = " `contains_kem` is `true` if the `chosen_group_name`"]
    #[doc = " contains a key encapsulation mechanism"]
    pub struct KeyExchangeGroup<'a> {
        pub chosen_group_name: &'a str,
        pub contains_kem: bool,
    }
    impl<'a> IntoEvent<api::KeyExchangeGroup<'a>> for KeyExchangeGroup<'a> {
        #[inline]
        fn into_event(self) -> api::KeyExchangeGroup<'a> {
            let KeyExchangeGroup {
                chosen_group_name,
                contains_kem,
            } = self;
            api::KeyExchangeGroup {
                chosen_group_name: chosen_group_name.into_event(),
                contains_kem: contains_kem.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Packet was skipped with a given reason"]
    pub struct PacketSkipped {
        pub number: u64,
        pub space: KeySpace,
        pub reason: PacketSkipReason,
    }
    impl IntoEvent<api::PacketSkipped> for PacketSkipped {
        #[inline]
        fn into_event(self) -> api::PacketSkipped {
            let PacketSkipped {
                number,
                space,
                reason,
            } = self;
            api::PacketSkipped {
                number: number.into_event(),
                space: space.into_event(),
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Packet was sent by a connection"]
    pub struct PacketSent {
        pub packet_header: PacketHeader,
        pub packet_len: usize,
    }
    impl IntoEvent<api::PacketSent> for PacketSent {
        #[inline]
        fn into_event(self) -> api::PacketSent {
            let PacketSent {
                packet_header,
                packet_len,
            } = self;
            api::PacketSent {
                packet_header: packet_header.into_event(),
                packet_len: packet_len.into_event(),
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
    pub struct FrameReceived<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub frame: Frame,
    }
    impl<'a> IntoEvent<api::FrameReceived<'a>> for FrameReceived<'a> {
        #[inline]
        fn into_event(self) -> api::FrameReceived<'a> {
            let FrameReceived {
                packet_header,
                path,
                frame,
            } = self;
            api::FrameReceived {
                packet_header: packet_header.into_event(),
                path: path.into_event(),
                frame: frame.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " A `CONNECTION_CLOSE` frame was received"]
    #[doc = ""]
    #[doc = " This event includes additional details from the frame, particularly the"]
    #[doc = " reason (if provided) the peer closed the connection"]
    pub struct ConnectionCloseFrameReceived<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub frame: ConnectionCloseFrame<'a>,
    }
    impl<'a> IntoEvent<api::ConnectionCloseFrameReceived<'a>> for ConnectionCloseFrameReceived<'a> {
        #[inline]
        fn into_event(self) -> api::ConnectionCloseFrameReceived<'a> {
            let ConnectionCloseFrameReceived {
                packet_header,
                path,
                frame,
            } = self;
            api::ConnectionCloseFrameReceived {
                packet_header: packet_header.into_event(),
                path: path.into_event(),
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
    pub struct RecoveryMetrics<'a> {
        pub path: Path<'a>,
        pub min_rtt: Duration,
        pub smoothed_rtt: Duration,
        pub latest_rtt: Duration,
        pub rtt_variance: Duration,
        pub max_ack_delay: Duration,
        pub pto_count: u32,
        pub congestion_window: u32,
        pub bytes_in_flight: u32,
        pub congestion_limited: bool,
    }
    impl<'a> IntoEvent<api::RecoveryMetrics<'a>> for RecoveryMetrics<'a> {
        #[inline]
        fn into_event(self) -> api::RecoveryMetrics<'a> {
            let RecoveryMetrics {
                path,
                min_rtt,
                smoothed_rtt,
                latest_rtt,
                rtt_variance,
                max_ack_delay,
                pto_count,
                congestion_window,
                bytes_in_flight,
                congestion_limited,
            } = self;
            api::RecoveryMetrics {
                path: path.into_event(),
                min_rtt: min_rtt.into_event(),
                smoothed_rtt: smoothed_rtt.into_event(),
                latest_rtt: latest_rtt.into_event(),
                rtt_variance: rtt_variance.into_event(),
                max_ack_delay: max_ack_delay.into_event(),
                pto_count: pto_count.into_event(),
                congestion_window: congestion_window.into_event(),
                bytes_in_flight: bytes_in_flight.into_event(),
                congestion_limited: congestion_limited.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Congestion (ECN or packet loss) has occurred"]
    pub struct Congestion<'a> {
        pub path: Path<'a>,
        pub source: CongestionSource,
    }
    impl<'a> IntoEvent<api::Congestion<'a>> for Congestion<'a> {
        #[inline]
        fn into_event(self) -> api::Congestion<'a> {
            let Congestion { path, source } = self;
            api::Congestion {
                path: path.into_event(),
                source: source.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Events related to ACK processing"]
    pub struct AckProcessed<'a> {
        pub action: AckAction,
        pub path: Path<'a>,
    }
    #[allow(deprecated)]
    impl<'a> IntoEvent<api::AckProcessed<'a>> for AckProcessed<'a> {
        #[inline]
        fn into_event(self) -> api::AckProcessed<'a> {
            let AckProcessed { action, path } = self;
            api::AckProcessed {
                action: action.into_event(),
                path: path.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Ack range for received packets was dropped due to space constraints"]
    #[doc = ""]
    #[doc = " For the purpose of processing Acks, RX packet numbers are stored as"]
    #[doc = " packet_number ranges in an IntervalSet; only lower and upper bounds"]
    #[doc = " are stored instead of individual packet_numbers. Ranges are merged"]
    #[doc = " when possible so only disjointed ranges are stored."]
    #[doc = ""]
    #[doc = " When at `capacity`, the lowest packet_number range is dropped."]
    pub struct RxAckRangeDropped<'a> {
        pub path: Path<'a>,
        #[doc = " The packet number range which was dropped"]
        pub packet_number_range: core::ops::RangeInclusive<u64>,
        #[doc = " The number of disjoint ranges the IntervalSet can store"]
        pub capacity: usize,
        #[doc = " The store packet_number range in the IntervalSet"]
        pub stored_range: core::ops::RangeInclusive<u64>,
    }
    impl<'a> IntoEvent<api::RxAckRangeDropped<'a>> for RxAckRangeDropped<'a> {
        #[inline]
        fn into_event(self) -> api::RxAckRangeDropped<'a> {
            let RxAckRangeDropped {
                path,
                packet_number_range,
                capacity,
                stored_range,
            } = self;
            api::RxAckRangeDropped {
                path: path.into_event(),
                packet_number_range: packet_number_range.into_event(),
                capacity: capacity.into_event(),
                stored_range: stored_range.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " ACK range was received"]
    pub struct AckRangeReceived<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub ack_range: RangeInclusive<u64>,
    }
    impl<'a> IntoEvent<api::AckRangeReceived<'a>> for AckRangeReceived<'a> {
        #[inline]
        fn into_event(self) -> api::AckRangeReceived<'a> {
            let AckRangeReceived {
                packet_header,
                path,
                ack_range,
            } = self;
            api::AckRangeReceived {
                packet_header: packet_header.into_event(),
                path: path.into_event(),
                ack_range: ack_range.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " ACK range was sent"]
    pub struct AckRangeSent {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub ack_range: RangeInclusive<u64>,
    }
    impl IntoEvent<api::AckRangeSent> for AckRangeSent {
        #[inline]
        fn into_event(self) -> api::AckRangeSent {
            let AckRangeSent {
                packet_header,
                path_id,
                ack_range,
            } = self;
            api::AckRangeSent {
                packet_header: packet_header.into_event(),
                path_id: path_id.into_event(),
                ack_range: ack_range.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Packet was dropped with the given reason"]
    pub struct PacketDropped<'a> {
        pub reason: PacketDropReason<'a>,
    }
    impl<'a> IntoEvent<api::PacketDropped<'a>> for PacketDropped<'a> {
        #[inline]
        fn into_event(self) -> api::PacketDropped<'a> {
            let PacketDropped { reason } = self;
            api::PacketDropped {
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Crypto key updated"]
    pub struct KeyUpdate {
        pub key_type: KeyType,
        pub cipher_suite: CipherSuite,
    }
    impl IntoEvent<api::KeyUpdate> for KeyUpdate {
        #[inline]
        fn into_event(self) -> api::KeyUpdate {
            let KeyUpdate {
                key_type,
                cipher_suite,
            } = self;
            api::KeyUpdate {
                key_type: key_type.into_event(),
                cipher_suite: cipher_suite.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct KeySpaceDiscarded {
        pub space: KeySpace,
    }
    impl IntoEvent<api::KeySpaceDiscarded> for KeySpaceDiscarded {
        #[inline]
        fn into_event(self) -> api::KeySpaceDiscarded {
            let KeySpaceDiscarded { space } = self;
            api::KeySpaceDiscarded {
                space: space.into_event(),
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
    #[doc = " Duplicate packet received"]
    pub struct DuplicatePacket<'a> {
        pub packet_header: PacketHeader,
        pub path: Path<'a>,
        pub error: DuplicatePacketError,
    }
    impl<'a> IntoEvent<api::DuplicatePacket<'a>> for DuplicatePacket<'a> {
        #[inline]
        fn into_event(self) -> api::DuplicatePacket<'a> {
            let DuplicatePacket {
                packet_header,
                path,
                error,
            } = self;
            api::DuplicatePacket {
                packet_header: packet_header.into_event(),
                path: path.into_event(),
                error: error.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Transport parameters received by connection"]
    pub struct TransportParametersReceived<'a> {
        pub transport_parameters: TransportParameters<'a>,
    }
    impl<'a> IntoEvent<api::TransportParametersReceived<'a>> for TransportParametersReceived<'a> {
        #[inline]
        fn into_event(self) -> api::TransportParametersReceived<'a> {
            let TransportParametersReceived {
                transport_parameters,
            } = self;
            api::TransportParametersReceived {
                transport_parameters: transport_parameters.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Datagram sent by a connection"]
    pub struct DatagramSent {
        pub len: u16,
        #[doc = " The GSO offset at which this datagram was written"]
        #[doc = ""]
        #[doc = " If this value is greater than 0, it indicates that this datagram has been sent with other"]
        #[doc = " segments in a single buffer."]
        #[doc = ""]
        #[doc = " See the [Linux kernel documentation](https://www.kernel.org/doc/html/latest/networking/segmentation-offloads.html#generic-segmentation-offload) for more details."]
        pub gso_offset: usize,
    }
    impl IntoEvent<api::DatagramSent> for DatagramSent {
        #[inline]
        fn into_event(self) -> api::DatagramSent {
            let DatagramSent { len, gso_offset } = self;
            api::DatagramSent {
                len: len.into_event(),
                gso_offset: gso_offset.into_event(),
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
    pub struct DatagramDropped<'a> {
        pub local_addr: SocketAddress<'a>,
        pub remote_addr: SocketAddress<'a>,
        pub destination_cid: ConnectionId<'a>,
        pub source_cid: Option<ConnectionId<'a>>,
        pub len: u16,
        pub reason: DatagramDropReason,
    }
    impl<'a> IntoEvent<api::DatagramDropped<'a>> for DatagramDropped<'a> {
        #[inline]
        fn into_event(self) -> api::DatagramDropped<'a> {
            let DatagramDropped {
                local_addr,
                remote_addr,
                destination_cid,
                source_cid,
                len,
                reason,
            } = self;
            api::DatagramDropped {
                local_addr: local_addr.into_event(),
                remote_addr: remote_addr.into_event(),
                destination_cid: destination_cid.into_event(),
                source_cid: source_cid.into_event(),
                len: len.into_event(),
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The remote address was changed before the handshake was complete"]
    pub struct HandshakeRemoteAddressChangeObserved<'a> {
        pub local_addr: SocketAddress<'a>,
        #[doc = " The newly observed remote address"]
        pub remote_addr: SocketAddress<'a>,
        #[doc = " The remote address established from the initial packet"]
        pub initial_remote_addr: SocketAddress<'a>,
    }
    impl<'a> IntoEvent<api::HandshakeRemoteAddressChangeObserved<'a>>
        for HandshakeRemoteAddressChangeObserved<'a>
    {
        #[inline]
        fn into_event(self) -> api::HandshakeRemoteAddressChangeObserved<'a> {
            let HandshakeRemoteAddressChangeObserved {
                local_addr,
                remote_addr,
                initial_remote_addr,
            } = self;
            api::HandshakeRemoteAddressChangeObserved {
                local_addr: local_addr.into_event(),
                remote_addr: remote_addr.into_event(),
                initial_remote_addr: initial_remote_addr.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " ConnectionId updated"]
    pub struct ConnectionIdUpdated<'a> {
        pub path_id: u64,
        #[doc = " The endpoint that updated its connection id"]
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
    pub struct EcnStateChanged<'a> {
        pub path: Path<'a>,
        pub state: EcnState,
    }
    impl<'a> IntoEvent<api::EcnStateChanged<'a>> for EcnStateChanged<'a> {
        #[inline]
        fn into_event(self) -> api::EcnStateChanged<'a> {
            let EcnStateChanged { path, state } = self;
            api::EcnStateChanged {
                path: path.into_event(),
                state: state.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ConnectionMigrationDenied {
        pub reason: MigrationDenyReason,
    }
    impl IntoEvent<api::ConnectionMigrationDenied> for ConnectionMigrationDenied {
        #[inline]
        fn into_event(self) -> api::ConnectionMigrationDenied {
            let ConnectionMigrationDenied { reason } = self;
            api::ConnectionMigrationDenied {
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct HandshakeStatusUpdated {
        pub status: HandshakeStatus,
    }
    impl IntoEvent<api::HandshakeStatusUpdated> for HandshakeStatusUpdated {
        #[inline]
        fn into_event(self) -> api::HandshakeStatusUpdated {
            let HandshakeStatusUpdated { status } = self;
            api::HandshakeStatusUpdated {
                status: status.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct TlsExporterReady<'a> {
        pub session: crate::event::TlsSession<'a>,
    }
    impl<'a> IntoEvent<api::TlsExporterReady<'a>> for TlsExporterReady<'a> {
        #[inline]
        fn into_event(self) -> api::TlsExporterReady<'a> {
            let TlsExporterReady { session } = self;
            api::TlsExporterReady {
                session: session.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct TlsHandshakeFailed<'a> {
        pub session: crate::event::TlsSession<'a>,
    }
    impl<'a> IntoEvent<api::TlsHandshakeFailed<'a>> for TlsHandshakeFailed<'a> {
        #[inline]
        fn into_event(self) -> api::TlsHandshakeFailed<'a> {
            let TlsHandshakeFailed { session } = self;
            api::TlsHandshakeFailed {
                session: session.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Path challenge updated"]
    pub struct PathChallengeUpdated<'a> {
        pub path_challenge_status: PathChallengeStatus,
        pub path: Path<'a>,
        pub challenge_data: &'a [u8],
    }
    impl<'a> IntoEvent<api::PathChallengeUpdated<'a>> for PathChallengeUpdated<'a> {
        #[inline]
        fn into_event(self) -> api::PathChallengeUpdated<'a> {
            let PathChallengeUpdated {
                path_challenge_status,
                path,
                challenge_data,
            } = self;
            api::PathChallengeUpdated {
                path_challenge_status: path_challenge_status.into_event(),
                path: path.into_event(),
                challenge_data: challenge_data.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct TlsClientHello<'a> {
        pub payload: &'a [&'a [u8]],
    }
    impl<'a> IntoEvent<api::TlsClientHello<'a>> for TlsClientHello<'a> {
        #[inline]
        fn into_event(self) -> api::TlsClientHello<'a> {
            let TlsClientHello { payload } = self;
            api::TlsClientHello {
                payload: payload.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct TlsServerHello<'a> {
        pub payload: &'a [&'a [u8]],
    }
    impl<'a> IntoEvent<api::TlsServerHello<'a>> for TlsServerHello<'a> {
        #[inline]
        fn into_event(self) -> api::TlsServerHello<'a> {
            let TlsServerHello { payload } = self;
            api::TlsServerHello {
                payload: payload.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct RxStreamProgress {
        pub bytes: usize,
    }
    impl IntoEvent<api::RxStreamProgress> for RxStreamProgress {
        #[inline]
        fn into_event(self) -> api::RxStreamProgress {
            let RxStreamProgress { bytes } = self;
            api::RxStreamProgress {
                bytes: bytes.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct TxStreamProgress {
        pub bytes: usize,
    }
    impl IntoEvent<api::TxStreamProgress> for TxStreamProgress {
        #[inline]
        fn into_event(self) -> api::TxStreamProgress {
            let TxStreamProgress { bytes } = self;
            api::TxStreamProgress {
                bytes: bytes.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct KeepAliveTimerExpired {
        pub timeout: Duration,
    }
    impl IntoEvent<api::KeepAliveTimerExpired> for KeepAliveTimerExpired {
        #[inline]
        fn into_event(self) -> api::KeepAliveTimerExpired {
            let KeepAliveTimerExpired { timeout } = self;
            api::KeepAliveTimerExpired {
                timeout: timeout.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The maximum transmission unit (MTU) and/or MTU probing status for the path has changed"]
    pub struct MtuUpdated {
        pub path_id: u64,
        #[doc = " The maximum QUIC datagram size, not including UDP and IP headers"]
        pub mtu: u16,
        pub cause: MtuUpdatedCause,
        #[doc = " The search for the maximum MTU has completed for now"]
        pub search_complete: bool,
    }
    impl IntoEvent<api::MtuUpdated> for MtuUpdated {
        #[inline]
        fn into_event(self) -> api::MtuUpdated {
            let MtuUpdated {
                path_id,
                mtu,
                cause,
                search_complete,
            } = self;
            api::MtuUpdated {
                path_id: path_id.into_event(),
                mtu: mtu.into_event(),
                cause: cause.into_event(),
                search_complete: search_complete.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The slow start congestion controller state has been exited"]
    pub struct SlowStartExited {
        pub path_id: u64,
        pub cause: SlowStartExitCause,
        pub congestion_window: u32,
    }
    impl IntoEvent<api::SlowStartExited> for SlowStartExited {
        #[inline]
        fn into_event(self) -> api::SlowStartExited {
            let SlowStartExited {
                path_id,
                cause,
                congestion_window,
            } = self;
            api::SlowStartExited {
                path_id: path_id.into_event(),
                cause: cause.into_event(),
                congestion_window: congestion_window.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " A new delivery rate sample has been generated"]
    #[doc = " Note: This event is only recorded for congestion controllers that support"]
    #[doc = "       bandwidth estimates, such as BBR"]
    pub struct DeliveryRateSampled {
        pub path_id: u64,
        pub rate_sample: RateSample,
    }
    impl IntoEvent<api::DeliveryRateSampled> for DeliveryRateSampled {
        #[inline]
        fn into_event(self) -> api::DeliveryRateSampled {
            let DeliveryRateSampled {
                path_id,
                rate_sample,
            } = self;
            api::DeliveryRateSampled {
                path_id: path_id.into_event(),
                rate_sample: rate_sample.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The pacing rate has been updated"]
    pub struct PacingRateUpdated {
        pub path_id: u64,
        pub bytes_per_second: u64,
        pub burst_size: u32,
        pub pacing_gain: f32,
    }
    impl IntoEvent<api::PacingRateUpdated> for PacingRateUpdated {
        #[inline]
        fn into_event(self) -> api::PacingRateUpdated {
            let PacingRateUpdated {
                path_id,
                bytes_per_second,
                burst_size,
                pacing_gain,
            } = self;
            api::PacingRateUpdated {
                path_id: path_id.into_event(),
                bytes_per_second: bytes_per_second.into_event(),
                burst_size: burst_size.into_event(),
                pacing_gain: pacing_gain.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The BBR state has changed"]
    pub struct BbrStateChanged {
        pub path_id: u64,
        pub state: BbrState,
    }
    impl IntoEvent<api::BbrStateChanged> for BbrStateChanged {
        #[inline]
        fn into_event(self) -> api::BbrStateChanged {
            let BbrStateChanged { path_id, state } = self;
            api::BbrStateChanged {
                path_id: path_id.into_event(),
                state: state.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The DC state has changed"]
    pub struct DcStateChanged {
        pub state: DcState,
    }
    impl IntoEvent<api::DcStateChanged> for DcStateChanged {
        #[inline]
        fn into_event(self) -> api::DcStateChanged {
            let DcStateChanged { state } = self;
            api::DcStateChanged {
                state: state.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " The DC path has been created"]
    pub struct DcPathCreated<'a> {
        #[doc = " This is the dc::Path struct, it's just type-erased. But if an event subscriber knows the"]
        #[doc = " type they can downcast."]
        pub path: &'a (dyn core::any::Any + Send + 'static),
    }
    impl<'a> IntoEvent<api::DcPathCreated<'a>> for DcPathCreated<'a> {
        #[inline]
        fn into_event(self) -> api::DcPathCreated<'a> {
            let DcPathCreated { path } = self;
            api::DcPathCreated {
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
        #[doc = " The GSO offset at which this datagram was written"]
        #[doc = ""]
        #[doc = " If this value is greater than 0, it indicates that this datagram has been sent with other"]
        #[doc = " segments in a single buffer."]
        #[doc = ""]
        #[doc = " See the [Linux kernel documentation](https://www.kernel.org/doc/html/latest/networking/segmentation-offloads.html#generic-segmentation-offload) for more details."]
        pub gso_offset: usize,
    }
    impl IntoEvent<api::EndpointDatagramSent> for EndpointDatagramSent {
        #[inline]
        fn into_event(self) -> api::EndpointDatagramSent {
            let EndpointDatagramSent { len, gso_offset } = self;
            api::EndpointDatagramSent {
                len: len.into_event(),
                gso_offset: gso_offset.into_event(),
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
        pub reason: DatagramDropReason,
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
    #[derive(Clone, Debug)]
    pub struct EndpointConnectionAttemptFailed {
        pub error: crate::connection::Error,
    }
    impl IntoEvent<api::EndpointConnectionAttemptFailed> for EndpointConnectionAttemptFailed {
        #[inline]
        fn into_event(self) -> api::EndpointConnectionAttemptFailed {
            let EndpointConnectionAttemptFailed { error } = self;
            api::EndpointConnectionAttemptFailed {
                error: error.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointConnectionAttemptDeduplicated {
        #[doc = " The internal connection ID this deduplicated with."]
        pub connection_id: u64,
        pub already_open: bool,
    }
    impl IntoEvent<api::EndpointConnectionAttemptDeduplicated>
        for EndpointConnectionAttemptDeduplicated
    {
        #[inline]
        fn into_event(self) -> api::EndpointConnectionAttemptDeduplicated {
            let EndpointConnectionAttemptDeduplicated {
                connection_id,
                already_open,
            } = self;
            api::EndpointConnectionAttemptDeduplicated {
                connection_id: connection_id.into_event(),
                already_open: already_open.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the platform sends at least one packet"]
    pub struct PlatformTx {
        #[doc = " The number of packets sent"]
        pub count: usize,
        #[doc = " The number of syscalls performed"]
        pub syscalls: usize,
        #[doc = " The number of syscalls that got blocked"]
        pub blocked_syscalls: usize,
        #[doc = " The total number of errors encountered since the last event"]
        pub total_errors: usize,
        #[doc = " The number of specific error codes dropped"]
        #[doc = ""]
        #[doc = " This can happen when a burst of errors exceeds the capacity of the recorder"]
        pub dropped_errors: usize,
    }
    impl IntoEvent<api::PlatformTx> for PlatformTx {
        #[inline]
        fn into_event(self) -> api::PlatformTx {
            let PlatformTx {
                count,
                syscalls,
                blocked_syscalls,
                total_errors,
                dropped_errors,
            } = self;
            api::PlatformTx {
                count: count.into_event(),
                syscalls: syscalls.into_event(),
                blocked_syscalls: blocked_syscalls.into_event(),
                total_errors: total_errors.into_event(),
                dropped_errors: dropped_errors.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the platform returns an error while sending datagrams"]
    pub struct PlatformTxError {
        #[doc = " The error code returned by the platform"]
        pub errno: i32,
    }
    impl IntoEvent<api::PlatformTxError> for PlatformTxError {
        #[inline]
        fn into_event(self) -> api::PlatformTxError {
            let PlatformTxError { errno } = self;
            api::PlatformTxError {
                errno: errno.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the platform receives at least one packet"]
    pub struct PlatformRx {
        #[doc = " The number of packets received"]
        pub count: usize,
        #[doc = " The number of syscalls performed"]
        pub syscalls: usize,
        #[doc = " The number of syscalls that got blocked"]
        pub blocked_syscalls: usize,
        #[doc = " The total number of errors encountered since the last event"]
        pub total_errors: usize,
        #[doc = " The number of specific error codes dropped"]
        #[doc = ""]
        #[doc = " This can happen when a burst of errors exceeds the capacity of the recorder"]
        pub dropped_errors: usize,
    }
    impl IntoEvent<api::PlatformRx> for PlatformRx {
        #[inline]
        fn into_event(self) -> api::PlatformRx {
            let PlatformRx {
                count,
                syscalls,
                blocked_syscalls,
                total_errors,
                dropped_errors,
            } = self;
            api::PlatformRx {
                count: count.into_event(),
                syscalls: syscalls.into_event(),
                blocked_syscalls: blocked_syscalls.into_event(),
                total_errors: total_errors.into_event(),
                dropped_errors: dropped_errors.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the platform returns an error while receiving datagrams"]
    pub struct PlatformRxError {
        #[doc = " The error code returned by the platform"]
        pub errno: i32,
    }
    impl IntoEvent<api::PlatformRxError> for PlatformRxError {
        #[inline]
        fn into_event(self) -> api::PlatformRxError {
            let PlatformRxError { errno } = self;
            api::PlatformRxError {
                errno: errno.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a platform feature is configured"]
    pub struct PlatformFeatureConfigured {
        pub configuration: PlatformFeatureConfiguration,
    }
    impl IntoEvent<api::PlatformFeatureConfigured> for PlatformFeatureConfigured {
        #[inline]
        fn into_event(self) -> api::PlatformFeatureConfigured {
            let PlatformFeatureConfigured { configuration } = self;
            api::PlatformFeatureConfigured {
                configuration: configuration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PlatformEventLoopWakeup {
        pub timeout_expired: bool,
        pub rx_ready: bool,
        pub tx_ready: bool,
        pub application_wakeup: bool,
    }
    impl IntoEvent<api::PlatformEventLoopWakeup> for PlatformEventLoopWakeup {
        #[inline]
        fn into_event(self) -> api::PlatformEventLoopWakeup {
            let PlatformEventLoopWakeup {
                timeout_expired,
                rx_ready,
                tx_ready,
                application_wakeup,
            } = self;
            api::PlatformEventLoopWakeup {
                timeout_expired: timeout_expired.into_event(),
                rx_ready: rx_ready.into_event(),
                tx_ready: tx_ready.into_event(),
                application_wakeup: application_wakeup.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PlatformEventLoopSleep {
        #[doc = " The next time at which the event loop will wake"]
        pub timeout: Option<core::time::Duration>,
        #[doc = " The amount of time spent processing endpoint events in a single event loop"]
        pub processing_duration: core::time::Duration,
    }
    impl IntoEvent<api::PlatformEventLoopSleep> for PlatformEventLoopSleep {
        #[inline]
        fn into_event(self) -> api::PlatformEventLoopSleep {
            let PlatformEventLoopSleep {
                timeout,
                processing_duration,
            } = self;
            api::PlatformEventLoopSleep {
                timeout: timeout.into_event(),
                processing_duration: processing_duration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PlatformEventLoopStarted<'a> {
        #[doc = " The local address of the socket"]
        pub local_address: SocketAddress<'a>,
    }
    impl<'a> IntoEvent<api::PlatformEventLoopStarted<'a>> for PlatformEventLoopStarted<'a> {
        #[inline]
        fn into_event(self) -> api::PlatformEventLoopStarted<'a> {
            let PlatformEventLoopStarted { local_address } = self;
            api::PlatformEventLoopStarted {
                local_address: local_address.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum PlatformFeatureConfiguration {
        #[doc = " Emitted when segment offload was configured"]
        Gso {
            #[doc = " The maximum number of segments that can be sent in a single GSO packet"]
            #[doc = ""]
            #[doc = " If this value not greater than 1, GSO is disabled."]
            max_segments: usize,
        },
        #[doc = " Emitted when receive segment offload was configured"]
        Gro { enabled: bool },
        #[doc = " Emitted when ECN support is configured"]
        Ecn { enabled: bool },
        #[doc = " Emitted when the base maximum transmission unit is configured"]
        BaseMtu { mtu: u16 },
        #[doc = " Emitted when the initial maximum transmission unit is configured"]
        InitialMtu { mtu: u16 },
        #[doc = " Emitted when the max maximum transmission unit is configured"]
        MaxMtu { mtu: u16 },
    }
    impl IntoEvent<api::PlatformFeatureConfiguration> for PlatformFeatureConfiguration {
        #[inline]
        fn into_event(self) -> api::PlatformFeatureConfiguration {
            use api::PlatformFeatureConfiguration::*;
            match self {
                Self::Gso { max_segments } => Gso {
                    max_segments: max_segments.into_event(),
                },
                Self::Gro { enabled } => Gro {
                    enabled: enabled.into_event(),
                },
                Self::Ecn { enabled } => Ecn {
                    enabled: enabled.into_event(),
                },
                Self::BaseMtu { mtu } => BaseMtu {
                    mtu: mtu.into_event(),
                },
                Self::InitialMtu { mtu } => InitialMtu {
                    mtu: mtu.into_event(),
                },
                Self::MaxMtu { mtu } => MaxMtu {
                    mtu: mtu.into_event(),
                },
            }
        }
    }
}
pub mod supervisor {
    #![doc = r" This module contains the `supervisor::Outcome` and `supervisor::Context` for use"]
    #![doc = r" when implementing [`Subscriber::supervisor_timeout`](crate::event::Subscriber::supervisor_timeout) and"]
    #![doc = r" [`Subscriber::on_supervisor_timeout`](crate::event::Subscriber::on_supervisor_timeout)"]
    #![doc = r" on a Subscriber."]
    use crate::{
        application,
        event::{builder::SocketAddress, IntoEvent},
    };
    #[non_exhaustive]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum Outcome {
        #[doc = r" Allow the connection to remain open"]
        Continue,
        #[doc = r" Close the connection and notify the peer"]
        Close { error_code: application::Error },
        #[doc = r" Close the connection without notifying the peer"]
        ImmediateClose { reason: &'static str },
    }
    impl Default for Outcome {
        fn default() -> Self {
            Self::Continue
        }
    }
    #[non_exhaustive]
    #[derive(Debug)]
    pub struct Context<'a> {
        #[doc = r" Number of handshakes that have begun but not completed"]
        pub inflight_handshakes: usize,
        #[doc = r" Number of open connections"]
        pub connection_count: usize,
        #[doc = r" The address of the peer"]
        pub remote_address: SocketAddress<'a>,
        #[doc = r" True if the connection is in the handshake state, false otherwise"]
        pub is_handshaking: bool,
    }
    impl<'a> Context<'a> {
        pub fn new(
            inflight_handshakes: usize,
            connection_count: usize,
            remote_address: &'a crate::inet::SocketAddress,
            is_handshaking: bool,
        ) -> Self {
            Self {
                inflight_handshakes,
                connection_count,
                remote_address: remote_address.into_event(),
                is_handshaking,
            }
        }
    }
}
pub use traits::*;
mod traits {
    use super::*;
    use crate::{event::Meta, query};
    use core::fmt;
    #[doc = r" Allows for events to be subscribed to"]
    pub trait Subscriber: 'static + Send {
        #[doc = r" An application provided type associated with each connection."]
        #[doc = r""]
        #[doc = r" The context provides a mechanism for applications to provide a custom type"]
        #[doc = r" and update it on each event, e.g. computing statistics. Each event"]
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
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext;
        #[doc = r" The period at which `on_supervisor_timeout` is called"]
        #[doc = r""]
        #[doc = r" If multiple `event::Subscriber`s are composed together, the minimum `supervisor_timeout`"]
        #[doc = r" across all `event::Subscriber`s will be used."]
        #[doc = r""]
        #[doc = r" If the `supervisor_timeout()` is `None` across all `event::Subscriber`s, connection supervision"]
        #[doc = r" will cease for the remaining lifetime of the connection and `on_supervisor_timeout` will no longer"]
        #[doc = r" be called."]
        #[doc = r""]
        #[doc = r" It is recommended to avoid setting this value less than ~100ms, as short durations"]
        #[doc = r" may lead to higher CPU utilization."]
        #[allow(unused_variables)]
        fn supervisor_timeout(
            &mut self,
            conn_context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            context: &supervisor::Context,
        ) -> Option<Duration> {
            None
        }
        #[doc = r" Called for each `supervisor_timeout` to determine any action to take on the connection based on the `supervisor::Outcome`"]
        #[doc = r""]
        #[doc = r" If multiple `event::Subscriber`s are composed together, the minimum `supervisor_timeout`"]
        #[doc = r" across all `event::Subscriber`s will be used, and thus `on_supervisor_timeout` may be called"]
        #[doc = r" earlier than the `supervisor_timeout` for a given `event::Subscriber` implementation."]
        #[allow(unused_variables)]
        fn on_supervisor_timeout(
            &mut self,
            conn_context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            context: &supervisor::Context,
        ) -> supervisor::Outcome {
            supervisor::Outcome::default()
        }
        #[doc = "Called when the `ApplicationProtocolInformation` event is triggered"]
        #[inline]
        fn on_application_protocol_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationProtocolInformation,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ServerNameInformation` event is triggered"]
        #[inline]
        fn on_server_name_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ServerNameInformation,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `KeyExchangeGroup` event is triggered"]
        #[inline]
        fn on_key_exchange_group(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeyExchangeGroup,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PacketSkipped` event is triggered"]
        #[inline]
        fn on_packet_skipped(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketSkipped,
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
            meta: &api::ConnectionMeta,
            event: &api::PacketSent,
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
            meta: &api::ConnectionMeta,
            event: &api::PacketReceived,
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
            meta: &api::ConnectionMeta,
            event: &api::ActivePathUpdated,
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
            meta: &api::ConnectionMeta,
            event: &api::PathCreated,
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
            meta: &api::ConnectionMeta,
            event: &api::FrameSent,
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
            meta: &api::ConnectionMeta,
            event: &api::FrameReceived,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ConnectionCloseFrameReceived` event is triggered"]
        #[inline]
        fn on_connection_close_frame_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionCloseFrameReceived,
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
            meta: &api::ConnectionMeta,
            event: &api::PacketLost,
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
            meta: &api::ConnectionMeta,
            event: &api::RecoveryMetrics,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `Congestion` event is triggered"]
        #[inline]
        fn on_congestion(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::Congestion,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AckProcessed` event is triggered"]
        #[inline]
        #[deprecated(note = "use on_rx_ack_range_dropped event instead")]
        #[allow(deprecated)]
        fn on_ack_processed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::AckProcessed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `RxAckRangeDropped` event is triggered"]
        #[inline]
        fn on_rx_ack_range_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::RxAckRangeDropped,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AckRangeReceived` event is triggered"]
        #[inline]
        fn on_ack_range_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::AckRangeReceived,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AckRangeSent` event is triggered"]
        #[inline]
        fn on_ack_range_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::AckRangeSent,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PacketDropped` event is triggered"]
        #[inline]
        fn on_packet_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketDropped,
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
            meta: &api::ConnectionMeta,
            event: &api::KeyUpdate,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `KeySpaceDiscarded` event is triggered"]
        #[inline]
        fn on_key_space_discarded(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeySpaceDiscarded,
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
            meta: &api::ConnectionMeta,
            event: &api::ConnectionStarted,
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
            meta: &api::ConnectionMeta,
            event: &api::DuplicatePacket,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `TransportParametersReceived` event is triggered"]
        #[inline]
        fn on_transport_parameters_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TransportParametersReceived,
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
            meta: &api::ConnectionMeta,
            event: &api::DatagramSent,
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
            meta: &api::ConnectionMeta,
            event: &api::DatagramReceived,
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
            meta: &api::ConnectionMeta,
            event: &api::DatagramDropped,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `HandshakeRemoteAddressChangeObserved` event is triggered"]
        #[inline]
        fn on_handshake_remote_address_change_observed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::HandshakeRemoteAddressChangeObserved,
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
            meta: &api::ConnectionMeta,
            event: &api::ConnectionIdUpdated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EcnStateChanged` event is triggered"]
        #[inline]
        fn on_ecn_state_changed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::EcnStateChanged,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ConnectionMigrationDenied` event is triggered"]
        #[inline]
        fn on_connection_migration_denied(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionMigrationDenied,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `HandshakeStatusUpdated` event is triggered"]
        #[inline]
        fn on_handshake_status_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::HandshakeStatusUpdated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `TlsExporterReady` event is triggered"]
        #[inline]
        fn on_tls_exporter_ready(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsExporterReady,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `TlsHandshakeFailed` event is triggered"]
        #[inline]
        fn on_tls_handshake_failed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsHandshakeFailed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathChallengeUpdated` event is triggered"]
        #[inline]
        fn on_path_challenge_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PathChallengeUpdated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `TlsClientHello` event is triggered"]
        #[inline]
        fn on_tls_client_hello(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsClientHello,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `TlsServerHello` event is triggered"]
        #[inline]
        fn on_tls_server_hello(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsServerHello,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `RxStreamProgress` event is triggered"]
        #[inline]
        fn on_rx_stream_progress(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::RxStreamProgress,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `TxStreamProgress` event is triggered"]
        #[inline]
        fn on_tx_stream_progress(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TxStreamProgress,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `KeepAliveTimerExpired` event is triggered"]
        #[inline]
        fn on_keep_alive_timer_expired(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeepAliveTimerExpired,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `MtuUpdated` event is triggered"]
        #[inline]
        fn on_mtu_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::MtuUpdated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `SlowStartExited` event is triggered"]
        #[inline]
        fn on_slow_start_exited(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::SlowStartExited,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `DeliveryRateSampled` event is triggered"]
        #[inline]
        fn on_delivery_rate_sampled(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DeliveryRateSampled,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PacingRateUpdated` event is triggered"]
        #[inline]
        fn on_pacing_rate_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacingRateUpdated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `BbrStateChanged` event is triggered"]
        #[inline]
        fn on_bbr_state_changed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::BbrStateChanged,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `DcStateChanged` event is triggered"]
        #[inline]
        fn on_dc_state_changed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DcStateChanged,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `DcPathCreated` event is triggered"]
        #[inline]
        fn on_dc_path_created(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DcPathCreated,
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
            meta: &api::ConnectionMeta,
            event: &api::ConnectionClosed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `VersionInformation` event is triggered"]
        #[inline]
        fn on_version_information(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::VersionInformation,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointPacketSent` event is triggered"]
        #[inline]
        fn on_endpoint_packet_sent(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointPacketSent,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointPacketReceived` event is triggered"]
        #[inline]
        fn on_endpoint_packet_received(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointPacketReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointDatagramSent` event is triggered"]
        #[inline]
        fn on_endpoint_datagram_sent(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramSent,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointDatagramReceived` event is triggered"]
        #[inline]
        fn on_endpoint_datagram_received(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointDatagramDropped` event is triggered"]
        #[inline]
        fn on_endpoint_datagram_dropped(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramDropped,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointConnectionAttemptFailed` event is triggered"]
        #[inline]
        fn on_endpoint_connection_attempt_failed(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointConnectionAttemptFailed,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointConnectionAttemptDeduplicated` event is triggered"]
        #[inline]
        fn on_endpoint_connection_attempt_deduplicated(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointConnectionAttemptDeduplicated,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PlatformTx` event is triggered"]
        #[inline]
        fn on_platform_tx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTx) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PlatformTxError` event is triggered"]
        #[inline]
        fn on_platform_tx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTxError) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PlatformRx` event is triggered"]
        #[inline]
        fn on_platform_rx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRx) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PlatformRxError` event is triggered"]
        #[inline]
        fn on_platform_rx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRxError) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PlatformFeatureConfigured` event is triggered"]
        #[inline]
        fn on_platform_feature_configured(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformFeatureConfigured,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PlatformEventLoopWakeup` event is triggered"]
        #[inline]
        fn on_platform_event_loop_wakeup(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopWakeup,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PlatformEventLoopSleep` event is triggered"]
        #[inline]
        fn on_platform_event_loop_sleep(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopSleep,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PlatformEventLoopStarted` event is triggered"]
        #[inline]
        fn on_platform_event_loop_started(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopStarted,
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
            meta: &api::ConnectionMeta,
            event: &E,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Used for querying the `Subscriber::ConnectionContext` on a Subscriber"]
        #[inline]
        fn query(
            context: &Self::ConnectionContext,
            query: &mut dyn query::Query,
        ) -> query::ControlFlow {
            query.execute(context)
        }
        #[doc = r" Used for querying and mutating the `Subscriber::ConnectionContext` on a Subscriber"]
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
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            (
                self.0.create_connection_context(meta, info),
                self.1.create_connection_context(meta, info),
            )
        }
        #[inline]
        fn supervisor_timeout(
            &mut self,
            conn_context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            context: &supervisor::Context,
        ) -> Option<Duration> {
            let timeout_a = self
                .0
                .supervisor_timeout(&mut conn_context.0, meta, context);
            let timeout_b = self
                .1
                .supervisor_timeout(&mut conn_context.1, meta, context);
            match (timeout_a, timeout_b) {
                (None, None) => None,
                (None, Some(timeout)) | (Some(timeout), None) => Some(timeout),
                (Some(a), Some(b)) => Some(a.min(b)),
            }
        }
        #[inline]
        fn on_supervisor_timeout(
            &mut self,
            conn_context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            context: &supervisor::Context,
        ) -> supervisor::Outcome {
            let outcome_a = self
                .0
                .on_supervisor_timeout(&mut conn_context.0, meta, context);
            let outcome_b = self
                .1
                .on_supervisor_timeout(&mut conn_context.1, meta, context);
            match (outcome_a, outcome_b) {
                (supervisor::Outcome::ImmediateClose { reason }, _)
                | (_, supervisor::Outcome::ImmediateClose { reason }) => {
                    supervisor::Outcome::ImmediateClose { reason }
                }
                (supervisor::Outcome::Close { error_code }, _)
                | (_, supervisor::Outcome::Close { error_code }) => {
                    supervisor::Outcome::Close { error_code }
                }
                _ => supervisor::Outcome::Continue,
            }
        }
        #[inline]
        fn on_application_protocol_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationProtocolInformation,
        ) {
            (self.0).on_application_protocol_information(&mut context.0, meta, event);
            (self.1).on_application_protocol_information(&mut context.1, meta, event);
        }
        #[inline]
        fn on_server_name_information(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ServerNameInformation,
        ) {
            (self.0).on_server_name_information(&mut context.0, meta, event);
            (self.1).on_server_name_information(&mut context.1, meta, event);
        }
        #[inline]
        fn on_key_exchange_group(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeyExchangeGroup,
        ) {
            (self.0).on_key_exchange_group(&mut context.0, meta, event);
            (self.1).on_key_exchange_group(&mut context.1, meta, event);
        }
        #[inline]
        fn on_packet_skipped(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketSkipped,
        ) {
            (self.0).on_packet_skipped(&mut context.0, meta, event);
            (self.1).on_packet_skipped(&mut context.1, meta, event);
        }
        #[inline]
        fn on_packet_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketSent,
        ) {
            (self.0).on_packet_sent(&mut context.0, meta, event);
            (self.1).on_packet_sent(&mut context.1, meta, event);
        }
        #[inline]
        fn on_packet_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketReceived,
        ) {
            (self.0).on_packet_received(&mut context.0, meta, event);
            (self.1).on_packet_received(&mut context.1, meta, event);
        }
        #[inline]
        fn on_active_path_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ActivePathUpdated,
        ) {
            (self.0).on_active_path_updated(&mut context.0, meta, event);
            (self.1).on_active_path_updated(&mut context.1, meta, event);
        }
        #[inline]
        fn on_path_created(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PathCreated,
        ) {
            (self.0).on_path_created(&mut context.0, meta, event);
            (self.1).on_path_created(&mut context.1, meta, event);
        }
        #[inline]
        fn on_frame_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::FrameSent,
        ) {
            (self.0).on_frame_sent(&mut context.0, meta, event);
            (self.1).on_frame_sent(&mut context.1, meta, event);
        }
        #[inline]
        fn on_frame_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::FrameReceived,
        ) {
            (self.0).on_frame_received(&mut context.0, meta, event);
            (self.1).on_frame_received(&mut context.1, meta, event);
        }
        #[inline]
        fn on_connection_close_frame_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionCloseFrameReceived,
        ) {
            (self.0).on_connection_close_frame_received(&mut context.0, meta, event);
            (self.1).on_connection_close_frame_received(&mut context.1, meta, event);
        }
        #[inline]
        fn on_packet_lost(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketLost,
        ) {
            (self.0).on_packet_lost(&mut context.0, meta, event);
            (self.1).on_packet_lost(&mut context.1, meta, event);
        }
        #[inline]
        fn on_recovery_metrics(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::RecoveryMetrics,
        ) {
            (self.0).on_recovery_metrics(&mut context.0, meta, event);
            (self.1).on_recovery_metrics(&mut context.1, meta, event);
        }
        #[inline]
        fn on_congestion(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::Congestion,
        ) {
            (self.0).on_congestion(&mut context.0, meta, event);
            (self.1).on_congestion(&mut context.1, meta, event);
        }
        #[inline]
        #[allow(deprecated)]
        fn on_ack_processed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::AckProcessed,
        ) {
            (self.0).on_ack_processed(&mut context.0, meta, event);
            (self.1).on_ack_processed(&mut context.1, meta, event);
        }
        #[inline]
        fn on_rx_ack_range_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::RxAckRangeDropped,
        ) {
            (self.0).on_rx_ack_range_dropped(&mut context.0, meta, event);
            (self.1).on_rx_ack_range_dropped(&mut context.1, meta, event);
        }
        #[inline]
        fn on_ack_range_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::AckRangeReceived,
        ) {
            (self.0).on_ack_range_received(&mut context.0, meta, event);
            (self.1).on_ack_range_received(&mut context.1, meta, event);
        }
        #[inline]
        fn on_ack_range_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::AckRangeSent,
        ) {
            (self.0).on_ack_range_sent(&mut context.0, meta, event);
            (self.1).on_ack_range_sent(&mut context.1, meta, event);
        }
        #[inline]
        fn on_packet_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketDropped,
        ) {
            (self.0).on_packet_dropped(&mut context.0, meta, event);
            (self.1).on_packet_dropped(&mut context.1, meta, event);
        }
        #[inline]
        fn on_key_update(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeyUpdate,
        ) {
            (self.0).on_key_update(&mut context.0, meta, event);
            (self.1).on_key_update(&mut context.1, meta, event);
        }
        #[inline]
        fn on_key_space_discarded(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeySpaceDiscarded,
        ) {
            (self.0).on_key_space_discarded(&mut context.0, meta, event);
            (self.1).on_key_space_discarded(&mut context.1, meta, event);
        }
        #[inline]
        fn on_connection_started(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionStarted,
        ) {
            (self.0).on_connection_started(&mut context.0, meta, event);
            (self.1).on_connection_started(&mut context.1, meta, event);
        }
        #[inline]
        fn on_duplicate_packet(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DuplicatePacket,
        ) {
            (self.0).on_duplicate_packet(&mut context.0, meta, event);
            (self.1).on_duplicate_packet(&mut context.1, meta, event);
        }
        #[inline]
        fn on_transport_parameters_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TransportParametersReceived,
        ) {
            (self.0).on_transport_parameters_received(&mut context.0, meta, event);
            (self.1).on_transport_parameters_received(&mut context.1, meta, event);
        }
        #[inline]
        fn on_datagram_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DatagramSent,
        ) {
            (self.0).on_datagram_sent(&mut context.0, meta, event);
            (self.1).on_datagram_sent(&mut context.1, meta, event);
        }
        #[inline]
        fn on_datagram_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DatagramReceived,
        ) {
            (self.0).on_datagram_received(&mut context.0, meta, event);
            (self.1).on_datagram_received(&mut context.1, meta, event);
        }
        #[inline]
        fn on_datagram_dropped(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DatagramDropped,
        ) {
            (self.0).on_datagram_dropped(&mut context.0, meta, event);
            (self.1).on_datagram_dropped(&mut context.1, meta, event);
        }
        #[inline]
        fn on_handshake_remote_address_change_observed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::HandshakeRemoteAddressChangeObserved,
        ) {
            (self.0).on_handshake_remote_address_change_observed(&mut context.0, meta, event);
            (self.1).on_handshake_remote_address_change_observed(&mut context.1, meta, event);
        }
        #[inline]
        fn on_connection_id_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionIdUpdated,
        ) {
            (self.0).on_connection_id_updated(&mut context.0, meta, event);
            (self.1).on_connection_id_updated(&mut context.1, meta, event);
        }
        #[inline]
        fn on_ecn_state_changed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::EcnStateChanged,
        ) {
            (self.0).on_ecn_state_changed(&mut context.0, meta, event);
            (self.1).on_ecn_state_changed(&mut context.1, meta, event);
        }
        #[inline]
        fn on_connection_migration_denied(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionMigrationDenied,
        ) {
            (self.0).on_connection_migration_denied(&mut context.0, meta, event);
            (self.1).on_connection_migration_denied(&mut context.1, meta, event);
        }
        #[inline]
        fn on_handshake_status_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::HandshakeStatusUpdated,
        ) {
            (self.0).on_handshake_status_updated(&mut context.0, meta, event);
            (self.1).on_handshake_status_updated(&mut context.1, meta, event);
        }
        #[inline]
        fn on_tls_exporter_ready(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsExporterReady,
        ) {
            (self.0).on_tls_exporter_ready(&mut context.0, meta, event);
            (self.1).on_tls_exporter_ready(&mut context.1, meta, event);
        }
        #[inline]
        fn on_tls_handshake_failed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsHandshakeFailed,
        ) {
            (self.0).on_tls_handshake_failed(&mut context.0, meta, event);
            (self.1).on_tls_handshake_failed(&mut context.1, meta, event);
        }
        #[inline]
        fn on_path_challenge_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PathChallengeUpdated,
        ) {
            (self.0).on_path_challenge_updated(&mut context.0, meta, event);
            (self.1).on_path_challenge_updated(&mut context.1, meta, event);
        }
        #[inline]
        fn on_tls_client_hello(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsClientHello,
        ) {
            (self.0).on_tls_client_hello(&mut context.0, meta, event);
            (self.1).on_tls_client_hello(&mut context.1, meta, event);
        }
        #[inline]
        fn on_tls_server_hello(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsServerHello,
        ) {
            (self.0).on_tls_server_hello(&mut context.0, meta, event);
            (self.1).on_tls_server_hello(&mut context.1, meta, event);
        }
        #[inline]
        fn on_rx_stream_progress(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::RxStreamProgress,
        ) {
            (self.0).on_rx_stream_progress(&mut context.0, meta, event);
            (self.1).on_rx_stream_progress(&mut context.1, meta, event);
        }
        #[inline]
        fn on_tx_stream_progress(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TxStreamProgress,
        ) {
            (self.0).on_tx_stream_progress(&mut context.0, meta, event);
            (self.1).on_tx_stream_progress(&mut context.1, meta, event);
        }
        #[inline]
        fn on_keep_alive_timer_expired(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeepAliveTimerExpired,
        ) {
            (self.0).on_keep_alive_timer_expired(&mut context.0, meta, event);
            (self.1).on_keep_alive_timer_expired(&mut context.1, meta, event);
        }
        #[inline]
        fn on_mtu_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::MtuUpdated,
        ) {
            (self.0).on_mtu_updated(&mut context.0, meta, event);
            (self.1).on_mtu_updated(&mut context.1, meta, event);
        }
        #[inline]
        fn on_slow_start_exited(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::SlowStartExited,
        ) {
            (self.0).on_slow_start_exited(&mut context.0, meta, event);
            (self.1).on_slow_start_exited(&mut context.1, meta, event);
        }
        #[inline]
        fn on_delivery_rate_sampled(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DeliveryRateSampled,
        ) {
            (self.0).on_delivery_rate_sampled(&mut context.0, meta, event);
            (self.1).on_delivery_rate_sampled(&mut context.1, meta, event);
        }
        #[inline]
        fn on_pacing_rate_updated(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacingRateUpdated,
        ) {
            (self.0).on_pacing_rate_updated(&mut context.0, meta, event);
            (self.1).on_pacing_rate_updated(&mut context.1, meta, event);
        }
        #[inline]
        fn on_bbr_state_changed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::BbrStateChanged,
        ) {
            (self.0).on_bbr_state_changed(&mut context.0, meta, event);
            (self.1).on_bbr_state_changed(&mut context.1, meta, event);
        }
        #[inline]
        fn on_dc_state_changed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DcStateChanged,
        ) {
            (self.0).on_dc_state_changed(&mut context.0, meta, event);
            (self.1).on_dc_state_changed(&mut context.1, meta, event);
        }
        #[inline]
        fn on_dc_path_created(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DcPathCreated,
        ) {
            (self.0).on_dc_path_created(&mut context.0, meta, event);
            (self.1).on_dc_path_created(&mut context.1, meta, event);
        }
        #[inline]
        fn on_connection_closed(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionClosed,
        ) {
            (self.0).on_connection_closed(&mut context.0, meta, event);
            (self.1).on_connection_closed(&mut context.1, meta, event);
        }
        #[inline]
        fn on_version_information(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::VersionInformation,
        ) {
            (self.0).on_version_information(meta, event);
            (self.1).on_version_information(meta, event);
        }
        #[inline]
        fn on_endpoint_packet_sent(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointPacketSent,
        ) {
            (self.0).on_endpoint_packet_sent(meta, event);
            (self.1).on_endpoint_packet_sent(meta, event);
        }
        #[inline]
        fn on_endpoint_packet_received(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointPacketReceived,
        ) {
            (self.0).on_endpoint_packet_received(meta, event);
            (self.1).on_endpoint_packet_received(meta, event);
        }
        #[inline]
        fn on_endpoint_datagram_sent(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramSent,
        ) {
            (self.0).on_endpoint_datagram_sent(meta, event);
            (self.1).on_endpoint_datagram_sent(meta, event);
        }
        #[inline]
        fn on_endpoint_datagram_received(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramReceived,
        ) {
            (self.0).on_endpoint_datagram_received(meta, event);
            (self.1).on_endpoint_datagram_received(meta, event);
        }
        #[inline]
        fn on_endpoint_datagram_dropped(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramDropped,
        ) {
            (self.0).on_endpoint_datagram_dropped(meta, event);
            (self.1).on_endpoint_datagram_dropped(meta, event);
        }
        #[inline]
        fn on_endpoint_connection_attempt_failed(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointConnectionAttemptFailed,
        ) {
            (self.0).on_endpoint_connection_attempt_failed(meta, event);
            (self.1).on_endpoint_connection_attempt_failed(meta, event);
        }
        #[inline]
        fn on_endpoint_connection_attempt_deduplicated(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointConnectionAttemptDeduplicated,
        ) {
            (self.0).on_endpoint_connection_attempt_deduplicated(meta, event);
            (self.1).on_endpoint_connection_attempt_deduplicated(meta, event);
        }
        #[inline]
        fn on_platform_tx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTx) {
            (self.0).on_platform_tx(meta, event);
            (self.1).on_platform_tx(meta, event);
        }
        #[inline]
        fn on_platform_tx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTxError) {
            (self.0).on_platform_tx_error(meta, event);
            (self.1).on_platform_tx_error(meta, event);
        }
        #[inline]
        fn on_platform_rx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRx) {
            (self.0).on_platform_rx(meta, event);
            (self.1).on_platform_rx(meta, event);
        }
        #[inline]
        fn on_platform_rx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRxError) {
            (self.0).on_platform_rx_error(meta, event);
            (self.1).on_platform_rx_error(meta, event);
        }
        #[inline]
        fn on_platform_feature_configured(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformFeatureConfigured,
        ) {
            (self.0).on_platform_feature_configured(meta, event);
            (self.1).on_platform_feature_configured(meta, event);
        }
        #[inline]
        fn on_platform_event_loop_wakeup(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopWakeup,
        ) {
            (self.0).on_platform_event_loop_wakeup(meta, event);
            (self.1).on_platform_event_loop_wakeup(meta, event);
        }
        #[inline]
        fn on_platform_event_loop_sleep(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopSleep,
        ) {
            (self.0).on_platform_event_loop_sleep(meta, event);
            (self.1).on_platform_event_loop_sleep(meta, event);
        }
        #[inline]
        fn on_platform_event_loop_started(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopStarted,
        ) {
            (self.0).on_platform_event_loop_started(meta, event);
            (self.1).on_platform_event_loop_started(meta, event);
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
            meta: &api::ConnectionMeta,
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
        #[doc = "Publishes a `EndpointConnectionAttemptFailed` event to the publisher's subscriber"]
        fn on_endpoint_connection_attempt_failed(
            &mut self,
            event: builder::EndpointConnectionAttemptFailed,
        );
        #[doc = "Publishes a `EndpointConnectionAttemptDeduplicated` event to the publisher's subscriber"]
        fn on_endpoint_connection_attempt_deduplicated(
            &mut self,
            event: builder::EndpointConnectionAttemptDeduplicated,
        );
        #[doc = "Publishes a `PlatformTx` event to the publisher's subscriber"]
        fn on_platform_tx(&mut self, event: builder::PlatformTx);
        #[doc = "Publishes a `PlatformTxError` event to the publisher's subscriber"]
        fn on_platform_tx_error(&mut self, event: builder::PlatformTxError);
        #[doc = "Publishes a `PlatformRx` event to the publisher's subscriber"]
        fn on_platform_rx(&mut self, event: builder::PlatformRx);
        #[doc = "Publishes a `PlatformRxError` event to the publisher's subscriber"]
        fn on_platform_rx_error(&mut self, event: builder::PlatformRxError);
        #[doc = "Publishes a `PlatformFeatureConfigured` event to the publisher's subscriber"]
        fn on_platform_feature_configured(&mut self, event: builder::PlatformFeatureConfigured);
        #[doc = "Publishes a `PlatformEventLoopWakeup` event to the publisher's subscriber"]
        fn on_platform_event_loop_wakeup(&mut self, event: builder::PlatformEventLoopWakeup);
        #[doc = "Publishes a `PlatformEventLoopSleep` event to the publisher's subscriber"]
        fn on_platform_event_loop_sleep(&mut self, event: builder::PlatformEventLoopSleep);
        #[doc = "Publishes a `PlatformEventLoopStarted` event to the publisher's subscriber"]
        fn on_platform_event_loop_started(&mut self, event: builder::PlatformEventLoopStarted);
        #[doc = r" Returns the QUIC version, if any"]
        fn quic_version(&self) -> Option<u32>;
    }
    pub struct EndpointPublisherSubscriber<'a, Sub: Subscriber> {
        meta: api::EndpointMeta,
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
        fn on_endpoint_connection_attempt_failed(
            &mut self,
            event: builder::EndpointConnectionAttemptFailed,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_connection_attempt_failed(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_connection_attempt_deduplicated(
            &mut self,
            event: builder::EndpointConnectionAttemptDeduplicated,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_connection_attempt_deduplicated(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_platform_tx(&mut self, event: builder::PlatformTx) {
            let event = event.into_event();
            self.subscriber.on_platform_tx(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_platform_tx_error(&mut self, event: builder::PlatformTxError) {
            let event = event.into_event();
            self.subscriber.on_platform_tx_error(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_platform_rx(&mut self, event: builder::PlatformRx) {
            let event = event.into_event();
            self.subscriber.on_platform_rx(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_platform_rx_error(&mut self, event: builder::PlatformRxError) {
            let event = event.into_event();
            self.subscriber.on_platform_rx_error(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_platform_feature_configured(&mut self, event: builder::PlatformFeatureConfigured) {
            let event = event.into_event();
            self.subscriber
                .on_platform_feature_configured(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_platform_event_loop_wakeup(&mut self, event: builder::PlatformEventLoopWakeup) {
            let event = event.into_event();
            self.subscriber
                .on_platform_event_loop_wakeup(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_platform_event_loop_sleep(&mut self, event: builder::PlatformEventLoopSleep) {
            let event = event.into_event();
            self.subscriber
                .on_platform_event_loop_sleep(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_platform_event_loop_started(&mut self, event: builder::PlatformEventLoopStarted) {
            let event = event.into_event();
            self.subscriber
                .on_platform_event_loop_started(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> Option<u32> {
            self.quic_version
        }
    }
    pub trait ConnectionPublisher {
        #[doc = "Publishes a `ApplicationProtocolInformation` event to the publisher's subscriber"]
        fn on_application_protocol_information(
            &mut self,
            event: builder::ApplicationProtocolInformation,
        );
        #[doc = "Publishes a `ServerNameInformation` event to the publisher's subscriber"]
        fn on_server_name_information(&mut self, event: builder::ServerNameInformation);
        #[doc = "Publishes a `KeyExchangeGroup` event to the publisher's subscriber"]
        fn on_key_exchange_group(&mut self, event: builder::KeyExchangeGroup);
        #[doc = "Publishes a `PacketSkipped` event to the publisher's subscriber"]
        fn on_packet_skipped(&mut self, event: builder::PacketSkipped);
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
        #[doc = "Publishes a `ConnectionCloseFrameReceived` event to the publisher's subscriber"]
        fn on_connection_close_frame_received(
            &mut self,
            event: builder::ConnectionCloseFrameReceived,
        );
        #[doc = "Publishes a `PacketLost` event to the publisher's subscriber"]
        fn on_packet_lost(&mut self, event: builder::PacketLost);
        #[doc = "Publishes a `RecoveryMetrics` event to the publisher's subscriber"]
        fn on_recovery_metrics(&mut self, event: builder::RecoveryMetrics);
        #[doc = "Publishes a `Congestion` event to the publisher's subscriber"]
        fn on_congestion(&mut self, event: builder::Congestion);
        #[doc = "Publishes a `AckProcessed` event to the publisher's subscriber"]
        fn on_ack_processed(&mut self, event: builder::AckProcessed);
        #[doc = "Publishes a `RxAckRangeDropped` event to the publisher's subscriber"]
        fn on_rx_ack_range_dropped(&mut self, event: builder::RxAckRangeDropped);
        #[doc = "Publishes a `AckRangeReceived` event to the publisher's subscriber"]
        fn on_ack_range_received(&mut self, event: builder::AckRangeReceived);
        #[doc = "Publishes a `AckRangeSent` event to the publisher's subscriber"]
        fn on_ack_range_sent(&mut self, event: builder::AckRangeSent);
        #[doc = "Publishes a `PacketDropped` event to the publisher's subscriber"]
        fn on_packet_dropped(&mut self, event: builder::PacketDropped);
        #[doc = "Publishes a `KeyUpdate` event to the publisher's subscriber"]
        fn on_key_update(&mut self, event: builder::KeyUpdate);
        #[doc = "Publishes a `KeySpaceDiscarded` event to the publisher's subscriber"]
        fn on_key_space_discarded(&mut self, event: builder::KeySpaceDiscarded);
        #[doc = "Publishes a `ConnectionStarted` event to the publisher's subscriber"]
        fn on_connection_started(&mut self, event: builder::ConnectionStarted);
        #[doc = "Publishes a `DuplicatePacket` event to the publisher's subscriber"]
        fn on_duplicate_packet(&mut self, event: builder::DuplicatePacket);
        #[doc = "Publishes a `TransportParametersReceived` event to the publisher's subscriber"]
        fn on_transport_parameters_received(&mut self, event: builder::TransportParametersReceived);
        #[doc = "Publishes a `DatagramSent` event to the publisher's subscriber"]
        fn on_datagram_sent(&mut self, event: builder::DatagramSent);
        #[doc = "Publishes a `DatagramReceived` event to the publisher's subscriber"]
        fn on_datagram_received(&mut self, event: builder::DatagramReceived);
        #[doc = "Publishes a `DatagramDropped` event to the publisher's subscriber"]
        fn on_datagram_dropped(&mut self, event: builder::DatagramDropped);
        #[doc = "Publishes a `HandshakeRemoteAddressChangeObserved` event to the publisher's subscriber"]
        fn on_handshake_remote_address_change_observed(
            &mut self,
            event: builder::HandshakeRemoteAddressChangeObserved,
        );
        #[doc = "Publishes a `ConnectionIdUpdated` event to the publisher's subscriber"]
        fn on_connection_id_updated(&mut self, event: builder::ConnectionIdUpdated);
        #[doc = "Publishes a `EcnStateChanged` event to the publisher's subscriber"]
        fn on_ecn_state_changed(&mut self, event: builder::EcnStateChanged);
        #[doc = "Publishes a `ConnectionMigrationDenied` event to the publisher's subscriber"]
        fn on_connection_migration_denied(&mut self, event: builder::ConnectionMigrationDenied);
        #[doc = "Publishes a `HandshakeStatusUpdated` event to the publisher's subscriber"]
        fn on_handshake_status_updated(&mut self, event: builder::HandshakeStatusUpdated);
        #[doc = "Publishes a `TlsExporterReady` event to the publisher's subscriber"]
        fn on_tls_exporter_ready(&mut self, event: builder::TlsExporterReady);
        #[doc = "Publishes a `TlsHandshakeFailed` event to the publisher's subscriber"]
        fn on_tls_handshake_failed(&mut self, event: builder::TlsHandshakeFailed);
        #[doc = "Publishes a `PathChallengeUpdated` event to the publisher's subscriber"]
        fn on_path_challenge_updated(&mut self, event: builder::PathChallengeUpdated);
        #[doc = "Publishes a `TlsClientHello` event to the publisher's subscriber"]
        fn on_tls_client_hello(&mut self, event: builder::TlsClientHello);
        #[doc = "Publishes a `TlsServerHello` event to the publisher's subscriber"]
        fn on_tls_server_hello(&mut self, event: builder::TlsServerHello);
        #[doc = "Publishes a `RxStreamProgress` event to the publisher's subscriber"]
        fn on_rx_stream_progress(&mut self, event: builder::RxStreamProgress);
        #[doc = "Publishes a `TxStreamProgress` event to the publisher's subscriber"]
        fn on_tx_stream_progress(&mut self, event: builder::TxStreamProgress);
        #[doc = "Publishes a `KeepAliveTimerExpired` event to the publisher's subscriber"]
        fn on_keep_alive_timer_expired(&mut self, event: builder::KeepAliveTimerExpired);
        #[doc = "Publishes a `MtuUpdated` event to the publisher's subscriber"]
        fn on_mtu_updated(&mut self, event: builder::MtuUpdated);
        #[doc = "Publishes a `SlowStartExited` event to the publisher's subscriber"]
        fn on_slow_start_exited(&mut self, event: builder::SlowStartExited);
        #[doc = "Publishes a `DeliveryRateSampled` event to the publisher's subscriber"]
        fn on_delivery_rate_sampled(&mut self, event: builder::DeliveryRateSampled);
        #[doc = "Publishes a `PacingRateUpdated` event to the publisher's subscriber"]
        fn on_pacing_rate_updated(&mut self, event: builder::PacingRateUpdated);
        #[doc = "Publishes a `BbrStateChanged` event to the publisher's subscriber"]
        fn on_bbr_state_changed(&mut self, event: builder::BbrStateChanged);
        #[doc = "Publishes a `DcStateChanged` event to the publisher's subscriber"]
        fn on_dc_state_changed(&mut self, event: builder::DcStateChanged);
        #[doc = "Publishes a `DcPathCreated` event to the publisher's subscriber"]
        fn on_dc_path_created(&mut self, event: builder::DcPathCreated);
        #[doc = "Publishes a `ConnectionClosed` event to the publisher's subscriber"]
        fn on_connection_closed(&mut self, event: builder::ConnectionClosed);
        #[doc = r" Returns the QUIC version negotiated for the current connection, if any"]
        fn quic_version(&self) -> u32;
        #[doc = r" Returns the [`Subject`] for the current publisher"]
        fn subject(&self) -> api::Subject;
    }
    pub struct ConnectionPublisherSubscriber<'a, Sub: Subscriber> {
        meta: api::ConnectionMeta,
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
        fn on_application_protocol_information(
            &mut self,
            event: builder::ApplicationProtocolInformation,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_application_protocol_information(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_server_name_information(&mut self, event: builder::ServerNameInformation) {
            let event = event.into_event();
            self.subscriber
                .on_server_name_information(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_key_exchange_group(&mut self, event: builder::KeyExchangeGroup) {
            let event = event.into_event();
            self.subscriber
                .on_key_exchange_group(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_packet_skipped(&mut self, event: builder::PacketSkipped) {
            let event = event.into_event();
            self.subscriber
                .on_packet_skipped(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_packet_sent(&mut self, event: builder::PacketSent) {
            let event = event.into_event();
            self.subscriber
                .on_packet_sent(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_packet_received(&mut self, event: builder::PacketReceived) {
            let event = event.into_event();
            self.subscriber
                .on_packet_received(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_active_path_updated(&mut self, event: builder::ActivePathUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_active_path_updated(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_created(&mut self, event: builder::PathCreated) {
            let event = event.into_event();
            self.subscriber
                .on_path_created(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_frame_sent(&mut self, event: builder::FrameSent) {
            let event = event.into_event();
            self.subscriber
                .on_frame_sent(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_frame_received(&mut self, event: builder::FrameReceived) {
            let event = event.into_event();
            self.subscriber
                .on_frame_received(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_connection_close_frame_received(
            &mut self,
            event: builder::ConnectionCloseFrameReceived,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_connection_close_frame_received(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_packet_lost(&mut self, event: builder::PacketLost) {
            let event = event.into_event();
            self.subscriber
                .on_packet_lost(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_recovery_metrics(&mut self, event: builder::RecoveryMetrics) {
            let event = event.into_event();
            self.subscriber
                .on_recovery_metrics(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_congestion(&mut self, event: builder::Congestion) {
            let event = event.into_event();
            self.subscriber
                .on_congestion(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        #[allow(deprecated)]
        fn on_ack_processed(&mut self, event: builder::AckProcessed) {
            let event = event.into_event();
            self.subscriber
                .on_ack_processed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_rx_ack_range_dropped(&mut self, event: builder::RxAckRangeDropped) {
            let event = event.into_event();
            self.subscriber
                .on_rx_ack_range_dropped(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_ack_range_received(&mut self, event: builder::AckRangeReceived) {
            let event = event.into_event();
            self.subscriber
                .on_ack_range_received(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_ack_range_sent(&mut self, event: builder::AckRangeSent) {
            let event = event.into_event();
            self.subscriber
                .on_ack_range_sent(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_packet_dropped(&mut self, event: builder::PacketDropped) {
            let event = event.into_event();
            self.subscriber
                .on_packet_dropped(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_key_update(&mut self, event: builder::KeyUpdate) {
            let event = event.into_event();
            self.subscriber
                .on_key_update(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_key_space_discarded(&mut self, event: builder::KeySpaceDiscarded) {
            let event = event.into_event();
            self.subscriber
                .on_key_space_discarded(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_connection_started(&mut self, event: builder::ConnectionStarted) {
            let event = event.into_event();
            self.subscriber
                .on_connection_started(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_duplicate_packet(&mut self, event: builder::DuplicatePacket) {
            let event = event.into_event();
            self.subscriber
                .on_duplicate_packet(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_transport_parameters_received(
            &mut self,
            event: builder::TransportParametersReceived,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_transport_parameters_received(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_datagram_sent(&mut self, event: builder::DatagramSent) {
            let event = event.into_event();
            self.subscriber
                .on_datagram_sent(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_datagram_received(&mut self, event: builder::DatagramReceived) {
            let event = event.into_event();
            self.subscriber
                .on_datagram_received(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_datagram_dropped(&mut self, event: builder::DatagramDropped) {
            let event = event.into_event();
            self.subscriber
                .on_datagram_dropped(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_handshake_remote_address_change_observed(
            &mut self,
            event: builder::HandshakeRemoteAddressChangeObserved,
        ) {
            let event = event.into_event();
            self.subscriber.on_handshake_remote_address_change_observed(
                self.context,
                &self.meta,
                &event,
            );
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_connection_id_updated(&mut self, event: builder::ConnectionIdUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_connection_id_updated(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_ecn_state_changed(&mut self, event: builder::EcnStateChanged) {
            let event = event.into_event();
            self.subscriber
                .on_ecn_state_changed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_connection_migration_denied(&mut self, event: builder::ConnectionMigrationDenied) {
            let event = event.into_event();
            self.subscriber
                .on_connection_migration_denied(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_handshake_status_updated(&mut self, event: builder::HandshakeStatusUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_handshake_status_updated(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_tls_exporter_ready(&mut self, event: builder::TlsExporterReady) {
            let event = event.into_event();
            self.subscriber
                .on_tls_exporter_ready(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_tls_handshake_failed(&mut self, event: builder::TlsHandshakeFailed) {
            let event = event.into_event();
            self.subscriber
                .on_tls_handshake_failed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_challenge_updated(&mut self, event: builder::PathChallengeUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_path_challenge_updated(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_tls_client_hello(&mut self, event: builder::TlsClientHello) {
            let event = event.into_event();
            self.subscriber
                .on_tls_client_hello(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_tls_server_hello(&mut self, event: builder::TlsServerHello) {
            let event = event.into_event();
            self.subscriber
                .on_tls_server_hello(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_rx_stream_progress(&mut self, event: builder::RxStreamProgress) {
            let event = event.into_event();
            self.subscriber
                .on_rx_stream_progress(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_tx_stream_progress(&mut self, event: builder::TxStreamProgress) {
            let event = event.into_event();
            self.subscriber
                .on_tx_stream_progress(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_keep_alive_timer_expired(&mut self, event: builder::KeepAliveTimerExpired) {
            let event = event.into_event();
            self.subscriber
                .on_keep_alive_timer_expired(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_mtu_updated(&mut self, event: builder::MtuUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_mtu_updated(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_slow_start_exited(&mut self, event: builder::SlowStartExited) {
            let event = event.into_event();
            self.subscriber
                .on_slow_start_exited(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_delivery_rate_sampled(&mut self, event: builder::DeliveryRateSampled) {
            let event = event.into_event();
            self.subscriber
                .on_delivery_rate_sampled(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_pacing_rate_updated(&mut self, event: builder::PacingRateUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_pacing_rate_updated(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_bbr_state_changed(&mut self, event: builder::BbrStateChanged) {
            let event = event.into_event();
            self.subscriber
                .on_bbr_state_changed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_dc_state_changed(&mut self, event: builder::DcStateChanged) {
            let event = event.into_event();
            self.subscriber
                .on_dc_state_changed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_dc_path_created(&mut self, event: builder::DcPathCreated) {
            let event = event.into_event();
            self.subscriber
                .on_dc_path_created(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_connection_closed(&mut self, event: builder::ConnectionClosed) {
            let event = event.into_event();
            self.subscriber
                .on_connection_closed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> u32 {
            self.quic_version
        }
        #[inline]
        fn subject(&self) -> api::Subject {
            self.meta.subject()
        }
    }
}
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::event::snapshot::Location;
    pub mod endpoint {
        use super::*;
        pub struct Subscriber {
            location: Option<Location>,
            output: Vec<String>,
            pub version_information: u64,
            pub endpoint_packet_sent: u64,
            pub endpoint_packet_received: u64,
            pub endpoint_datagram_sent: u64,
            pub endpoint_datagram_received: u64,
            pub endpoint_datagram_dropped: u64,
            pub endpoint_connection_attempt_failed: u64,
            pub endpoint_connection_attempt_deduplicated: u64,
            pub platform_tx: u64,
            pub platform_tx_error: u64,
            pub platform_rx: u64,
            pub platform_rx_error: u64,
            pub platform_feature_configured: u64,
            pub platform_event_loop_wakeup: u64,
            pub platform_event_loop_sleep: u64,
            pub platform_event_loop_started: u64,
        }
        impl Drop for Subscriber {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    return;
                }
                if let Some(location) = self.location.as_ref() {
                    location.snapshot_log(&self.output);
                }
            }
        }
        impl Subscriber {
            #[doc = r" Creates a subscriber with snapshot assertions enabled"]
            #[track_caller]
            pub fn snapshot() -> Self {
                let mut sub = Self::no_snapshot();
                sub.location = Location::from_thread_name();
                sub
            }
            #[doc = r" Creates a subscriber with snapshot assertions enabled"]
            #[track_caller]
            pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
                let mut sub = Self::no_snapshot();
                sub.location = Some(Location::new(name));
                sub
            }
            #[doc = r" Creates a subscriber with snapshot assertions disabled"]
            pub fn no_snapshot() -> Self {
                Self {
                    location: None,
                    output: Default::default(),
                    version_information: 0,
                    endpoint_packet_sent: 0,
                    endpoint_packet_received: 0,
                    endpoint_datagram_sent: 0,
                    endpoint_datagram_received: 0,
                    endpoint_datagram_dropped: 0,
                    endpoint_connection_attempt_failed: 0,
                    endpoint_connection_attempt_deduplicated: 0,
                    platform_tx: 0,
                    platform_tx_error: 0,
                    platform_rx: 0,
                    platform_rx_error: 0,
                    platform_feature_configured: 0,
                    platform_event_loop_wakeup: 0,
                    platform_event_loop_sleep: 0,
                    platform_event_loop_started: 0,
                }
            }
        }
        impl super::super::Subscriber for Subscriber {
            type ConnectionContext = ();
            fn create_connection_context(
                &mut self,
                _meta: &api::ConnectionMeta,
                _info: &api::ConnectionInfo,
            ) -> Self::ConnectionContext {
            }
            fn on_version_information(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::VersionInformation,
            ) {
                self.version_information += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_endpoint_packet_sent(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::EndpointPacketSent,
            ) {
                self.endpoint_packet_sent += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_endpoint_packet_received(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::EndpointPacketReceived,
            ) {
                self.endpoint_packet_received += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_endpoint_datagram_sent(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::EndpointDatagramSent,
            ) {
                self.endpoint_datagram_sent += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_endpoint_datagram_received(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::EndpointDatagramReceived,
            ) {
                self.endpoint_datagram_received += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_endpoint_datagram_dropped(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::EndpointDatagramDropped,
            ) {
                self.endpoint_datagram_dropped += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_endpoint_connection_attempt_failed(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::EndpointConnectionAttemptFailed,
            ) {
                self.endpoint_connection_attempt_failed += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_endpoint_connection_attempt_deduplicated(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::EndpointConnectionAttemptDeduplicated,
            ) {
                self.endpoint_connection_attempt_deduplicated += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_platform_tx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTx) {
                self.platform_tx += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_platform_tx_error(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::PlatformTxError,
            ) {
                self.platform_tx_error += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_platform_rx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRx) {
                self.platform_rx += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_platform_rx_error(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::PlatformRxError,
            ) {
                self.platform_rx_error += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_platform_feature_configured(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::PlatformFeatureConfigured,
            ) {
                self.platform_feature_configured += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_platform_event_loop_wakeup(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::PlatformEventLoopWakeup,
            ) {
                self.platform_event_loop_wakeup += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_platform_event_loop_sleep(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::PlatformEventLoopSleep,
            ) {
                self.platform_event_loop_sleep += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
            fn on_platform_event_loop_started(
                &mut self,
                meta: &api::EndpointMeta,
                event: &api::PlatformEventLoopStarted,
            ) {
                self.platform_event_loop_started += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
    }
    #[derive(Debug)]
    pub struct Subscriber {
        location: Option<Location>,
        output: Vec<String>,
        pub application_protocol_information: u64,
        pub server_name_information: u64,
        pub key_exchange_group: u64,
        pub packet_skipped: u64,
        pub packet_sent: u64,
        pub packet_received: u64,
        pub active_path_updated: u64,
        pub path_created: u64,
        pub frame_sent: u64,
        pub frame_received: u64,
        pub connection_close_frame_received: u64,
        pub packet_lost: u64,
        pub recovery_metrics: u64,
        pub congestion: u64,
        pub ack_processed: u64,
        pub rx_ack_range_dropped: u64,
        pub ack_range_received: u64,
        pub ack_range_sent: u64,
        pub packet_dropped: u64,
        pub key_update: u64,
        pub key_space_discarded: u64,
        pub connection_started: u64,
        pub duplicate_packet: u64,
        pub transport_parameters_received: u64,
        pub datagram_sent: u64,
        pub datagram_received: u64,
        pub datagram_dropped: u64,
        pub handshake_remote_address_change_observed: u64,
        pub connection_id_updated: u64,
        pub ecn_state_changed: u64,
        pub connection_migration_denied: u64,
        pub handshake_status_updated: u64,
        pub tls_exporter_ready: u64,
        pub tls_handshake_failed: u64,
        pub path_challenge_updated: u64,
        pub tls_client_hello: u64,
        pub tls_server_hello: u64,
        pub rx_stream_progress: u64,
        pub tx_stream_progress: u64,
        pub keep_alive_timer_expired: u64,
        pub mtu_updated: u64,
        pub slow_start_exited: u64,
        pub delivery_rate_sampled: u64,
        pub pacing_rate_updated: u64,
        pub bbr_state_changed: u64,
        pub dc_state_changed: u64,
        pub dc_path_created: u64,
        pub connection_closed: u64,
        pub version_information: u64,
        pub endpoint_packet_sent: u64,
        pub endpoint_packet_received: u64,
        pub endpoint_datagram_sent: u64,
        pub endpoint_datagram_received: u64,
        pub endpoint_datagram_dropped: u64,
        pub endpoint_connection_attempt_failed: u64,
        pub endpoint_connection_attempt_deduplicated: u64,
        pub platform_tx: u64,
        pub platform_tx_error: u64,
        pub platform_rx: u64,
        pub platform_rx_error: u64,
        pub platform_feature_configured: u64,
        pub platform_event_loop_wakeup: u64,
        pub platform_event_loop_sleep: u64,
        pub platform_event_loop_started: u64,
    }
    impl Drop for Subscriber {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            if let Some(location) = self.location.as_ref() {
                location.snapshot_log(&self.output);
            }
        }
    }
    impl Subscriber {
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn snapshot() -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Location::from_thread_name();
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Some(Location::new(name));
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions disabled"]
        pub fn no_snapshot() -> Self {
            Self {
                location: None,
                output: Default::default(),
                application_protocol_information: 0,
                server_name_information: 0,
                key_exchange_group: 0,
                packet_skipped: 0,
                packet_sent: 0,
                packet_received: 0,
                active_path_updated: 0,
                path_created: 0,
                frame_sent: 0,
                frame_received: 0,
                connection_close_frame_received: 0,
                packet_lost: 0,
                recovery_metrics: 0,
                congestion: 0,
                ack_processed: 0,
                rx_ack_range_dropped: 0,
                ack_range_received: 0,
                ack_range_sent: 0,
                packet_dropped: 0,
                key_update: 0,
                key_space_discarded: 0,
                connection_started: 0,
                duplicate_packet: 0,
                transport_parameters_received: 0,
                datagram_sent: 0,
                datagram_received: 0,
                datagram_dropped: 0,
                handshake_remote_address_change_observed: 0,
                connection_id_updated: 0,
                ecn_state_changed: 0,
                connection_migration_denied: 0,
                handshake_status_updated: 0,
                tls_exporter_ready: 0,
                tls_handshake_failed: 0,
                path_challenge_updated: 0,
                tls_client_hello: 0,
                tls_server_hello: 0,
                rx_stream_progress: 0,
                tx_stream_progress: 0,
                keep_alive_timer_expired: 0,
                mtu_updated: 0,
                slow_start_exited: 0,
                delivery_rate_sampled: 0,
                pacing_rate_updated: 0,
                bbr_state_changed: 0,
                dc_state_changed: 0,
                dc_path_created: 0,
                connection_closed: 0,
                version_information: 0,
                endpoint_packet_sent: 0,
                endpoint_packet_received: 0,
                endpoint_datagram_sent: 0,
                endpoint_datagram_received: 0,
                endpoint_datagram_dropped: 0,
                endpoint_connection_attempt_failed: 0,
                endpoint_connection_attempt_deduplicated: 0,
                platform_tx: 0,
                platform_tx_error: 0,
                platform_rx: 0,
                platform_rx_error: 0,
                platform_feature_configured: 0,
                platform_event_loop_wakeup: 0,
                platform_event_loop_sleep: 0,
                platform_event_loop_started: 0,
            }
        }
    }
    impl super::Subscriber for Subscriber {
        type ConnectionContext = ();
        fn create_connection_context(
            &mut self,
            _meta: &api::ConnectionMeta,
            _info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
        }
        fn on_application_protocol_information(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationProtocolInformation,
        ) {
            self.application_protocol_information += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_server_name_information(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ServerNameInformation,
        ) {
            self.server_name_information += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_key_exchange_group(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeyExchangeGroup,
        ) {
            self.key_exchange_group += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_skipped(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketSkipped,
        ) {
            self.packet_skipped += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_sent(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketSent,
        ) {
            self.packet_sent += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketReceived,
        ) {
            self.packet_received += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_active_path_updated(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ActivePathUpdated,
        ) {
            self.active_path_updated += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_path_created(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PathCreated,
        ) {
            self.path_created += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_frame_sent(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::FrameSent,
        ) {
            self.frame_sent += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_frame_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::FrameReceived,
        ) {
            self.frame_received += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_close_frame_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionCloseFrameReceived,
        ) {
            self.connection_close_frame_received += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_lost(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketLost,
        ) {
            self.packet_lost += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_recovery_metrics(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::RecoveryMetrics,
        ) {
            self.recovery_metrics += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_congestion(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::Congestion,
        ) {
            self.congestion += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        #[allow(deprecated)]
        fn on_ack_processed(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::AckProcessed,
        ) {
            self.ack_processed += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_rx_ack_range_dropped(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::RxAckRangeDropped,
        ) {
            self.rx_ack_range_dropped += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_ack_range_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::AckRangeReceived,
        ) {
            self.ack_range_received += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_ack_range_sent(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::AckRangeSent,
        ) {
            self.ack_range_sent += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_dropped(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacketDropped,
        ) {
            self.packet_dropped += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_key_update(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeyUpdate,
        ) {
            self.key_update += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_key_space_discarded(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeySpaceDiscarded,
        ) {
            self.key_space_discarded += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_started(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionStarted,
        ) {
            self.connection_started += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_duplicate_packet(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DuplicatePacket,
        ) {
            self.duplicate_packet += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_transport_parameters_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TransportParametersReceived,
        ) {
            self.transport_parameters_received += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_datagram_sent(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DatagramSent,
        ) {
            self.datagram_sent += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_datagram_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DatagramReceived,
        ) {
            self.datagram_received += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_datagram_dropped(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DatagramDropped,
        ) {
            self.datagram_dropped += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_handshake_remote_address_change_observed(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::HandshakeRemoteAddressChangeObserved,
        ) {
            self.handshake_remote_address_change_observed += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_id_updated(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionIdUpdated,
        ) {
            self.connection_id_updated += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_ecn_state_changed(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::EcnStateChanged,
        ) {
            self.ecn_state_changed += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_migration_denied(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionMigrationDenied,
        ) {
            self.connection_migration_denied += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_handshake_status_updated(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::HandshakeStatusUpdated,
        ) {
            self.handshake_status_updated += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_tls_exporter_ready(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsExporterReady,
        ) {
            self.tls_exporter_ready += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_tls_handshake_failed(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsHandshakeFailed,
        ) {
            self.tls_handshake_failed += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_path_challenge_updated(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PathChallengeUpdated,
        ) {
            self.path_challenge_updated += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_tls_client_hello(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsClientHello,
        ) {
            self.tls_client_hello += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_tls_server_hello(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TlsServerHello,
        ) {
            self.tls_server_hello += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_rx_stream_progress(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::RxStreamProgress,
        ) {
            self.rx_stream_progress += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_tx_stream_progress(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::TxStreamProgress,
        ) {
            self.tx_stream_progress += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_keep_alive_timer_expired(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::KeepAliveTimerExpired,
        ) {
            self.keep_alive_timer_expired += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_mtu_updated(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::MtuUpdated,
        ) {
            self.mtu_updated += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_slow_start_exited(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::SlowStartExited,
        ) {
            self.slow_start_exited += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_delivery_rate_sampled(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DeliveryRateSampled,
        ) {
            self.delivery_rate_sampled += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_pacing_rate_updated(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::PacingRateUpdated,
        ) {
            self.pacing_rate_updated += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_bbr_state_changed(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::BbrStateChanged,
        ) {
            self.bbr_state_changed += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_dc_state_changed(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DcStateChanged,
        ) {
            self.dc_state_changed += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_dc_path_created(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::DcPathCreated,
        ) {
            self.dc_path_created += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_closed(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionClosed,
        ) {
            self.connection_closed += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_version_information(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::VersionInformation,
        ) {
            self.version_information += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_endpoint_packet_sent(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointPacketSent,
        ) {
            self.endpoint_packet_sent += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_endpoint_packet_received(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointPacketReceived,
        ) {
            self.endpoint_packet_received += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_endpoint_datagram_sent(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramSent,
        ) {
            self.endpoint_datagram_sent += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_endpoint_datagram_received(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramReceived,
        ) {
            self.endpoint_datagram_received += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_endpoint_datagram_dropped(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointDatagramDropped,
        ) {
            self.endpoint_datagram_dropped += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_endpoint_connection_attempt_failed(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointConnectionAttemptFailed,
        ) {
            self.endpoint_connection_attempt_failed += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_endpoint_connection_attempt_deduplicated(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::EndpointConnectionAttemptDeduplicated,
        ) {
            self.endpoint_connection_attempt_deduplicated += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_platform_tx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTx) {
            self.platform_tx += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_platform_tx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTxError) {
            self.platform_tx_error += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_platform_rx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRx) {
            self.platform_rx += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_platform_rx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRxError) {
            self.platform_rx_error += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_platform_feature_configured(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformFeatureConfigured,
        ) {
            self.platform_feature_configured += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_platform_event_loop_wakeup(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopWakeup,
        ) {
            self.platform_event_loop_wakeup += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_platform_event_loop_sleep(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopSleep,
        ) {
            self.platform_event_loop_sleep += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
        fn on_platform_event_loop_started(
            &mut self,
            meta: &api::EndpointMeta,
            event: &api::PlatformEventLoopStarted,
        ) {
            self.platform_event_loop_started += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
    }
    #[derive(Debug)]
    pub struct Publisher {
        location: Option<Location>,
        output: Vec<String>,
        pub application_protocol_information: u64,
        pub server_name_information: u64,
        pub key_exchange_group: u64,
        pub packet_skipped: u64,
        pub packet_sent: u64,
        pub packet_received: u64,
        pub active_path_updated: u64,
        pub path_created: u64,
        pub frame_sent: u64,
        pub frame_received: u64,
        pub connection_close_frame_received: u64,
        pub packet_lost: u64,
        pub recovery_metrics: u64,
        pub congestion: u64,
        pub ack_processed: u64,
        pub rx_ack_range_dropped: u64,
        pub ack_range_received: u64,
        pub ack_range_sent: u64,
        pub packet_dropped: u64,
        pub key_update: u64,
        pub key_space_discarded: u64,
        pub connection_started: u64,
        pub duplicate_packet: u64,
        pub transport_parameters_received: u64,
        pub datagram_sent: u64,
        pub datagram_received: u64,
        pub datagram_dropped: u64,
        pub handshake_remote_address_change_observed: u64,
        pub connection_id_updated: u64,
        pub ecn_state_changed: u64,
        pub connection_migration_denied: u64,
        pub handshake_status_updated: u64,
        pub tls_exporter_ready: u64,
        pub tls_handshake_failed: u64,
        pub path_challenge_updated: u64,
        pub tls_client_hello: u64,
        pub tls_server_hello: u64,
        pub rx_stream_progress: u64,
        pub tx_stream_progress: u64,
        pub keep_alive_timer_expired: u64,
        pub mtu_updated: u64,
        pub slow_start_exited: u64,
        pub delivery_rate_sampled: u64,
        pub pacing_rate_updated: u64,
        pub bbr_state_changed: u64,
        pub dc_state_changed: u64,
        pub dc_path_created: u64,
        pub connection_closed: u64,
        pub version_information: u64,
        pub endpoint_packet_sent: u64,
        pub endpoint_packet_received: u64,
        pub endpoint_datagram_sent: u64,
        pub endpoint_datagram_received: u64,
        pub endpoint_datagram_dropped: u64,
        pub endpoint_connection_attempt_failed: u64,
        pub endpoint_connection_attempt_deduplicated: u64,
        pub platform_tx: u64,
        pub platform_tx_error: u64,
        pub platform_rx: u64,
        pub platform_rx_error: u64,
        pub platform_feature_configured: u64,
        pub platform_event_loop_wakeup: u64,
        pub platform_event_loop_sleep: u64,
        pub platform_event_loop_started: u64,
    }
    impl Publisher {
        #[doc = r" Creates a publisher with snapshot assertions enabled"]
        #[track_caller]
        pub fn snapshot() -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Location::from_thread_name();
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Some(Location::new(name));
            sub
        }
        #[doc = r" Creates a publisher with snapshot assertions disabled"]
        pub fn no_snapshot() -> Self {
            Self {
                location: None,
                output: Default::default(),
                application_protocol_information: 0,
                server_name_information: 0,
                key_exchange_group: 0,
                packet_skipped: 0,
                packet_sent: 0,
                packet_received: 0,
                active_path_updated: 0,
                path_created: 0,
                frame_sent: 0,
                frame_received: 0,
                connection_close_frame_received: 0,
                packet_lost: 0,
                recovery_metrics: 0,
                congestion: 0,
                ack_processed: 0,
                rx_ack_range_dropped: 0,
                ack_range_received: 0,
                ack_range_sent: 0,
                packet_dropped: 0,
                key_update: 0,
                key_space_discarded: 0,
                connection_started: 0,
                duplicate_packet: 0,
                transport_parameters_received: 0,
                datagram_sent: 0,
                datagram_received: 0,
                datagram_dropped: 0,
                handshake_remote_address_change_observed: 0,
                connection_id_updated: 0,
                ecn_state_changed: 0,
                connection_migration_denied: 0,
                handshake_status_updated: 0,
                tls_exporter_ready: 0,
                tls_handshake_failed: 0,
                path_challenge_updated: 0,
                tls_client_hello: 0,
                tls_server_hello: 0,
                rx_stream_progress: 0,
                tx_stream_progress: 0,
                keep_alive_timer_expired: 0,
                mtu_updated: 0,
                slow_start_exited: 0,
                delivery_rate_sampled: 0,
                pacing_rate_updated: 0,
                bbr_state_changed: 0,
                dc_state_changed: 0,
                dc_path_created: 0,
                connection_closed: 0,
                version_information: 0,
                endpoint_packet_sent: 0,
                endpoint_packet_received: 0,
                endpoint_datagram_sent: 0,
                endpoint_datagram_received: 0,
                endpoint_datagram_dropped: 0,
                endpoint_connection_attempt_failed: 0,
                endpoint_connection_attempt_deduplicated: 0,
                platform_tx: 0,
                platform_tx_error: 0,
                platform_rx: 0,
                platform_rx_error: 0,
                platform_feature_configured: 0,
                platform_event_loop_wakeup: 0,
                platform_event_loop_sleep: 0,
                platform_event_loop_started: 0,
            }
        }
    }
    impl super::EndpointPublisher for Publisher {
        fn on_version_information(&mut self, event: builder::VersionInformation) {
            self.version_information += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_endpoint_packet_sent(&mut self, event: builder::EndpointPacketSent) {
            self.endpoint_packet_sent += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_endpoint_packet_received(&mut self, event: builder::EndpointPacketReceived) {
            self.endpoint_packet_received += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_endpoint_datagram_sent(&mut self, event: builder::EndpointDatagramSent) {
            self.endpoint_datagram_sent += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_endpoint_datagram_received(&mut self, event: builder::EndpointDatagramReceived) {
            self.endpoint_datagram_received += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_endpoint_datagram_dropped(&mut self, event: builder::EndpointDatagramDropped) {
            self.endpoint_datagram_dropped += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_endpoint_connection_attempt_failed(
            &mut self,
            event: builder::EndpointConnectionAttemptFailed,
        ) {
            self.endpoint_connection_attempt_failed += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_endpoint_connection_attempt_deduplicated(
            &mut self,
            event: builder::EndpointConnectionAttemptDeduplicated,
        ) {
            self.endpoint_connection_attempt_deduplicated += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_platform_tx(&mut self, event: builder::PlatformTx) {
            self.platform_tx += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_platform_tx_error(&mut self, event: builder::PlatformTxError) {
            self.platform_tx_error += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_platform_rx(&mut self, event: builder::PlatformRx) {
            self.platform_rx += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_platform_rx_error(&mut self, event: builder::PlatformRxError) {
            self.platform_rx_error += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_platform_feature_configured(&mut self, event: builder::PlatformFeatureConfigured) {
            self.platform_feature_configured += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_platform_event_loop_wakeup(&mut self, event: builder::PlatformEventLoopWakeup) {
            self.platform_event_loop_wakeup += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_platform_event_loop_sleep(&mut self, event: builder::PlatformEventLoopSleep) {
            self.platform_event_loop_sleep += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn on_platform_event_loop_started(&mut self, event: builder::PlatformEventLoopStarted) {
            self.platform_event_loop_started += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn quic_version(&self) -> Option<u32> {
            Some(1)
        }
    }
    impl super::ConnectionPublisher for Publisher {
        fn on_application_protocol_information(
            &mut self,
            event: builder::ApplicationProtocolInformation,
        ) {
            self.application_protocol_information += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_server_name_information(&mut self, event: builder::ServerNameInformation) {
            self.server_name_information += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_key_exchange_group(&mut self, event: builder::KeyExchangeGroup) {
            self.key_exchange_group += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_skipped(&mut self, event: builder::PacketSkipped) {
            self.packet_skipped += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_sent(&mut self, event: builder::PacketSent) {
            self.packet_sent += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_received(&mut self, event: builder::PacketReceived) {
            self.packet_received += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_active_path_updated(&mut self, event: builder::ActivePathUpdated) {
            self.active_path_updated += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_path_created(&mut self, event: builder::PathCreated) {
            self.path_created += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_frame_sent(&mut self, event: builder::FrameSent) {
            self.frame_sent += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_frame_received(&mut self, event: builder::FrameReceived) {
            self.frame_received += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_close_frame_received(
            &mut self,
            event: builder::ConnectionCloseFrameReceived,
        ) {
            self.connection_close_frame_received += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_lost(&mut self, event: builder::PacketLost) {
            self.packet_lost += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_recovery_metrics(&mut self, event: builder::RecoveryMetrics) {
            self.recovery_metrics += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_congestion(&mut self, event: builder::Congestion) {
            self.congestion += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        #[allow(deprecated)]
        fn on_ack_processed(&mut self, event: builder::AckProcessed) {
            self.ack_processed += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_rx_ack_range_dropped(&mut self, event: builder::RxAckRangeDropped) {
            self.rx_ack_range_dropped += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_ack_range_received(&mut self, event: builder::AckRangeReceived) {
            self.ack_range_received += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_ack_range_sent(&mut self, event: builder::AckRangeSent) {
            self.ack_range_sent += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_packet_dropped(&mut self, event: builder::PacketDropped) {
            self.packet_dropped += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_key_update(&mut self, event: builder::KeyUpdate) {
            self.key_update += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_key_space_discarded(&mut self, event: builder::KeySpaceDiscarded) {
            self.key_space_discarded += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_started(&mut self, event: builder::ConnectionStarted) {
            self.connection_started += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_duplicate_packet(&mut self, event: builder::DuplicatePacket) {
            self.duplicate_packet += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_transport_parameters_received(
            &mut self,
            event: builder::TransportParametersReceived,
        ) {
            self.transport_parameters_received += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_datagram_sent(&mut self, event: builder::DatagramSent) {
            self.datagram_sent += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_datagram_received(&mut self, event: builder::DatagramReceived) {
            self.datagram_received += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_datagram_dropped(&mut self, event: builder::DatagramDropped) {
            self.datagram_dropped += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_handshake_remote_address_change_observed(
            &mut self,
            event: builder::HandshakeRemoteAddressChangeObserved,
        ) {
            self.handshake_remote_address_change_observed += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_id_updated(&mut self, event: builder::ConnectionIdUpdated) {
            self.connection_id_updated += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_ecn_state_changed(&mut self, event: builder::EcnStateChanged) {
            self.ecn_state_changed += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_migration_denied(&mut self, event: builder::ConnectionMigrationDenied) {
            self.connection_migration_denied += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_handshake_status_updated(&mut self, event: builder::HandshakeStatusUpdated) {
            self.handshake_status_updated += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_tls_exporter_ready(&mut self, event: builder::TlsExporterReady) {
            self.tls_exporter_ready += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_tls_handshake_failed(&mut self, event: builder::TlsHandshakeFailed) {
            self.tls_handshake_failed += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_path_challenge_updated(&mut self, event: builder::PathChallengeUpdated) {
            self.path_challenge_updated += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_tls_client_hello(&mut self, event: builder::TlsClientHello) {
            self.tls_client_hello += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_tls_server_hello(&mut self, event: builder::TlsServerHello) {
            self.tls_server_hello += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_rx_stream_progress(&mut self, event: builder::RxStreamProgress) {
            self.rx_stream_progress += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_tx_stream_progress(&mut self, event: builder::TxStreamProgress) {
            self.tx_stream_progress += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_keep_alive_timer_expired(&mut self, event: builder::KeepAliveTimerExpired) {
            self.keep_alive_timer_expired += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_mtu_updated(&mut self, event: builder::MtuUpdated) {
            self.mtu_updated += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_slow_start_exited(&mut self, event: builder::SlowStartExited) {
            self.slow_start_exited += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_delivery_rate_sampled(&mut self, event: builder::DeliveryRateSampled) {
            self.delivery_rate_sampled += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_pacing_rate_updated(&mut self, event: builder::PacingRateUpdated) {
            self.pacing_rate_updated += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_bbr_state_changed(&mut self, event: builder::BbrStateChanged) {
            self.bbr_state_changed += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_dc_state_changed(&mut self, event: builder::DcStateChanged) {
            self.dc_state_changed += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_dc_path_created(&mut self, event: builder::DcPathCreated) {
            self.dc_path_created += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_connection_closed(&mut self, event: builder::ConnectionClosed) {
            self.connection_closed += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn quic_version(&self) -> u32 {
            1
        }
        fn subject(&self) -> api::Subject {
            builder::Subject::Connection { id: 0 }.into_event()
        }
    }
    impl Drop for Publisher {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            if let Some(location) = self.location.as_ref() {
                location.snapshot_log(&self.output);
            }
        }
    }
}
