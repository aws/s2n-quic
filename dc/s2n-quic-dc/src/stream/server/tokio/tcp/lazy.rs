// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    msg,
    stream::socket::{fd::tcp, Flags, Socket},
};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    io::{self, ErrorKind, Write},
    net::TcpStream as StdTcpStream,
    os::fd::AsRawFd,
    pin::Pin,
    task::Poll,
    time::Duration,
};
use tokio::{io::AsyncWrite as _, net::TcpStream as TokioTcpStream};

pub enum LazyBoundStream {
    Tokio(TokioTcpStream),
    Std(StdTcpStream),
    // needed for moving between the previous two while only having &mut access.
    TempEmpty,
}

impl LazyBoundStream {
    pub fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        match self {
            LazyBoundStream::Tokio(s) => s.set_nodelay(nodelay),
            LazyBoundStream::Std(s) => s.set_nodelay(nodelay),
            LazyBoundStream::TempEmpty => unreachable!(),
        }
    }

    pub fn set_linger(&self, linger: Option<Duration>) -> io::Result<()> {
        match self {
            LazyBoundStream::Tokio(s) => s.set_linger(linger),
            LazyBoundStream::Std(s) => {
                // Once it stabilizes we can switch to the std function
                // https://github.com/rust-lang/rust/issues/88494
                let res = unsafe {
                    libc::setsockopt(
                        s.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_LINGER,
                        &libc::linger {
                            l_onoff: linger.is_some() as libc::c_int,
                            l_linger: linger.unwrap_or_default().as_secs() as libc::c_int,
                        } as *const _ as *const _,
                        std::mem::size_of::<libc::linger>() as libc::socklen_t,
                    )
                };
                if res != 0 {
                    return Err(std::io::Error::last_os_error());
                }

                Ok(())
            }
            LazyBoundStream::TempEmpty => unreachable!(),
        }
    }

    pub fn into_std(self) -> io::Result<StdTcpStream> {
        match self {
            LazyBoundStream::Tokio(s) => s.into_std(),
            LazyBoundStream::Std(s) => Ok(s),
            LazyBoundStream::TempEmpty => unreachable!(),
        }
    }

    pub fn poll_write(
        &mut self,
        cx: &mut std::task::Context,
        buffer: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        loop {
            match self {
                LazyBoundStream::Tokio(stream) => return Pin::new(stream).poll_write(cx, buffer),
                LazyBoundStream::Std(stream) => match stream.write(buffer) {
                    Ok(v) => return Poll::Ready(Ok(v)),
                    Err(e) => {
                        if e.kind() == ErrorKind::WouldBlock {
                            let LazyBoundStream::Std(stream) =
                                std::mem::replace(self, LazyBoundStream::TempEmpty)
                            else {
                                unreachable!();
                            };
                            *self = LazyBoundStream::Tokio(TokioTcpStream::from_std(stream)?);
                        } else {
                            return Poll::Ready(Err(e));
                        }
                    }
                },
                LazyBoundStream::TempEmpty => unreachable!(),
            }
        }
    }

    pub fn poll_recv_buffer(
        &mut self,
        cx: &mut std::task::Context,
        buffer: &mut msg::recv::Message,
    ) -> std::task::Poll<io::Result<usize>> {
        loop {
            match self {
                LazyBoundStream::Tokio(stream) => {
                    return Pin::new(stream).poll_recv_buffer(cx, buffer)
                }
                LazyBoundStream::Std(stream) => {
                    let res = buffer.recv_with(|_addr, cmsg, buffer| {
                        loop {
                            let flags = Flags::default();
                            let res = tcp::recv(&*stream, buffer, flags);

                            match res {
                                Ok(len) => {
                                    // we don't need ECN markings from TCP since it handles that logic for us
                                    cmsg.set_ecn(ExplicitCongestionNotification::NotEct);

                                    // TCP doesn't have segments so just set it to 0 (which will indicate a single
                                    // stream of bytes)
                                    cmsg.set_segment_len(0);

                                    return Ok(len);
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                                    // try the operation again if we were interrupted
                                    continue;
                                }
                                Err(err) => return Err(err),
                            }
                        }
                    });
                    match res {
                        Ok(v) => return Poll::Ready(Ok(v)),
                        Err(e) => {
                            if e.kind() == ErrorKind::WouldBlock {
                                let LazyBoundStream::Std(stream) =
                                    std::mem::replace(self, LazyBoundStream::TempEmpty)
                                else {
                                    unreachable!();
                                };
                                *self = LazyBoundStream::Tokio(TokioTcpStream::from_std(stream)?);
                            } else {
                                return Poll::Ready(Err(e));
                            }
                        }
                    }
                }
                LazyBoundStream::TempEmpty => unreachable!(),
            }
        }
    }
}
