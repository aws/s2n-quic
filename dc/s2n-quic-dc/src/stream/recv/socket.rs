// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::router::Router;
use crate::{
    msg,
    stream::{server, socket::fd::udp},
};
use std::net::UdpSocket;

pub fn worker_udp(socket: UdpSocket, router: Router<msg::recv::Message>) {
    let mut buffer = msg::recv::Message::new(9000.try_into().unwrap());
    loop {
        let res = buffer.recv_with(|addr, cmsg, buffer| {
            udp::recv(&socket, addr, cmsg, buffer, Default::default())
        });

        if let Err(err) = res {
            tracing::error!("socket recv error: {err}");
            continue;
        }

        let packet = server::InitialPacket::peek(&mut buffer, 16);

        let packet = match packet {
            Ok(packet) => packet,
            Err(err) => {
                tracing::error!("failed to peek packet: {err}");
                continue;
            }
        };

        router.send(packet.stream_id.route_key, buffer.take());
    }
}
