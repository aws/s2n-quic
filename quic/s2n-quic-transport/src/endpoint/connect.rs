// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, Connection},
    endpoint::handle::ConnectorSender,
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use futures_channel::oneshot;
use s2n_quic_core::{
    application::Sni,
    inet::SocketAddress,
    path::{LocalAddress, RemoteAddress},
};

pub(crate) type ConnectionReceiver = oneshot::Receiver<Result<Connection, connection::Error>>;
pub(crate) type ConnectionSender = oneshot::Sender<Result<Connection, connection::Error>>;

#[derive(Debug)]
#[allow(dead_code)]
pub struct Connect {
    remote_address: RemoteAddress,
    local_address: Option<LocalAddress>,
    hostname: Option<Sni>,
}

impl Connect {
    /// Creates a connection attempt with the specified remote address
    pub fn new<Addr: Into<SocketAddress>>(addr: Addr) -> Self {
        Self {
            remote_address: addr.into().into(),
            local_address: None,
            hostname: None,
        }
    }

    /// Specifies the hostname to use for the connection
    pub fn with_hostname<Hostname: Into<Sni>>(self, hostname: Hostname) -> Self {
        Self {
            hostname: Some(hostname.into()),
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
        match core::mem::replace(&mut self.state, AttemptState::Unreachable) {
            AttemptState::Connect(request, mut opener, response) => {
                match opener.poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        match opener.try_send(request) {
                            Ok(_) => {
                                // transition to the waiting state
                                self.state = AttemptState::Waiting(response);
                            }
                            Err(err) if err.is_full() => {
                                // reset to the original state
                                self.state =
                                    AttemptState::Connect(err.into_inner(), opener, response);

                                // yield and wake up the task since the opener mis-reported its ready state
                                cx.waker().wake_by_ref();
                            }
                            Err(_) => {
                                // The endpoint has closed
                                return Err(connection::Error::Unspecified).into();
                            }
                        }
                    }
                    Poll::Ready(Err(_)) => {
                        // The endpoint has closed
                        return Err(connection::Error::Unspecified).into();
                    }
                    Poll::Pending => {
                        // reset to the original state
                        self.state = AttemptState::Connect(request, opener, response);
                    }
                }

                Poll::Pending
            }
            AttemptState::Waiting(mut response) => match Pin::new(&mut response).poll(cx) {
                Poll::Ready(Ok(res)) => Poll::Ready(res),
                Poll::Ready(Err(_)) => {
                    // The endpoint has closed
                    Err(connection::Error::Unspecified).into()
                }
                Poll::Pending => {
                    self.state = AttemptState::Waiting(response);
                    Poll::Pending
                }
            },
            AttemptState::Unreachable => {
                unreachable!("Unreachable is an immediate state and should not exist across polls");
            }
        }
    }
}
