// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(unused)]

/// Event is a marker trait which is used for collecting all event types
/// of interest for logging or metric collection.
pub trait Event: Meta {}

/// This is meant to capture high level values used to categorize and
/// aggregate events. Some naming semantics are taken from the qlog rfc.
pub trait Meta {
    /// Taken from qlog rfc. This value is composed of category and type
    fn name(&self) -> &str;
}

macro_rules! impl_event {
    ($name:ident, $qlog_name:literal) => {
        impl Event for $name {}

        impl Meta for $name {
            fn name(&self) -> &str {
                self.name
            }
        }
    };
}

#[non_exhaustive]
pub struct VersionInformation {
    name: &'static str,
    server_versions: Vec<String>,
    client_versions: Vec<String>,
    chosen_version: Vec<String>,
}
impl_event!(VersionInformation, "transport:version_information");

#[non_exhaustive]
pub struct AlpnInformation {
    name: &'static str,
    server_alpns: Vec<String>,
    client_alpns: Vec<String>,
    chosen_alpn: Vec<String>,
}
impl_event!(AlpnInformation, "transport:alpn_information");
