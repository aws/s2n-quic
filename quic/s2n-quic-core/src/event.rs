// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint;
use paste::paste;

#[macro_use]
mod macros;

/// All event types which can be emitted from this library.
pub trait Event {
    const NAME: &'static str;
}

pub mod builders {
    pub use super::{common_builders::*, event_builders::*};
}

common!(
    //
    struct Meta {
        pub endpoint_type: endpoint::Type,
        pub group_id: u64,
    }

    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.4
    //# Note: short vs long header is implicit through PacketType
    struct PacketHeader {
        pub packet_type: common::PacketType,
        pub packet_number: u64,
        pub version: Option<u32>,
    }

    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#A.2
    //# PacketType
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

events!(
    #[name = "transport::version_information"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.1
    //# QUIC endpoints each have their own list of of QUIC versions they
    //# support.
    /// QUIC version
    struct VersionInformation<'a> {
        pub server_versions: &'a [u32],
        pub client_versions: &'a [u32],
        pub chosen_version: u32,
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
        pub is_coalesced: bool,
    }

    #[name = "transport:packet_received"]
    //= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02.txt#5.3.6
    /// Packet was received
    struct PacketReceived {
        pub packet_header: common::PacketHeader,
        pub is_coalesced: bool,
    }
);
