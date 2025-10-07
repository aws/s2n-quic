// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use nix::{
    fcntl::{fcntl, FcntlArg, OFlag},
    sys::socket::{
        bind, recvmsg, socket, AddressFamily, ControlMessageOwned, MsgFlags, SockFlag, SockType,
        UnixAddr,
    },
    unistd::{close, unlink},
};
use std::{
    os::{
        fd::AsFd,
        unix::io::{AsRawFd, RawFd},
    },
    path::{Path, PathBuf},
};
use tokio::io::{unix::AsyncFd, Interest};

const BUFFER_SIZE: usize = u16::MAX as usize;

pub struct Receiver {
    async_fd: AsyncFd<RawFd>,
    socket_path: PathBuf,
}

impl Receiver {
    pub fn new(socket_path: &Path) -> Result<Self, std::io::Error> {
        // Remove existing socket file if it exists
        unlink(socket_path)?;

        let socket_owned = socket(
            AddressFamily::Unix,
            SockType::Datagram,
            SockFlag::empty(),
            None,
        )?;
        let socket_fd = socket_owned.as_raw_fd();

        // Set socket to non-blocking mode
        let flags = fcntl(socket_owned.as_fd(), FcntlArg::F_GETFL)?;
        let new_flags = OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK;
        fcntl(socket_owned.as_fd(), FcntlArg::F_SETFL(new_flags))?;

        let unix_addr = UnixAddr::new(socket_path)?;
        bind(socket_fd, &unix_addr)?;

        // Prevent the socket from being closed when socket_owned goes out of scope
        std::mem::forget(socket_owned);

        let async_fd = AsyncFd::new(socket_fd)?;

        Ok(Self {
            async_fd,
            socket_path: socket_path.to_path_buf(),
        })
    }

    pub async fn receive_msg(&self) -> Result<(Vec<u8>, RawFd), std::io::Error> {
        loop {
            // Wait for socket to be readable
            let mut guard = self.async_fd.ready(Interest::READABLE).await?;

            match self.try_receive_nonblocking() {
                Ok(result) => {
                    return Ok(result);
                }
                Err(nix::Error::EAGAIN) => {
                    guard.clear_ready();
                    continue;
                }
                Err(e) => {
                    return Err(std::io::Error::from(e));
                }
            }
        }
    }

    fn try_receive_nonblocking(&self) -> Result<(Vec<u8>, RawFd), nix::Error> {
        let mut buffer = [0u8; BUFFER_SIZE];
        let mut cmsg_buffer = nix::cmsg_space!([RawFd; 1]);
        let mut iov = [std::io::IoSliceMut::new(&mut buffer)];

        let msg = recvmsg::<UnixAddr>(
            self.async_fd.as_raw_fd(),
            &mut iov,
            Some(&mut cmsg_buffer),
            MsgFlags::empty(),
        )?;

        let mut packet_data = Vec::new();
        for iov_slice in msg.iovs() {
            packet_data.extend_from_slice(iov_slice);
        }

        for cmsg in msg.cmsgs()? {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                if let Some(&fd) = fds.first() {
                    return Ok((packet_data, fd));
                }
            }
        }

        Err(nix::Error::EINVAL) // No file descriptor found
    }

    pub fn socket_fd(&self) -> RawFd {
        self.async_fd.as_raw_fd()
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let _ = close(self.async_fd.as_raw_fd());
        let _ = unlink(&self.socket_path);
    }
}
