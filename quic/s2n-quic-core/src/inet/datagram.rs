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
    pub destination_connection_id_classification: connection::id::Classification,
    pub source_connection_id: Option<connection::PeerId>,
}

/// Additional metadata for a datagram sent/received over the network
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AncillaryData {
    pub ecn: ExplicitCongestionNotification,
    pub local_address: LocalAddress,
    /// The network interface the datagram is sent/received on
    ///
    /// Correctly threading this value through to connections ensures packets end up on the same
    /// network interfaces and thereby have consistent MAC addresses.
    pub local_interface: Option<u32>,
    /// Set when the packet buffer is an aggregate of multiple received packets
    pub segment_size: u16,
}
