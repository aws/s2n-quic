use std::{io::Result as IOResult, os::unix::io::AsRawFd};

pub trait UdpSocketExt: AsRawFd {
    fn enable_nonblocking(&self) -> IOResult<()>;
    fn reset_nonblocking(&self) -> IOResult<()>;
}

#[cfg(feature = "mio")]
mod socket_impl {
    use super::*;
    pub use mio::{event::Evented, net::UdpSocket, PollOpt, Ready, Token};

    impl UdpSocketExt for mio::net::UdpSocket {
        fn enable_nonblocking(&self) -> IOResult<()> {
            // noop: nonblocking is always enabled with mio
            Ok(())
        }

        fn reset_nonblocking(&self) -> IOResult<()> {
            // noop: nonblocking is always enabled with mio
            Ok(())
        }
    }
}

#[cfg(not(feature = "mio"))]
mod socket_impl {
    use super::*;
    use core::ops::{Deref, DerefMut};
    use std::{io::Result as IOResult, net::UdpSocket as StdSocket, os::unix::io::AsRawFd};

    #[derive(Debug)]
    pub struct UdpSocket {
        inner: StdSocket,
        nonblocking: bool,
    }

    impl UdpSocket {
        pub fn from_socket(inner: StdSocket) -> IOResult<Self> {
            Ok(UdpSocket {
                inner,
                nonblocking: false,
            })
        }

        pub fn set_nonblocking(&mut self, nonblocking: bool) -> IOResult<()> {
            self.nonblocking = nonblocking;
            self.inner.set_nonblocking(nonblocking)
        }
    }

    impl Deref for UdpSocket {
        type Target = StdSocket;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    impl DerefMut for UdpSocket {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner
        }
    }

    impl AsRawFd for UdpSocket {
        fn as_raw_fd(&self) -> i32 {
            self.inner.as_raw_fd()
        }
    }

    impl UdpSocketExt for UdpSocket {
        fn enable_nonblocking(&self) -> IOResult<()> {
            // Only call `set_nonblocking` if we're in blocking mode
            if !self.nonblocking {
                self.inner.set_nonblocking(true)
            } else {
                Ok(())
            }
        }

        fn reset_nonblocking(&self) -> IOResult<()> {
            // Only call `set_nonblocking` if we're in blocking mode
            if !self.nonblocking {
                self.inner.set_nonblocking(false)
            } else {
                Ok(())
            }
        }
    }
}

pub use socket_impl::*;
