// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    inet::{ExplicitCongestionNotification, SocketAddress},
    time::Timestamp,
};

/// Metadata for a datagram sent/received over the network
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DatagramInfo {
    pub timestamp: Timestamp,
    pub remote_address: SocketAddress,
    pub payload_len: usize,
    pub ecn: ExplicitCongestionNotification,

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.2
    //# the Destination Connection ID is chosen by the recipient of the
    //# packet and is used to provide consistent routing
    pub destination_connection_id: connection::LocalId,
}
