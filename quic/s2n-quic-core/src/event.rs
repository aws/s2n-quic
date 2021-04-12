// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint;
use paste::paste;

#[macro_use]
mod macros;

pub trait Event {
    const NAME: &'static str;
}

#[derive(Clone, Debug)]
pub struct Meta {
    pub vantage_point: endpoint::Type,
    pub group_id: u64,
}

events!(
    #[name = "transport::version_information"]
    struct VersionInformation<'a> {
        pub server: &'a [u32],
        pub client: &'a [u32],
        pub chosen: u32,
        pub meta: Meta,
    }

    #[name = "transport:alpn_information"]
    struct AlpnInformation<'a> {
        pub server: &'a [&'a [u8]],
        pub client: &'a [&'a [u8]],
        pub chosen: u32,
        pub meta: Meta,
    }
);
