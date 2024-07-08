// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::super::{ArcApplication, Tracing};
use std::{io, sync::Arc};
use tokio::io::unix::AsyncFd;

pub trait Builder: 'static + Send + Sync {
    fn build(self: Box<Self>) -> io::Result<ArcApplication>;
}

impl Builder for std::net::UdpSocket {
    #[inline]
    fn build(self: Box<Self>) -> io::Result<ArcApplication> {
        let v = AsyncFd::new(*self)?;
        let v = Tracing(v);
        let v = super::Single(v);
        let v = Arc::new(v);
        Ok(v)
    }
}

impl Builder for std::net::TcpStream {
    #[inline]
    fn build(self: Box<Self>) -> io::Result<ArcApplication> {
        let v = tokio::net::TcpStream::from_std(*self)?;
        let v = Tracing(v);
        let v = super::Single(v);
        let v = Arc::new(v);
        Ok(v)
    }
}

impl Builder for tokio::net::TcpStream {
    #[inline]
    fn build(self: Box<Self>) -> io::Result<ArcApplication> {
        let v = Tracing(*self);
        let v = super::Single(v);
        let v = Arc::new(v);
        Ok(v)
    }
}

impl Builder for Arc<std::net::UdpSocket> {
    #[inline]
    fn build(self: Box<Self>) -> io::Result<ArcApplication> {
        // TODO avoid the Box<Arc<...>> indirection here?
        let v = AsyncFd::new(*self)?;
        let v = Tracing(v);
        let v = super::Single(v);
        let v = Arc::new(v);
        Ok(v)
    }
}

pub struct UdpPair {
    pub reader: Arc<std::net::UdpSocket>,
    pub writer: Arc<std::net::UdpSocket>,
}

impl Builder for UdpPair {
    #[inline]
    fn build(self: Box<Self>) -> io::Result<ArcApplication> {
        let read = AsyncFd::new(self.reader)?;
        let read = Tracing(read);
        let write = AsyncFd::new(self.writer)?;
        let write = Tracing(write);
        let v = super::Pair { read, write };
        let v = Arc::new(v);
        Ok(v)
    }
}
