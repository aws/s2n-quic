// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, Connection},
    endpoint::handle::ConnectorSender,
};
use core::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use futures_channel::oneshot;
use s2n_quic_core::{application::ServerName, inet::SocketAddress, path::RemoteAddress};

/// Held by connection Attempt future. Used to receive the actual connection.
pub(crate) type ConnectionReceiver = oneshot::Receiver<Result<Connection, connection::Error>>;

/// Held within the library connection_container. Used to send the actual connection once
/// its been created.
pub(crate) type ConnectionSender = oneshot::Sender<Result<Connection, connection::Error>>;

#[derive(Clone, Debug)]
pub struct Connect {
    pub(crate) remote_address: RemoteAddress,
    pub(crate) server_name: Option<ServerName>,
}

impl fmt::Display for Connect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            if let Some(hostname) = self.server_name.as_deref() {
                write!(f, "{hostname} at {}", &*self.remote_address)
            } else {
                write!(f, "{}", &*self.remote_address)
            }
        } else if let Some(hostname) = self.server_name.as_deref() {
            write!(f, "{hostname}")
        } else {
            write!(f, "{}", &*self.remote_address)
        }
    }
}

impl Connect {
    /// Creates a connection attempt with the specified remote address
    pub fn new<Addr: Into<SocketAddress>>(addr: Addr) -> Self {
        Self {
            remote_address: addr.into().into(),
            server_name: None,
        }
    }

    /// Specifies the server name to use for the connection
    #[must_use]
    pub fn with_server_name<Name: Into<ServerName>>(self, server_name: Name) -> Self {
        Self {
            server_name: Some(server_name.into()),
            ..self
        }
    }
}

/// Make it easy for applications to create a connection attempt without importing the `Connect` struct
impl<T: Into<SocketAddress>> From<T> for Connect {
    fn from(addr: T) -> Self {
        Self::new(addr)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct Request {
    pub connect: Connect,
    pub sender: ConnectionSender,
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Attempt {
    state: AttemptState,
}

impl Attempt {
    /// Creates a connection attempt
    ///
    /// The flow is currently implemented as follows:
    ///
    /// * The applications provides a `Connect` struct containing information for the remote endpoint
    /// * The attempt creates a oneshot channel and creates a `Request` with the sender and `Connect` struct
    /// * The attempt returns a `Self` while holding on to the oneshot receiver
    /// * The application polls the `Attempt` until either a successful `Connection` or `connection::Error` is
    ///   received over the oneshot receiver.
    pub(crate) fn new(opener: &ConnectorSender, connect: Connect) -> Self {
        // open a oneshot channel to receive the connection or error after the endpoint attempted the handshake
        let (response, receiver) = oneshot::channel();
        // The request includes both the connection info and response onshot channel
        let request = Request {
            connect,
            sender: response,
        };
        Self {
            state: AttemptState::Connect(request, opener.clone(), receiver),
        }
    }

    #[inline]
    fn poll_state(&mut self, cx: &mut Context) -> Poll<<Self as Future>::Output> {
        loop {
            match core::mem::replace(&mut self.state, AttemptState::Unreachable) {
                AttemptState::Connect(request, mut opener, response) => {
                    match opener.poll_ready(cx) {
                        Poll::Ready(Ok(())) => {
                            match opener.try_send(request) {
                                Ok(_) => {
                                    // transition to the waiting state
                                    self.state = AttemptState::Waiting(response);
                                    continue;
                                }
                                Err(err) if err.is_full() => {
                                    // reset to the original state
                                    self.state =
                                        AttemptState::Connect(err.into_inner(), opener, response);

                                    // yield and wake up the task since the opener misreported its ready state
                                    cx.waker().wake_by_ref();
                                }
                                Err(_) => {
                                    // The endpoint has closed
                                    return Err(connection::Error::unspecified()).into();
                                }
                            }
                        }
                        Poll::Ready(Err(_)) => {
                            // The endpoint has closed
                            return Err(connection::Error::unspecified()).into();
                        }
                        Poll::Pending => {
                            // reset to the original state
                            self.state = AttemptState::Connect(request, opener, response);
                        }
                    }

                    return Poll::Pending;
                }
                AttemptState::Waiting(mut response) => {
                    return match Pin::new(&mut response).poll(cx) {
                        Poll::Ready(Ok(res)) => Poll::Ready(res),
                        Poll::Ready(Err(_)) => {
                            // The endpoint has closed
                            Err(connection::Error::unspecified()).into()
                        }
                        Poll::Pending => {
                            self.state = AttemptState::Waiting(response);
                            Poll::Pending
                        }
                    };
                }
                AttemptState::Unreachable => {
                    unreachable!(
                        "Unreachable is an immediate state and should not exist across polls"
                    );
                }
            }
        }
    }
}

enum AttemptState {
    /// The attempt is currently waiting for capacity in the `ConnectorSender` to make the `Request`
    Connect(Request, ConnectorSender, ConnectionReceiver),
    /// The attempt is currently waiting for a response back from the endpoint on the `ConnectionReceiver`
    Waiting(ConnectionReceiver),
    /// This is an intermediate state and should not persist across calls to `poll`
    Unreachable,
}

impl Future for Attempt {
    type Output = Result<Connection, connection::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| self.poll_state(cx))
    }
}
