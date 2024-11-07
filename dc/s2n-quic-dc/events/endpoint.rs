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
