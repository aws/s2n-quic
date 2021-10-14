// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Allows to accept connections

use crate::{connection::Connection, endpoint::connect};
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures_channel::mpsc;
use futures_core::Stream;

/// Held by application. Used to receive when a new connection is accepted.
pub(crate) type AcceptorReceiver = mpsc::UnboundedReceiver<Connection>;
/// Held by library. Used to send a new connection when a datagram is received.
pub(crate) type AcceptorSender = mpsc::UnboundedSender<Connection>;

/// Held by library. Used to receive new connections requested by the application.
pub(crate) type ConnectorReceiver = mpsc::Receiver<connect::Request>;
/// Held by application. Used to send request for new connections to library.
pub(crate) type ConnectorSender = mpsc::Sender<connect::Request>;

/// The [`Handle`] allows applications to accept and open QUIC connections on an `Endpoint`.
#[derive(Debug)]
pub(crate) struct Handle {
    pub acceptor: Acceptor,
    pub connector: Connector,
}

impl Handle {
    /// Creates a new `Handle` with a limit opening connection limit.
    pub(crate) fn new(max_opening_connections: usize) -> (Self, AcceptorSender, ConnectorReceiver) {
        let (acceptor_sender, acceptor_receiver) = mpsc::unbounded();
        let (connector_sender, connector_receiver) = mpsc::channel(max_opening_connections);
        let handle = Self {
            acceptor: Acceptor(acceptor_receiver),
            connector: Connector(connector_sender),
        };
        (handle, acceptor_sender, connector_receiver)
    }
}

#[derive(Debug)]
pub struct Acceptor(AcceptorReceiver);

impl Acceptor {
    /// Polls for incoming connections and returns them.
    ///
    /// The method will return
    /// - `Poll::Ready(Some(connection))` if a connection was accepted.
    /// - `Poll::Ready(None)` if the acceptor is closed.
    /// - `Poll::Pending` if no new connection was accepted yet.
    ///   In this case the caller must retry polling as soon as a client
    ///   establishes a connection.
    ///   In order to notify the application of this condition,
    ///   the method will save the [`core::task::Waker`] which is provided as part of the
    ///   [`Context`] parameter, and notify it as soon as retrying
    ///   the method will yield a different result.
    pub fn poll_accept(&mut self, context: &mut Context) -> Poll<Option<Connection>> {
        match Stream::poll_next(Pin::new(&mut self.0), context) {
            Poll::Ready(Some(connection)) => Poll::Ready(Some(connection)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Connector(ConnectorSender);

impl Connector {
    /// Attempts to establish a connection to an endpoint and returns a future to be awaited
    pub fn connect(&self, connect: connect::Connect) -> connect::Attempt {
        connect::Attempt::new(&self.0, connect)
    }
}
