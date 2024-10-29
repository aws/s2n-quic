// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("endpoint:initialized")]
#[subject(endpoint)]
struct EndpointInitialized<'a> {
    acceptor_addr: SocketAddress<'a>,
    handshake_addr: SocketAddress<'a>,
    tcp: bool,
    udp: bool,
}
