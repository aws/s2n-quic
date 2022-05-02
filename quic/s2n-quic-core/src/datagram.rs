// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
pub trait Endpoint: 'static + Send {
    type Sender: Sender;
    type Receiver: Receiver;

    fn new_datagram(&mut self) -> (Self::Sender, Self::Receiver);
}

pub trait Receiver: 'static + Send {}
pub trait Sender: 'static + Send {
    fn on_transmit<P: Packet>(&mut self, _packet: &mut P);
}

pub trait Packet {
    fn remaining_capacity(&self) -> usize;
    fn maximum_datagram_payload(&self) -> usize;
    fn write_datagram(&mut self, data: &[u8]);
    fn pending_streams(&self) -> bool;
}

#[derive(Debug, Default)]
pub struct Disabled;

impl Endpoint for Disabled {
    type Sender = DisabledSender;
    type Receiver = DisabledReceiver;

    fn new_datagram(&mut self) -> (Self::Sender, Self::Receiver) {
        (DisabledSender, DisabledReceiver)
    }
}
pub struct DisabledSender;
pub struct DisabledReceiver;

impl Sender for DisabledSender {
    fn on_transmit<P: Packet>(&mut self, _packet: &mut P) {}
}
impl Receiver for DisabledReceiver {}
