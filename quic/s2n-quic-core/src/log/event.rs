// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! impl_event {
    ($name:ident, $qlog_name:literal) => {
        impl Event for $name {}

        impl Meta for $name{
            fn name(&self) -> &str {
                self.name
            }
        }
    };
}

/// The Event trait marks all possible types of events that can be emitted by
/// the library.
pub trait Event: Meta {}

/// This is meant to capture common meta values that are shared across events.
/// Some of these are included to maintain compatibility with the main qlog
/// rfc.
pub trait Meta {
    fn name(&self) -> &str;
}

#[non_exhaustive]
pub struct VersionInformation{
    name: &'static str
}
impl_event!(
    VersionInformation, "transport:version_information"
);

#[non_exhaustive]
pub struct AlpnInformation{
    name: &'static str
}
impl_event!(AlpnInformation, "transport:alpn_information");
