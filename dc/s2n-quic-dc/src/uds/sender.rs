// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use nix::sys::socket::{
    sendmsg, socket, AddressFamily, ControlMessage, MsgFlags, SockFlag, SockType, UnixAddr,
};
use std::{
    os::{
        fd::{BorrowedFd, OwnedFd},
        unix::io::AsRawFd as _,
    },
    path::Path,
};
use tokio::io::{unix::AsyncFd, Interest};

pub struct Sender {
    socket_fd: AsyncFd<OwnedFd>,
}

impl Sender {
    pub fn new() -> Result<Self, std::io::Error> {
        let socket_owned = socket(
            AddressFamily::Unix,
            SockType::Datagram,
            SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
            None,
        )?;

        let async_fd = AsyncFd::new(socket_owned)?;

        Ok(Self {
            socket_fd: async_fd,
        })
    }

    pub async fn send_msg(
        &self,
        packet: &[u8],
        dest_path: &Path,
        fd_to_send: BorrowedFd<'_>,
    ) -> Result<(), std::io::Error> {
        loop {
            let mut guard = self.socket_fd.ready(Interest::WRITABLE).await?;

            match self.try_send_nonblocking(packet, dest_path, fd_to_send) {
                Ok(()) => {
                    return Ok(());
                }
                Err(nix::Error::EAGAIN) => {
                    guard.clear_ready();
                    continue;
                }
                Err(e) => {
                    let err = Err(std::io::Error::from(e));
                    println!("{:?}", err);
                    return err;
                }
            }
        }
    }

    fn try_send_nonblocking(
        &self,
        packet: &[u8],
        dest_path: &Path,
        fd_to_send: BorrowedFd,
    ) -> Result<(), nix::Error> {
        let fds = [fd_to_send.as_raw_fd()];
        let cmsg = ControlMessage::ScmRights(&fds);

        let dest_unix_addr = UnixAddr::new(dest_path)?;

        sendmsg::<UnixAddr>(
            self.socket_fd.as_raw_fd(),
            &[std::io::IoSlice::new(packet)],
            &[cmsg],
            MsgFlags::MSG_NOSIGNAL,
            Some(&dest_unix_addr),
        )?;

        Ok(())
    }
}
