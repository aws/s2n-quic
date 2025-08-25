// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Context;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Q: What happens when the application connects with an unspecified IPv4 address?
///
/// A: The connection assumes localhost and successfully connects
#[tokio::test]
async fn unspecified_ipv4_test() {
    let context = Context::new().await;

    let mut acceptor_addr = context.acceptor_addr();
    acceptor_addr.set_ip(Ipv4Addr::UNSPECIFIED.into());

    let (client, server) = context.pair_with(acceptor_addr).await;

    // We're currently not updating the remote address for UDP
    // TODO fix this
    //
    // This will be a bit awkward because the client won't actually know what the assigned IP
    // is until it actually gets an ACK for its initial packet.
    if !context.protocol().is_udp() {
        assert_eq!(client.peer_addr().unwrap().ip(), Ipv4Addr::LOCALHOST);
        assert_eq!(server.peer_addr().unwrap().ip(), Ipv4Addr::LOCALHOST);
    }
}

/// Q: What happens when the application connects with an unspecified IPv6 address?
///
/// A: The connection assumes localhost and successfully connects
#[tokio::test]
async fn unspecified_ipv6_test() {
    let context = Context::bind("[::1]:0".parse().unwrap()).await;

    let mut acceptor_addr = context.acceptor_addr();
    acceptor_addr.set_ip(Ipv6Addr::UNSPECIFIED.into());

    let (client, server) = context.pair_with(acceptor_addr).await;

    // We're currently not updating the remote address for UDP
    // TODO fix this
    //
    // This will be a bit awkward because the client won't actually know what the assigned IP
    // is until it actually gets an ACK for its initial packet.
    if !context.protocol().is_udp() {
        assert_eq!(client.peer_addr().unwrap().ip(), Ipv6Addr::LOCALHOST);
        assert_eq!(server.peer_addr().unwrap().ip(), Ipv6Addr::LOCALHOST);
    }
}

/// Q: What happens when the application connects with both IPv6 and IPv4?
///
/// A: Both connections succeed
#[tokio::test]
#[ignore = "dual stack isn't currently implemented"]
async fn dual_stack_test() {
    let context = Context::bind("[::1]:0".parse().unwrap()).await;

    let (_client, _server) = context.pair().await;

    let mut acceptor_addr = context.acceptor_addr();
    acceptor_addr.set_ip(Ipv4Addr::LOCALHOST.into());

    let (_client, _server) = context.pair_with(acceptor_addr).await;
}
