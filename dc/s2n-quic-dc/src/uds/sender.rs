// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
use std::{
    future::Future,
    os::{
        fd::{AsFd, BorrowedFd, OwnedFd},
        unix::{io::AsRawFd as _, net::UnixDatagram},
    },
    path::Path,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{unix::AsyncFd, Interest};

#[derive(Clone)]
pub struct Sender {
    socket_fd: Arc<AsyncFd<OwnedFd>>,
}

impl Sender {
    pub fn new(connect_path: &Path) -> Result<Self, std::io::Error> {
        let socket = UnixDatagram::unbound()?;
        socket.set_nonblocking(true)?;
        socket.connect(connect_path)?; // without this the socket is always writable
        let async_fd = Arc::new(unsafe {
            // SAFETY: the OwnedFd is owned by the AsyncFd, so its fd is stable until drop.
            AsyncFd::register(OwnedFd::from(socket))
        }?);

        Ok(Self {
            socket_fd: async_fd,
        })
    }

    /// Enqueues a packet without a file descriptor, returning `WouldBlock` if full.
    pub fn try_send(&self, packet: &[u8]) -> Result<(), std::io::Error> {
        self.try_send_nonblocking(packet, None)
    }

    /// Waits for writable readiness to enqueue a packet without a file descriptor.
    pub async fn send(&self, packet: &[u8]) -> Result<(), std::io::Error> {
        self.socket_fd
            .async_io(Interest::WRITABLE, |_| self.try_send(packet))
            .await
    }

    fn try_send_nonblocking(
        &self,
        packet: &[u8],
        fd_to_send: Option<BorrowedFd>,
    ) -> Result<(), std::io::Error> {
        let fds = fd_to_send.map(|fd| [fd.as_raw_fd()]);
        let cmsg = fds.as_ref().map(|fds| ControlMessage::ScmRights(fds));

        #[cfg(target_os = "linux")]
        let send_flags = MsgFlags::MSG_NOSIGNAL;

        #[cfg(not(target_os = "linux"))]
        let send_flags = MsgFlags::empty();

        sendmsg::<()>(
            self.socket_fd.as_raw_fd(),
            &[std::io::IoSlice::new(packet)],
            cmsg.as_slice(),
            send_flags,
            None,
        )?;

        Ok(())
    }
}

pub enum SendMsg {
    Initial {
        sender: Sender,
        packet: Vec<u8>,
        fd_to_send: Option<OwnedFd>,
    },
    Blocked {
        send_future: Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + Sync>>,
    },
    Temporary,
}

impl SendMsg {
    pub fn new(sender: Sender, packet: Vec<u8>, fd_to_send: OwnedFd) -> SendMsg {
        Self::Initial {
            sender,
            packet,
            fd_to_send: Some(fd_to_send),
        }
    }

    /// Sends `packet` without passing a file descriptor.
    pub fn without_fd(sender: Sender, packet: Vec<u8>) -> SendMsg {
        Self::Initial {
            sender,
            packet,
            fd_to_send: None,
        }
    }
}

impl Future for SendMsg {
    type Output = std::io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        match std::mem::replace(this, Self::Temporary) {
            Self::Initial {
                sender,
                packet,
                fd_to_send,
            } => match sender.try_send_nonblocking(&packet, fd_to_send.as_ref().map(AsFd::as_fd)) {
                Ok(_) => Poll::Ready(Ok(())),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    let mut future = Box::pin(async move {
                        sender
                            .socket_fd
                            .async_io(Interest::WRITABLE, |_| {
                                sender.try_send_nonblocking(
                                    &packet,
                                    fd_to_send.as_ref().map(AsFd::as_fd),
                                )
                            })
                            .await
                    });
                    match future.as_mut().poll(cx) {
                        Poll::Ready(result) => Poll::Ready(result),
                        Poll::Pending => {
                            *this = Self::Blocked {
                                send_future: future,
                            };
                            Poll::Pending
                        }
                    }
                }
                Err(err) => Poll::Ready(Err(err)),
            },
            Self::Blocked { mut send_future } => match send_future.as_mut().poll(cx) {
                Poll::Ready(result) => Poll::Ready(result),
                Poll::Pending => {
                    *this = Self::Blocked { send_future };
                    Poll::Pending
                }
            },
            Self::Temporary => unreachable!(),
        }
    }
}
