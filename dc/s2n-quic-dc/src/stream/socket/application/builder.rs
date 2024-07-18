// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::socket::ArcApplication;
use std::io;

pub trait Builder: 'static + Send + Sync {
    fn build(self: Box<Self>) -> io::Result<ArcApplication>;
}

#[cfg(feature = "tokio")]
mod tokio_impl {
    use super::*;
    use crate::stream::socket::{application, Tracing};
    use std::{io, sync::Arc};
    use tokio::io::unix::AsyncFd;

    impl Builder for std::net::UdpSocket {
        #[inline]
        fn build(self: Box<Self>) -> io::Result<ArcApplication> {
            let v = AsyncFd::new(*self)?;
            let v = Tracing(v);
            let v = application::Single(v);
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
            let v = application::Single(v);
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
            let v = application::Pair { read, write };
            let v = Arc::new(v);
            Ok(v)
        }
    }

    impl Builder for std::net::TcpStream {
        #[inline]
        fn build(self: Box<Self>) -> io::Result<ArcApplication> {
            let v = tokio::net::TcpStream::from_std(*self)?;
            let v = Tracing(v);
            let v = application::Single(v);
            let v = Arc::new(v);
            Ok(v)
        }
    }

    impl Builder for tokio::net::TcpStream {
        #[inline]
        fn build(self: Box<Self>) -> io::Result<ArcApplication> {
            let v = Tracing(*self);
            let v = application::Single(v);
            let v = Arc::new(v);
            Ok(v)
        }
    }
}

#[cfg(feature = "tokio")]
pub use tokio_impl::*;
