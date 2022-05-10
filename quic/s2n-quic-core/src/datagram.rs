// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// The datagram endpoint trait provides a way to implement custom unreliable datagram
/// sending and receiving logic. The Sender type should be implemented for custom
/// sending behavior, and the Receiver type should be implemented for custom
/// receiving behavior.
pub trait Endpoint: 'static + Send {
    type Sender: Sender;
    type Receiver: Receiver;

    fn create_connection(&mut self, info: &ConnectionInfo) -> (Self::Sender, Self::Receiver);
}

// ConnectionInfo will eventually contain information needed to set up a
// datagram provider
#[non_exhaustive]
pub struct ConnectionInfo {}

impl ConnectionInfo {
    pub fn new() -> Self {
        ConnectionInfo {}
    }
}

impl Default for ConnectionInfo {
    fn default() -> Self {
        ConnectionInfo::new()
    }
}

pub trait Receiver: 'static + Send {}
pub trait Sender: 'static + Send {
    /// A callback that allows users to write datagrams directly to the packet.
    fn on_transmit<P: Packet>(&mut self, packet: &mut P);

    /// A callback that checks if a user has datagrams ready to send
    ///
    /// Use method to trigger the on_transmit callback
    fn has_transmission_interest(&self) -> bool;
}

/// A packet will be available during the on_transmit callback. Use the methods
/// defined here to interrogate the packet struct and write datagrams to the packet.
pub trait Packet {
    /// Returns the remaining space in the packet left to write datagrams
    fn remaining_capacity(&self) -> usize;

    /// Writes a single datagram to a packet. This function should be called
    /// per datagram.
    fn write_datagram(&mut self, data: &[u8]) -> Result<(), WriteError>;

    /// Returns whether or not there is reliable data waiting to be sent.
    ///
    /// Use method to decide whether or not to cede the packet space to the stream data.
    fn has_pending_streams(&self) -> bool;
}

#[non_exhaustive]
#[derive(Debug)]
pub enum WriteError {
    DatagramIsTooLarge,
}

#[derive(Debug, Default)]
pub struct Disabled;

impl Endpoint for Disabled {
    type Sender = DisabledSender;
    type Receiver = DisabledReceiver;

    fn create_connection(&mut self, _info: &ConnectionInfo) -> (Self::Sender, Self::Receiver) {
        (DisabledSender, DisabledReceiver)
    }
}

pub struct DisabledSender;
pub struct DisabledReceiver;

impl Sender for DisabledSender {
    fn on_transmit<P: Packet>(&mut self, _packet: &mut P) {}

    #[inline]
    fn has_transmission_interest(&self) -> bool {
        false
    }
}

impl Receiver for DisabledReceiver {}
