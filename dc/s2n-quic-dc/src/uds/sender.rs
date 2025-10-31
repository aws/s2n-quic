// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
use std::{
    os::{
        fd::{BorrowedFd, OwnedFd},
        unix::{io::AsRawFd as _, net::UnixDatagram},
    },
    path::Path,
};
use tokio::io::{unix::AsyncFd, Interest};

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

    pub async fn send_msg(
        &self,
        packet: &[u8],
        fd_to_send: BorrowedFd<'_>,
    ) -> Result<(), std::io::Error> {
        self.socket_fd
            .async_io(Interest::WRITABLE, |_inner| {
                self.try_send_nonblocking(packet, fd_to_send)
            })
            .await?;
        Ok(())
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
