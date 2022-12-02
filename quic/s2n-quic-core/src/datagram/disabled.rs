// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::datagram::{
    ConnectionInfo, Endpoint, Packet, PreConnectionInfo, ReceiveContext, Receiver, Sender,
};

#[derive(Debug, Default)]
pub struct Disabled(());

impl Endpoint for Disabled {
    type Sender = DisabledSender;
    type Receiver = DisabledReceiver;

    fn create_connection(&mut self, _info: &ConnectionInfo) -> (Self::Sender, Self::Receiver) {
        (DisabledSender(()), DisabledReceiver(()))
    }

    fn max_datagram_frame_size(&self, _info: &PreConnectionInfo) -> u64 {
        0
    }
}

pub struct DisabledSender(());
pub struct DisabledReceiver(());

impl Sender for DisabledSender {
    fn on_transmit<P: Packet>(&mut self, _packet: &mut P) {}

    #[inline]
    fn has_transmission_interest(&self) -> bool {
        false
    }

    fn on_connection_error(&mut self, _error: crate::connection::Error) {}
}

impl Receiver for DisabledReceiver {
    fn on_datagram(&mut self, _: &ReceiveContext<'_>, _: &[u8]) {}

    fn on_connection_error(&mut self, _error: crate::connection::Error) {}
}
