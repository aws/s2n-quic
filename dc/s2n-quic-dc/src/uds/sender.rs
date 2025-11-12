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
        let async_fd = Arc::new(AsyncFd::new(OwnedFd::from(socket))?);

        Ok(Self {
            socket_fd: async_fd,
        })
    }

    fn try_send_nonblocking(
        &self,
        packet: &[u8],
        fd_to_send: BorrowedFd,
    ) -> Result<(), std::io::Error> {
        let fds = [fd_to_send.as_raw_fd()];
        let cmsg = ControlMessage::ScmRights(&fds);

        #[cfg(target_os = "linux")]
        let send_flags = MsgFlags::MSG_NOSIGNAL;

        #[cfg(not(target_os = "linux"))]
        let send_flags = MsgFlags::empty();

        sendmsg::<()>(
            self.socket_fd.as_raw_fd(),
            &[std::io::IoSlice::new(packet)],
            &[cmsg],
            send_flags,
            None,
        )?;

        Ok(())
    }
}

pub struct SendMsg {
    sender: Sender,
    packet: Vec<u8>,
    fd_to_send: OwnedFd,
    send_future: Option<Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + Sync>>>,
}

impl SendMsg {
    pub fn new(sender: Sender, packet: &[u8], fd_to_send: OwnedFd) -> SendMsg {
        SendMsg {
            sender,
            packet: packet.to_vec(),
            fd_to_send,
            send_future: None,
        }
    }
}

impl Future for SendMsg {
    type Output = std::io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.send_future.is_none() {
            match this
                .sender
                .try_send_nonblocking(&this.packet, this.fd_to_send.as_fd())
            {
                Ok(_) => return Poll::Ready(Ok(())),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    // Allocate a future on the heap only if initial send returns WouldBlock
                    let sender = this.sender.clone();
                    let packet = this.packet.clone();
                    let fd = this.fd_to_send.try_clone()?;

                    let future = Box::pin(async move {
                        sender
                            .socket_fd
                            .async_io(Interest::WRITABLE, |_| {
                                sender.try_send_nonblocking(&packet, fd.as_fd())
                            })
                            .await
                    });
                    this.send_future = Some(future);
                }
                Err(err) => return Poll::Ready(Err(err)),
            }
        }

        if let Some(ref mut future) = this.send_future {
            future.as_mut().poll(cx)
        } else {
            unreachable!()
        }
    }
}
