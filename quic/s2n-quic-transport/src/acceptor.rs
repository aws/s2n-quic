//! Allows to accept connections

use crate::{connection::Connection, unbounded_channel::Receiver};
use core::task::{Context, Poll};

/// The [`Acceptor`] allows to accept incoming QUIC connections on an `Endpoint`.
pub struct Acceptor {
    receiver: Receiver<Connection>,
}

impl Acceptor {
    /// Creates a new `Acceptor` from a `Receiver`.
    pub(crate) fn new(receiver: Receiver<Connection>) -> Self {
        Self { receiver }
    }

    /// Polls for incoming connections and returns them.
    ///
    /// The method will return
    /// - `Poll::Ready(connection)` if a connection was accepted.
    /// - `Poll::Pending` if no new connection was accepted yet.
    ///   In this case the caller must retry polling as soon as a client
    ///   establishes a connection.
    ///   In order to notify the application of this condition,
    ///   the method will save the [`Waker`] which is provided as part of the
    ///   [`Context`] parameter, and notify it as soon as retrying
    ///   the method will yield a different result.
    pub fn poll_accept(&mut self, context: &Context) -> Poll<Connection> {
        self.receiver.poll_next(context)
    }
}
