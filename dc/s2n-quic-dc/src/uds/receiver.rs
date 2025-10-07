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
        fd::{AsFd, OwnedFd},
        unix::io::{AsRawFd, RawFd},
    },
    path::{Path, PathBuf},
};
use tokio::io::{unix::AsyncFd, Interest};

const BUFFER_SIZE: usize = u16::MAX as usize;

pub struct Receiver {
    async_fd: AsyncFd<OwnedFd>,
    socket_path: PathBuf,
}

impl Receiver {
    pub fn new(socket_path: &Path) -> Result<Self, std::io::Error> {
        let _ = unlink(socket_path);

        let socket_owned = socket(
            AddressFamily::Unix,
            SockType::Datagram,
            SockFlag::empty(),
            None,
        )?;

        let flags = fcntl(socket_owned.as_fd(), FcntlArg::F_GETFL)?;
        let new_flags = OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK;
        fcntl(socket_owned.as_fd(), FcntlArg::F_SETFL(new_flags))?;

        let socket_fd = socket_owned.as_raw_fd();
        let unix_addr = UnixAddr::new(socket_path)?;
        bind(socket_fd, &unix_addr)?;

        if !socket_path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Socket file not created after bind",
            ));
        }

        let async_fd = AsyncFd::new(socket_owned)?;

        Ok(Self {
            async_fd,
            socket_path: socket_path.to_path_buf(),
        })
    }

    pub async fn receive_msg(&self) -> Result<(Vec<u8>, RawFd), std::io::Error> {
        loop {
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
                    for &extra_fd in fds.iter().skip(1) {
                        let _ = close(extra_fd);
                    }
                    return Ok((packet_data, fd));
                }
            }
        }

        Err(nix::Error::EINVAL) // No file descriptor found
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let _ = unlink(&self.socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uds::sender::Sender;
    use std::{
        io::Read as _,
        os::{fd::FromRawFd as _, unix::io::AsRawFd},
        path::Path,
    };
    use tokio::{
        fs::File,
        io::AsyncWriteExt,
        time::{timeout, Duration},
    };

    #[tokio::test]
    async fn test_send_receive() {
        let receiver_path = Path::new("/tmp/receiver.sock");

        let receiver = Receiver::new(receiver_path).unwrap();
        let sender = Sender::new().unwrap();

        let file_path = "/tmp/test.txt";
        let mut file = File::create(file_path).await.unwrap();
        let test_data = b"Hello, world!";
        file.write_all(test_data).await.unwrap();
        file.sync_all().await.unwrap();

        let file = std::fs::File::open(file_path).unwrap();
        let fd_to_send = file.as_raw_fd();

        let packet_data = b"test packet data";

        let result = tokio::try_join!(
            async {
                timeout(Duration::from_secs(5), receiver.receive_msg())
                    .await
                    .unwrap()
            },
            sender.send_msg(packet_data, receiver_path, fd_to_send)
        );

        match result {
            Ok(((received_data, received_fd), ())) => {
                assert_eq!(received_data, packet_data);
                assert!(received_fd > 0);
                let mut received_file = unsafe { std::fs::File::from_raw_fd(received_fd) };
                let mut read_buffer = Vec::new();
                received_file.read_to_end(&mut read_buffer).unwrap();
                assert_eq!(read_buffer, test_data);
            }
            Err(e) => {
                panic!("Error: {}", e);
            }
        }

        tokio::fs::remove_file(file_path).await.unwrap();
    }
}
