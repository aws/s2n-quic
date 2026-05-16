// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::socket::fd::udp;
use std::{
    io,
    net::SocketAddr,
    os::fd::{AsRawFd, RawFd},
};

pub struct BusyPoll<T>(pub T);

impl<T: udp::Socket> udp::Socket for BusyPoll<T> {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }
}

impl<T: udp::Socket> AsRawFd for BusyPoll<T> {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}
