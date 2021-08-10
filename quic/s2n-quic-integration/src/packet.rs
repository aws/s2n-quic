// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Represents a packet that is sent over a network

use s2n_quic_core::{
    inet::{datagram, ExplicitCongestionNotification, SocketAddress},
    io::{rx, tx},
    path::{self, Handle as _},
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
    type Handle = path::Tuple;

    fn set<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        mut message: M,
    ) -> Result<usize, tx::Error> {
        let handle = message.path_handle();
        self.destination_address = handle.remote_address();
        self.source_address = handle.local_address();
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
    type Handle = path::Tuple;

    fn read(&mut self) -> Option<(datagram::Header<Self::Handle>, &mut [u8])> {
        let path = path::Tuple {
            remote_address: self.source_address,
            local_address: self.destination_address,
        };
        let header = datagram::Header {
            path,
            ecn: self.ecn,
        };
        Some((header, &mut self.payload[..]))
    }
}
