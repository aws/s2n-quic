// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Allows to accept connections

use crate::{
    connection::Connection,
    endpoint::{close, connect},
};
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures_channel::mpsc;
use futures_core::Stream;
use std::{
    sync::{atomic::AtomicBool, Arc},
    task::Waker,
};

/// Held by application. Used to accept new connections.
pub(crate) type AcceptorReceiver = mpsc::UnboundedReceiver<Connection>;
/// Held by library. Used to notify the application of newly-accepted connections.
pub(crate) type AcceptorSender = mpsc::UnboundedSender<Connection>;

/// Held by library. Used to receive connection attempts from the application.
pub(crate) type ConnectorReceiver = mpsc::Receiver<connect::Request>;
/// Held by application. Used to submit connection attempts to the library.
pub(crate) type ConnectorSender = mpsc::Sender<connect::Request>;

/// Held by library. Used to receive close attempts from the application.
pub(crate) type CloseReceiver = mpsc::Receiver<Waker>;
/// Held by the application. Used to submit connection close attempts to the library.
pub(crate) type CloseSender = mpsc::Sender<Waker>;

/// The [`Handle`] allows applications to accept and open QUIC connections on an `Endpoint`.
#[derive(Debug)]
pub(crate) struct Handle {
    pub acceptor: Acceptor,
    pub connector: Connector,
}

#[derive(Clone, Debug)]
pub struct Closer {
    pub(crate) close_sender: CloseSender,
    pub(crate) is_open: Arc<AtomicBool>,
}

impl Handle {
    /// Creates a new `Handle` with a limit opening connection limit.
    pub(crate) fn new(
        max_opening_connections: usize,
    ) -> (
        Self,
        AcceptorSender,
        ConnectorReceiver,
        CloseReceiver,
        Arc<AtomicBool>,
    ) {
        let (acceptor_sender, acceptor_receiver) = mpsc::unbounded();
        let (connector_sender, connector_receiver) = mpsc::channel(max_opening_connections);

        let (close_sender, closer_receiver) = mpsc::channel(max_opening_connections);

        let is_open = Arc::new(AtomicBool::new(true));
        let closer = Closer {
            close_sender,
            is_open: is_open.clone(),
        };
        let handle = Self {
            acceptor: Acceptor {
                acceptor: acceptor_receiver,
                closer: closer.clone(),
            },
            connector: Connector {
                connector: connector_sender,
                closer,
            },
        };
        (
            handle,
            acceptor_sender,
            connector_receiver,
            closer_receiver,
            is_open,
        )
    }
}

#[derive(Debug)]
pub struct Acceptor {
    acceptor: AcceptorReceiver,
    closer: Closer,
}

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
        match Stream::poll_next(Pin::new(&mut self.acceptor), context) {
            Poll::Ready(Some(connection)) => Poll::Ready(Some(connection)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    pub fn poll_close(&self) -> close::Attempt {
        close::Attempt::new(self.closer.clone())
    }
}

#[derive(Clone, Debug)]
pub struct Connector {
    connector: ConnectorSender,
    closer: Closer,
}

impl Connector {
    /// Attempts to establish a connection to an endpoint and returns a future to be awaited
    pub fn connect(&self, connect: connect::Connect) -> connect::Attempt {
        connect::Attempt::new(&self.connector, connect)
    }

    pub fn poll_close(&self) -> close::Attempt {
        close::Attempt::new(self.closer.clone())
    }
}
