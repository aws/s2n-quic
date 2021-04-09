// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub trait Event: Meta {}

pub trait Meta {
    fn evt_name<'a>() -> &'a str;
}

#[non_exhaustive]
pub struct VersionInformation{}

impl Event for VersionInformation{}

impl Meta for VersionInformation{
    fn evt_name<'a>() -> &'a str {
        "transport:version_information"
    }
}

#[non_exhaustive]
pub struct AlpnInformation{}

