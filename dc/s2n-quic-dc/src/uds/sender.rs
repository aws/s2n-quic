// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use nix::{
    sys::socket::{
        bind, sendmsg, socket, AddressFamily, ControlMessage, MsgFlags, SockFlag, SockType,
        UnixAddr,
    },
    unistd::{close, unlink},
};
use std::{
    os::unix::io::{AsRawFd, RawFd},
    path::{Path, PathBuf},
};

pub struct Sender {
    socket_fd: RawFd,
    socket_path: PathBuf,
}

impl Sender {
    pub fn new(socket_path: &Path) -> Result<Self, std::io::Error> {
        unlink(socket_path)?;

        let socket_owned = socket(
            AddressFamily::Unix,
            SockType::Datagram,
            SockFlag::empty(),
            None,
        )?;
        let socket_fd = socket_owned.as_raw_fd();

        let unix_addr = UnixAddr::new(socket_path)?;
        bind(socket_fd, &unix_addr)?;

        // Prevent the socket from being closed when socket_owned goes out of scope
        std::mem::forget(socket_owned);

        Ok(Self {
            socket_fd,
            socket_path: socket_path.to_path_buf(),
        })
    }

    pub fn send_msg(
        &self,
        packet: &[u8],
        dest_path: &str,
        fd_to_send: RawFd,
    ) -> Result<(), std::io::Error> {
        let fds = [fd_to_send];
        let cmsg = ControlMessage::ScmRights(&fds);

        let dest_unix_addr = UnixAddr::new(dest_path)?;

        sendmsg::<UnixAddr>(
            self.socket_fd,
            &[std::io::IoSlice::new(packet)],
            &[cmsg],
            MsgFlags::empty(),
            Some(&dest_unix_addr),
        )?;
        Ok(())
    }

    pub fn socket_fd(&self) -> RawFd {
        self.socket_fd
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        let _ = close(self.socket_fd);
        let _ = unlink(&self.socket_path);
    }
}
