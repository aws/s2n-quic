// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! impl_event {
    ($name:ident, $qlog_name:literal) => {
        impl Event for $name {}

        impl Meta for $name{
            fn name<'a>() -> &'a str {
                $qlog_name
            }
        }
    };
}

pub trait Event: Meta {}

pub trait Meta {
    fn name<'a>() -> &'a str;
}

#[non_exhaustive]
pub struct VersionInformation{}
impl_event!(
    VersionInformation, "transport:version_information"
);

#[non_exhaustive]
pub struct AlpnInformation{}
impl_event!(AlpnInformation, "transport:alpn_information");
