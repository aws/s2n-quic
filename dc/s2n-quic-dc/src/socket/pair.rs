// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Options;
use std::{io, net::UdpSocket};

pub struct Pair {
    pub writer: UdpSocket,
    pub reader: UdpSocket,
}

impl Pair {
    #[inline]
    pub fn open(mut options: Options) -> io::Result<Self> {
        // have the OS select a random port for us
        options.addr.set_port(0);
        // don't reuse the ports since we don't have a consistent way to route packets
        options.reuse_port = Default::default();

        let writer = options.build_udp()?;
        let reader = options.build_udp()?;

        Ok(Self { writer, reader })
    }
}
