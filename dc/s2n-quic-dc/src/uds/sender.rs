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
    task::{ready, Context, Poll},
};
use tokio::io::unix::AsyncFd;

pub struct Sender {
    socket_fd: AsyncFd<OwnedFd>,
}

impl Sender {
    pub fn new(connect_path: &Path) -> Result<Self, std::io::Error> {
        let socket = UnixDatagram::unbound()?;
        socket.set_nonblocking(true)?;
        socket.connect(connect_path)?; // without this the socket is always writable

        let async_fd = AsyncFd::new(OwnedFd::from(socket))?;

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
    fd: OwnedFd,
}

impl SendMsg {
    pub fn new(sender: Sender, packet: &[u8], fd_to_send: OwnedFd) -> SendMsg {
        SendMsg {
            sender,
            packet: packet.to_vec(),
            fd: fd_to_send,
        }
    }
}

impl Future for SendMsg {
    type Output = Result<(), std::io::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        loop {
            let mut guard = ready!(this.sender.socket_fd.poll_write_ready(cx))?;

            match guard.try_io(|_inner| {
                this.sender
                    .try_send_nonblocking(&this.packet, this.fd.as_fd())
            }) {
                Ok(result) => return Poll::Ready(result),
                Err(_would_block) => continue,
            }
        }
    }
}
