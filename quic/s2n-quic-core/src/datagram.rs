// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// The datagram endpoint trait provides a way to implement custom unreliable datagram
/// sending and receiving logic. The Sender type should be implemented for custom
/// sending behavior, and the Receiver type should be implemented for custom
/// receiving behavior.
pub trait Endpoint: 'static + Send {
    type Sender: Sender;
    type Receiver: Receiver;

    fn split(&mut self) -> (Self::Sender, Self::Receiver);
}

pub trait Receiver: 'static + Send {}
pub trait Sender: 'static + Send {
    /// A callback that allows users to write datagrams directly to the packet.
    fn on_transmit<P: Packet>(&mut self, packet: &mut P);
}

/// A packet will be available during the on_transmit callback. Use the methods
/// defined here to interrogate the packet struct and write datagrams to the packet.
pub trait Packet {
    /// Returns the remaining space in the packet left to write datagrams
    fn remaining_capacity(&self) -> usize;

    /// Returns the largest datagram that can fit in space remaining in the packet
    fn maximum_datagram_payload(&self) -> usize;

    /// Writes a single datagram to a packet. This function should be called
    /// per datagram.
    fn write_datagram(&mut self, data: &[u8]);

    /// Returns whether or not there is reliable data waiting to be sent.
    ///
    /// Use method to decide whether or not to cede the packet space to the stream data.
    fn pending_streams(&self) -> bool;
}

#[derive(Debug, Default)]
pub struct Disabled;

impl Endpoint for Disabled {
    type Sender = DisabledSender;
    type Receiver = DisabledReceiver;

    fn split(&mut self) -> (Self::Sender, Self::Receiver) {
        (DisabledSender, DisabledReceiver)
    }
}
pub struct DisabledSender;
pub struct DisabledReceiver;

impl Sender for DisabledSender {
    fn on_transmit<P: Packet>(&mut self, _packet: &mut P) {}
}
impl Receiver for DisabledReceiver {}
