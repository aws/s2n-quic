// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(unused)]

/// Event is a marker trait which is used for collecting all event types
/// of interest for logging or metric collection.
pub trait Event {}

macro_rules! impl_event {
    ($name:ident, $qlog_name:literal) => {
        impl Event for $name<'_> {}

        impl $name<'_> {
            fn name(&self) -> &str {
                $qlog_name
            }
        }
    };
}

#[non_exhaustive]
pub struct VersionInformation<'a> {
    vantage_point: &'a str,
    group_id: &'a str,
    server_versions: &'a [&'a str],
    client_versions: &'a [&'a str],
    chosen_version: &'a [&'a str],
}
impl_event!(VersionInformation, "transport:version_information");

#[non_exhaustive]
pub struct AlpnInformation<'a> {
    vantage_point: &'a str,
    group_id: &'a str,
    server_alpns: &'a [&'a str],
    client_alpns: &'a [&'a str],
    chosen_alpn: &'a [&'a str],
}
impl_event!(AlpnInformation, "transport:alpn_information");
