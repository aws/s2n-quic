// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::datagram::{ConnectionInfo, Endpoint, Packet, Receiver, Sender};

#[derive(Debug, Default)]
pub struct Disabled(());

impl Endpoint for Disabled {
    type Sender = DisabledSender;
    type Receiver = DisabledReceiver;

    fn create_connection(&mut self, _info: &ConnectionInfo) -> (Self::Sender, Self::Receiver) {
        (DisabledSender(()), DisabledReceiver(()))
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
}

impl Receiver for DisabledReceiver {
    fn on_datagram(&mut self, _datagram: &[u8]) {}
}
