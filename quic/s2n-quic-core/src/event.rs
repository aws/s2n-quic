// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection, endpoint, packet::number::PacketNumberSpace};
use core::time::Duration;
use paste::paste;

#[macro_use]
mod macros;

/// All event types which can be emitted from this library.
pub trait Event: core::fmt::Debug {
    const NAME: &'static str;
}

pub mod builders {
    pub use super::{common_builders::*, event_builders::*};
}

#[derive(Clone, Debug)]
pub struct Timestamp(crate::time::Timestamp);

impl Timestamp {
    pub(super) fn new(timestamp: crate::time::Timestamp) -> Self {
        Timestamp(timestamp)
    }

    /// The duration since the start of the s2n-quic process.
    ///
    /// Record the start `SystemTime` at the start of the program
    /// to derive the absolute time at which an event is emitted.
    ///
    /// ```rust
    /// use s2n_quic_core::{
    ///     endpoint,
    ///     event,
    ///     time::{Duration, Timestamp},
    /// };
    ///
    /// let start_time = std::time::SystemTime::now();
    /// // Meta is included as part of each event
    /// let meta: event::common::Meta = event::builders::Meta {
    ///     endpoint_type: endpoint::Type::Server,
    ///     group_id: 0,
    ///     timestamp: unsafe { Timestamp::from_duration(Duration::from_secs(1) )},
    /// }.into();
    /// let event_time = start_time + meta.timestamp.duration_since_start();
    /// ```
    pub fn duration_since_start(&self) -> Duration {
        // Safety: the duration is relative to start of program. This function along
        // with it's documentation captures this intent.
        unsafe { self.0.as_duration() }
    }
}

common!(
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.4
    struct PacketHeader {
        pub packet_type: common::PacketType,
        pub packet_number: u64,
        pub version: Option<u32>,
    }

    struct Path<'a> {
        // TODO uncomment once we record the local Address/CID
        // pub local_addr: common::SocketAddress<'a>,
        // pub local_cid: common::ConnectionId<'a>,
        pub remote_addr: common::SocketAddress<'a>,
        pub remote_cid: common::ConnectionId<'a>,
        pub id: u64,
    }

    struct ConnectionId<'a> {
        pub bytes: &'a [u8],
    }

    enum SocketAddress<'a> {
        IpV4 { ip: &'a [u8; 4], port: u16 },
        IpV6 { ip: &'a [u8; 16], port: u16 },
    }

    enum DuplicatePacketError {
        #[non_exhaustive]
        /// The packet number was already received and is a duplicate.
        Duplicate,

        #[non_exhaustive]
        /// The received packet number was outside the range of tracked packet numbers.
        ///
        /// This can happen when packets are heavily delayed or reordered. Currently, the maximum
        /// amount of reordering is limited to 128 packets. For example, if packet number `142`
        /// is received, the allowed range would be limited to `14-142`. If an endpoint received
        /// packet `< 14`, it would trigger this event.
        TooOld,
    }

    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.7
    enum Frame {
        #[non_exhaustive]
        Padding,
        #[non_exhaustive]
        Ping,
        #[non_exhaustive]
        Ack,
        #[non_exhaustive]
        ResetStream,
        #[non_exhaustive]
        StopSending,
        #[non_exhaustive]
        Crypto { offset: u64, len: u16 },
        #[non_exhaustive]
        NewToken,
        #[non_exhaustive]
        Stream {
            id: u64,
            offset: u64,
            len: u16,
            is_fin: bool,
        },
        #[non_exhaustive]
        MaxData,
        #[non_exhaustive]
        MaxStreamData,
        #[non_exhaustive]
        MaxStreams,
        #[non_exhaustive]
        DataBlocked,
        #[non_exhaustive]
        StreamDataBlocked,
        #[non_exhaustive]
        StreamsBlocked,
        #[non_exhaustive]
        NewConnectionId,
        #[non_exhaustive]
        RetireConnectionId,
        #[non_exhaustive]
        PathChallenge,
        #[non_exhaustive]
        PathResponse,
        #[non_exhaustive]
        ConnectionClose,
        #[non_exhaustive]
        HandshakeDone,
        #[non_exhaustive]
        Unknown,
    }

    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.2
    enum PacketType {
        Initial,
        Handshake,
        ZeroRtt,
        OneRtt,
        Retry,
        VersionNegotiation,
        StatelessReset,
        Unknown,
    }

    enum KeyType {
        Initial,
        Handshake,
        ZeroRtt,
        OneRtt { generation: u16 },
    }
);

impl Default for common::PacketType {
    fn default() -> Self {
        common::PacketType::Unknown
    }
}

impl From<PacketNumberSpace> for common::PacketType {
    fn from(packet_space: PacketNumberSpace) -> common::PacketType {
        match packet_space {
            PacketNumberSpace::Initial => common::PacketType::Initial,
            PacketNumberSpace::Handshake => common::PacketType::Handshake,
            PacketNumberSpace::ApplicationData => common::PacketType::OneRtt, // TODO: need to figure out how to capture ZeroRtt
        }
    }
}

events!(
    #[name = "transport::version_information"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.1
    //# QUIC endpoints each have their own list of of QUIC versions they
    //# support.
    /// QUIC version
    struct VersionInformation<'a> {
        pub server_versions: &'a [u32],
        pub client_versions: &'a [u32],
        pub chosen_version: Option<u32>,
    }

    #[name = "transport:alpn_information"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.2
    //# QUIC implementations each have their own list of application level
    //# protocols and versions thereof they support.
    /// Application level protocol
    struct AlpnInformation<'a> {
        pub server_alpns: &'a [&'a [u8]],
        pub client_alpns: &'a [&'a [u8]],
        pub chosen_alpn: &'a [u8],
    }

    #[name = "transport:packet_sent"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.5
    /// Packet was sent
    struct PacketSent {
        pub packet_header: common::PacketHeader,
    }

    #[name = "transport:packet_received"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.6
    /// Packet was received
    struct PacketReceived {
        pub packet_header: common::PacketHeader,
    }

    #[name = "connectivity:active_path_updated"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.1.8
    /// Active path was updated
    struct ActivePathUpdated<'a> {
        // TODO: many events seem to require PacketHeader. Make it more ergonomic
        // to include this field.
        // pub packet_header: common::PacketHeader,
        pub previous: common::Path<'a>,
        pub active: common::Path<'a>,
    }

    #[name = "transport:frame_sent"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.5
    // This diverges a bit from the qlog spec, which prefers to log data as part of the
    // packet events.
    /// Frame was sent
    struct FrameSent {
        pub packet_header: common::PacketHeader,
        pub path_id: u64,
        pub frame: common::Frame,
    }

    #[name = "transport:frame_received"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.6
    // This diverges a bit from the qlog spec, which prefers to log data as part of the
    // packet events.
    /// Frame was received
    struct FrameReceived {
        pub packet_header: common::PacketHeader,
        pub path_id: u64,
        pub frame: common::Frame,
    }

    #[name = "recovery:packet_lost"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.4.5
    /// Packet was lost
    struct PacketLost<'a> {
        pub packet_header: common::PacketHeader,
        pub path: common::Path<'a>,
        pub bytes_lost: u16,
        pub is_mtu_probe: bool,
    }

    #[name = "recovery:metrics_updated"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.4.2
    /// Recovery metrics updated
    struct RecoveryMetrics {
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

    #[name = "security:key_update"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.2.1
    /// Crypto key updated
    struct KeyUpdate {
        pub key_type: common::KeyType,
    }

    #[name = "connectivity:connection_started"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.1.2
    /// Connection started
    struct ConnectionStarted<'a> {
        pub path: common::Path<'a>,
    }

    #[name = "connectivity:connection_closed"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.1.3
    /// Connection closed
    struct ConnectionClosed {
        pub error: connection::Error,
    }

    #[name = "transport:duplicate_packet"]
    /// Duplicate packet received
    struct DuplicatePacket {
        pub packet_header: common::PacketHeader,
        pub path_id: u64,
        pub error: common::DuplicatePacketError,
    }

    #[name = "transport:datagram_received"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.11
    /// Datagram received
    struct DatagramReceived {
        pub len: u16,
    }

    #[name = "transport:datagram_dropped"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.12
    /// Datagram dropped
    struct DatagramDropped {
        pub len: u16,
    }
);
