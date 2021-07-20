// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::PeerId, endpoint, inet::SocketAddress, packet::number::PacketNumberSpace};
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

common!(
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#4
    //# When the qlog "group_id" field is used, it is recommended to use
    //# QUIC's Original Destination Connection ID (ODCID, the CID chosen by
    //# the client when first contacting the server)
    struct Meta {
        pub endpoint_type: endpoint::Type,
        pub group_id: u64,
    }

    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.4
    struct PacketHeader {
        pub packet_type: common::PacketType,
        pub packet_number: u64,
        pub version: Option<u32>,
    }

    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.7
    enum Frame {
        Padding,
        Ping,
        Ack,
        ResetStream,
        StopSending,
        Crypto,
        NewToken,
        Stream,
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
        pub chosen_alpn: u32,
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
        pub src_addr: &'a SocketAddress,
        pub dst_addr: &'a SocketAddress,
        pub src_cid: &'a PeerId,
        pub dst_cid: &'a PeerId,
    }

    #[name = "transport:frame_received"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.5
    // This diverges a bit from the qlog spec, which prefers to log data as part of the
    // packet events.
    /// Frame was received
    struct FrameReceived {
        pub packet_header: common::PacketHeader,
        pub frame: common::Frame,
    }

    #[name = "recovery:packet_lost"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.4.5
    /// Packet was lost
    struct PacketLost<'a> {
        pub packet_header: common::PacketHeader,
        pub src_addr: &'a SocketAddress,
        pub src_cid: &'a PeerId,
        pub bytes_lost: u16,
        pub is_mtu_probe: bool,
    }

    #[name = "recovery:metrics_updated"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.1.8
    /// Recovery metrics updated
    struct RecoveryMetrics {
        pub min_rtt: Duration,
        pub smoothed_rtt: Duration,
        pub latest_rtt: Duration,
        pub rtt_variance: Duration,
        pub max_ack_delay: Duration,
        pub pto_backoff: u32,
        pub congestion_window: u32,
        pub bytes_in_flight: u32,
    }
);
