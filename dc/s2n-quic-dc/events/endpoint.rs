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

#[event("dc:handshake_probe")]
#[subject(endpoint)]
/// Emitted after a pre-handshake liveness probe completes
struct DcHandshakeProbe<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[timer("latency")]
    latency: core::time::Duration,

    #[nominal_counter("outcome")]
    outcome: HandshakeProbeOutcome,
}

enum HandshakeProbeOutcome {
    Responsive,
    Unresponsive,
    Error,
}
