// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::connection;

/// The datagram endpoint trait provides a way to implement custom unreliable datagram
/// sending and receiving logic. The Sender type should be implemented for custom
/// sending behavior, and the Receiver type should be implemented for custom
/// receiving behavior.
pub trait Endpoint: 'static + Send {
    type Sender: Sender;
    type Receiver: Receiver;

    fn create_connection(&mut self, info: &ConnectionInfo) -> (Self::Sender, Self::Receiver);

    /// Returns the maximum datagram frame size the provider is willing to accept
    fn max_datagram_frame_size(&self, info: &PreConnectionInfo) -> u64;
}

/// ConnectionInfo contains the peer's limit on the size of datagrams
/// they accept
///
/// Sending a datagram larger than this will result in an error
#[non_exhaustive]
#[derive(Debug)]
pub struct ConnectionInfo {
    pub max_datagram_payload: u64,
}

impl ConnectionInfo {
    #[doc(hidden)]
    pub fn new(max_datagram_payload: u64) -> Self {
        ConnectionInfo {
            max_datagram_payload,
        }
    }
}

/// PreConnectionInfo will contain information needed to determine whether
/// or not a provider will accept datagrams.
#[non_exhaustive]
pub struct PreConnectionInfo(());

impl PreConnectionInfo {
    #[doc(hidden)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        PreConnectionInfo(())
    }
}

/// ReceiveContext contains information about the connection.
#[non_exhaustive]
#[derive(Debug)]
pub struct ReceiveContext<'a> {
    /// This is the current connection path this datagram was received on.
    pub path: crate::event::api::Path<'a>,
}

impl<'a> ReceiveContext<'a> {
    #[doc(hidden)]
    pub fn new(path: crate::event::api::Path<'a>) -> Self {
        ReceiveContext { path }
    }
}

/// Allows users to configure the behavior of receiving datagrams.
pub trait Receiver: 'static + Send {
    /// A callback that gives users direct access to datagrams as they are read off a packet
    fn on_datagram(&mut self, context: &ReceiveContext<'_>, datagram: &[u8]);

    /// A callback used to notify the application in the case of a connection error
    fn on_connection_error(&mut self, error: connection::Error);
}

/// Allows users to configure the behavior of sending datagrams.
pub trait Sender: 'static + Send {
    /// A callback that allows users to write datagrams directly to the packet
    fn on_transmit<P: Packet>(&mut self, packet: &mut P);

    /// A callback that checks if a user has datagrams ready to send
    ///
    /// Use method to trigger the on_transmit callback
    fn has_transmission_interest(&self) -> bool;

    /// A callback used to notify the application in the case of a connection error
    fn on_connection_error(&mut self, error: connection::Error);
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

    /// Returns whether or not datagrams are prioritized in this packet or not.
    ///
    /// Datagrams get prioritized every other packet, which gives the application the best
    /// chance to send a large datagram.
    fn datagrams_prioritized(&self) -> bool;
}

#[non_exhaustive]
#[derive(Debug)]
pub enum WriteError {
    ExceedsPacketCapacity,
    ExceedsPeerTransportLimits,
}
