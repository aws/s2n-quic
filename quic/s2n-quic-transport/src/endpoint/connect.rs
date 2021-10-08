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

type ConnectionReceiver = oneshot::Receiver<Result<Connection, connection::Error>>;
type ConnectionSender = oneshot::Sender<Result<Connection, connection::Error>>;

#[derive(Debug)]
pub struct Connect {
    remote_address: RemoteAddress,
    local_address: Option<LocalAddress>,
    hostname: Option<Sni>,
}

impl Connect {
    pub fn new<Addr: Into<SocketAddress>>(addr: Addr) -> Self {
        Self {
            remote_address: addr.into().into(),
            local_address: None,
            hostname: None,
        }
    }

    pub fn with_hostname<Hostname: Into<Sni>>(self, hostname: Hostname) -> Self {
        Self {
            hostname: Some(hostname.into()),
            ..self
        }
    }
}

#[derive(Debug)]
pub struct Request {
    connect: Connect,
    sender: ConnectionSender,
}

pub struct Attempt {
    state: AttemptState,
}

impl Attempt {
    pub(crate) fn new(opener: &ConnectorSender, connect: Connect) -> Self {
        Self {
            state: AttemptState::Connect(connect, opener.clone()),
        }
    }
}

enum AttemptState {
    Connect(Connect, ConnectorSender),
    Waiting(ConnectionReceiver),
    Unreachable,
}

impl Future for Attempt {
    type Output = Result<Connection, connection::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match core::mem::replace(&mut self.state, AttemptState::Unreachable) {
            AttemptState::Connect(connect, mut sender) => {
                match sender.poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        let (response, receiver) = oneshot::channel();
                        let request = Request {
                            connect,
                            sender: response,
                        };

                        if sender.start_send(request).is_err() {
                            // The endpoint has closed
                            return Err(connection::Error::Unspecified).into();
                        }

                        self.state = AttemptState::Waiting(receiver);

                        Poll::Pending
                    }
                    Poll::Ready(Err(_)) => {
                        // The endpoint has closed
                        Err(connection::Error::Unspecified).into()
                    }
                    Poll::Pending => {
                        self.state = AttemptState::Connect(connect, sender);
                        Poll::Pending
                    }
                }
            }
            AttemptState::Waiting(mut receiver) => match Pin::new(&mut receiver).poll(cx) {
                Poll::Ready(Ok(res)) => Poll::Ready(res),
                Poll::Ready(Err(_)) => {
                    // The endpoint has closed
                    Err(connection::Error::Unspecified).into()
                }
                Poll::Pending => {
                    self.state = AttemptState::Waiting(receiver);
                    Poll::Pending
                }
            },
            AttemptState::Unreachable => {
                unreachable!("Unreachable is an immediate state and should not exist across polls");
            }
        }
    }
}
