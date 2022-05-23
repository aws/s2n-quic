// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("transport::version_information")]
#[subject(endpoint)]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.1
//# QUIC endpoints each have their own list of of QUIC versions they
//# support.
/// QUIC version
struct VersionInformation<'a> {
    server_versions: &'a [u32],
    client_versions: &'a [u32],
    chosen_version: Option<u32>,
}

#[event("transport:packet_sent")]
#[subject(endpoint)]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.5
/// Packet was sent by the endpoint
struct EndpointPacketSent {
    packet_header: PacketHeader,
}

#[event("transport:packet_received")]
#[subject(endpoint)]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.6
/// Packet was received by the endpoint
struct EndpointPacketReceived {
    packet_header: PacketHeader,
}

#[event("transport:datagram_sent")]
#[subject(endpoint)]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.10
/// Datagram sent by the endpoint
struct EndpointDatagramSent {
    len: u16,
    /// The GSO offset at which this datagram was written
    ///
    /// If this value is greater than 0, it indicates that this datagram has been sent with other
    /// segments in a single buffer.
    ///
    /// See the [Linux kernel documentation](https://www.kernel.org/doc/html/latest/networking/segmentation-offloads.html#generic-segmentation-offload) for more details.
    gso_offset: usize,
}

#[event("transport:datagram_received")]
#[subject(endpoint)]
#[bpf]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.11
/// Datagram received by the endpoint
struct EndpointDatagramReceived {
    len: u16,
}

#[event("transport:datagram_dropped")]
#[subject(endpoint)]
//= https://tools.ietf.org/id/draft-marx-qlog-event-definitions-quic-h3-02#5.3.12
/// Datagram dropped by the endpoint
struct EndpointDatagramDropped {
    len: u16,
    reason: DatagramDropReason,
}

#[event("transport:connection_attempt_failed")]
#[subject(endpoint)]
struct EndpointConnectionAttemptFailed {
    error: crate::connection::Error,
}
