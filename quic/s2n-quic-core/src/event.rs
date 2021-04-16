// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint;
use paste::paste;

#[macro_use]
mod macros;

/// All events types which can be emitted from this library.
pub trait Event {
    const NAME: &'static str;
}

/// Common fields that are common to all events. Some of these fields exits to
/// maintain compatibility with the qlog spec.
#[derive(Clone, Debug)]
pub struct Meta {
    pub endpoint_type: endpoint::Type,
    pub group_id: u64,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            endpoint_type: endpoint::Type::Server,
            group_id: 0,
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
        pub meta: Meta,
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
        pub meta: Meta,
        pub server_alpns: &'a [&'a [u8]],
        pub client_alpns: &'a [&'a [u8]],
        pub chosen_alpn: u32,
    }
);
