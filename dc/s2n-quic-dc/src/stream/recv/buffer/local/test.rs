// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{recv, recv::buffer::Dispatch};
use std::net::{IpAddr, Ipv4Addr};

// Checks that a stream which saw EOF mid-packet will not expect more data to come in and returns an error.
#[test]
fn check_truncation() {
    let mut local = super::Local::new(super::msg::recv::Message::new(9000), None);
    let mut dispatch = NoopDispatch;

    // Populate the receive buffer enough that we can start parsing...
    let packet = vec![0b0101_0000];
    local.recv_buffer.test_recv(
        (IpAddr::from(Ipv4Addr::LOCALHOST), 1).into(),
        Default::default(),
        packet,
    );

    // OK, maybe more bytes will come in.
    local.dispatch_buffer_stream(&mut dispatch).unwrap();

    // Then we indicate that no more bytes are coming, at which point dispatch indicates an error
    // has happened.
    local.saw_fin = true;
    let err = local.dispatch_buffer_stream(&mut dispatch).unwrap_err();

    assert!(matches!(err.kind(), recv::error::Kind::Decode));
}

struct NoopDispatch;

impl Dispatch for NoopDispatch {
    fn on_packet(
        &mut self,
        _remote_addr: &s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: crate::packet::Packet,
    ) -> Result<(), crate::stream::recv::Error> {
        // should never actually parse a packet
        unreachable!()
    }
}
