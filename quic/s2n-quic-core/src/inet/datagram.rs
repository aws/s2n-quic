// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection, inet::ExplicitCongestionNotification, path::LocalAddress, time::Timestamp,
};

/// Header information for a datagram sent/received over the network
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Header<Path> {
    pub path: Path,
    pub ecn: ExplicitCongestionNotification,
}

/// Metadata for a datagram sent/received over the network
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DatagramInfo {
    pub timestamp: Timestamp,
    pub payload_len: usize,
    pub ecn: ExplicitCongestionNotification,
    pub destination_connection_id: connection::LocalId,
}

/// Additional metadata for a datagram sent/received over the network
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AncillaryData {
    pub ecn: ExplicitCongestionNotification,
    pub local_address: LocalAddress,
    pub local_interface: Option<i32>,
}
