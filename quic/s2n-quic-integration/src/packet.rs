// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Represents a packet that is sent over a network

use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::{rx, tx},
};

#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub struct Packet {
    pub destination_address: SocketAddress,
    pub source_address: SocketAddress,
    pub ecn: ExplicitCongestionNotification,
    pub ipv6_flow_label: u32,
    pub payload: Vec<u8>,
}

impl tx::Entry for Packet {
    fn set<M: tx::Message>(&mut self, mut message: M) -> Result<usize, tx::Error> {
        self.destination_address = message.remote_address();
        self.ecn = message.ecn();
        self.ipv6_flow_label = message.ipv6_flow_label();

        let len = message.write_payload(&mut self.payload[..]);

        if len == 0 {
            return Err(tx::Error::EmptyPayload);
        }

        self.payload.truncate(len);

        Ok(len)
    }

    fn payload(&self) -> &[u8] {
        &self.payload[..]
    }

    fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.payload[..]
    }
}

impl rx::Entry for Packet {
    fn remote_address(&self) -> Option<SocketAddress> {
        Some(self.source_address)
    }

    fn ecn(&self) -> ExplicitCongestionNotification {
        self.ecn
    }

    fn payload(&self) -> &[u8] {
        &self.payload[..]
    }

    fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.payload[..]
    }
}
