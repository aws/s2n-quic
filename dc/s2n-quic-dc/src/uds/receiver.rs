// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use nix::{
    sys::socket::{recvmsg, ControlMessageOwned, MsgFlags, UnixAddr},
    unistd::{close, unlink},
};
use std::{
    os::{
        fd::{FromRawFd as _, OwnedFd},
        unix::{
            io::{AsRawFd, RawFd},
            net::UnixDatagram,
        },
    },
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::{unix::AsyncFd, Interest};

const BUFFER_SIZE: usize = u16::MAX as usize;

#[derive(Clone)]
pub struct Receiver {
    socket_fd: Arc<AsyncFd<OwnedFd>>,
    socket_path: PathBuf,
}

impl Receiver {
    pub fn new(socket_path: &Path) -> Result<Self, std::io::Error> {
        let _ = unlink(socket_path); // Required in case drop did not run previously
        let socket = UnixDatagram::bind(socket_path)?;
        socket.set_nonblocking(true)?;

        let async_fd = Arc::new(AsyncFd::new(OwnedFd::from(socket))?);

        Ok(Self {
            socket_fd: async_fd,
            socket_path: socket_path.to_path_buf(),
        })
    }

    pub async fn receive_msg(&self) -> Result<(Vec<u8>, OwnedFd), std::io::Error> {
        let res = self
            .socket_fd
            .async_io(Interest::READABLE, |_inner| self.try_receive_nonblocking())
            .await?;
        Ok(res)
    }

    fn try_receive_nonblocking(&self) -> Result<(Vec<u8>, OwnedFd), std::io::Error> {
        let mut buffer = [0u8; BUFFER_SIZE];
        let mut cmsg_buffer = nix::cmsg_space!([RawFd; 1]);
        let mut iov = [std::io::IoSliceMut::new(&mut buffer)];

        #[cfg(target_os = "linux")]
        let recv_flags = MsgFlags::MSG_CMSG_CLOEXEC;

        #[cfg(not(target_os = "linux"))]
        let recv_flags = MsgFlags::empty();

        let msg = recvmsg::<UnixAddr>(
            self.socket_fd.as_raw_fd(),
            &mut iov,
            Some(&mut cmsg_buffer),
            recv_flags,
        )?;

        let mut packet_data = Vec::new();
        for iov_slice in msg.iovs() {
            packet_data.extend_from_slice(iov_slice);
        }

        for cmsg in msg.cmsgs()? {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                if let Some(&fd) = fds.first() {
                    for &extra_fd in fds.iter().skip(1) {
                        tracing::warn!("Closing extra file descriptors");
                        let _ = close(extra_fd);
                    }
                    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
                    #[cfg(not(target_os = "linux"))]
                    {
                        use nix::fcntl::{fcntl, FcntlArg, FdFlag};
                        fcntl(&fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
                    }
                    return Ok((packet_data, fd));
                }
            }
        }
        Err(std::io::Error::from(nix::Error::EINVAL)) // No file descriptor found
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
    use std::{io::Read as _, os::fd::AsFd as _, path::Path};
    use tokio::{
        fs::File,
        io::AsyncWriteExt,
        time::{timeout, Duration},
    };

    #[tokio::test]
    async fn test_send_receive() {
        let receiver_path = Path::new("/tmp/receiver.sock");

        let receiver = Receiver::new(receiver_path).unwrap();
        let sender = Sender::new(receiver_path).unwrap();

        let file_path = "/tmp/test.txt";
        let mut file = File::create(file_path).await.unwrap();
        let test_data = b"Hello, world!";
        file.write_all(test_data).await.unwrap();
        file.sync_all().await.unwrap();

        let file = std::fs::File::open(file_path).unwrap();
        let fd_to_send = file.as_fd();

        let packet_data = b"test packet data";

        let result = tokio::try_join!(
            async {
                timeout(Duration::from_secs(5), receiver.receive_msg())
                    .await
                    .unwrap()
            },
            sender.send_msg(packet_data, fd_to_send)
        );

        match result {
            Ok(((received_data, received_fd), ())) => {
                assert_eq!(received_data, packet_data);
                let mut received_file = std::fs::File::from(received_fd);
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
