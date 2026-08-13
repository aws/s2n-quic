// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("endpoint:initialized")]
#[subject(endpoint)]
struct EndpointInitialized<'a> {
    #[nominal_counter("acceptor.protocol")]
    acceptor_addr: SocketAddress<'a>,
    #[nominal_counter("handshake.protocol")]
    handshake_addr: SocketAddress<'a>,
    #[bool_counter("tcp")]
    tcp: bool,
    #[bool_counter("udp")]
    udp: bool,
}

#[event("dc:connection_timeout")]
#[subject(endpoint)]
/// Emitted when the DC handshake confirmation or MTU probing times out
struct DcConnectionTimeout<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,
}

#[event("offload:task_metrics")]
#[subject(endpoint)]
struct OffloadTaskMetrics {
    #[measure("mean_poll_duration", Duration)]
    mean_poll_duration: core::time::Duration,
    #[measure("mean_scheduled_duration", Duration)]
    mean_scheduled_duration: core::time::Duration,
}
