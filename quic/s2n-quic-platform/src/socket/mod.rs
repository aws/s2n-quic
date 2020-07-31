#[macro_use]
mod macros;

#[cfg(feature = "std")]
pub mod std;

#[cfg(all(feature = "std", feature = "mio"))]
pub mod mio;

#[cfg(all(feature = "std", feature = "tokio"))]
pub mod tokio;

use s2n_quic_core::inet::SocketAddress;

pub trait Simple {
    type Error;

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddress>), Self::Error>;

    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> Result<usize, Self::Error>;
}

pub mod raw {
    use cfg_if::cfg_if;

    cfg_if! {
        if #[cfg(all(target_family = "unix", feature = "std"))] {
            pub type Socket = ::std::os::unix::io::RawFd;

            pub trait AsRaw {
                fn as_raw(&self) -> Socket;
            }

            impl<T: ::std::os::unix::io::AsRawFd> AsRaw for T {
                fn as_raw(&self) -> Socket {
                    self.as_raw_fd()
                }
            }
        } else {
            pub type Socket = *const u8;

            pub trait AsRaw {
                fn as_raw(&self) -> Socket;
            }

            /// make the socket contraints easier to deal with by stubbing out an implementation
            impl<T> AsRaw for T {
                fn as_raw(&self) -> Socket {
                    panic!("raw sockets are not supported on this platform");
                }
            }
        }
    }
}

pub mod default {
    use cfg_if::cfg_if;

    cfg_if! {
        if #[cfg(all(feature = "std", feature = "tokio"))] {
            pub use super::tokio::*;
        } else if #[cfg(all(feature = "std", feature = "mio"))] {
            pub use super::mio::*;
        } else if #[cfg(feature = "std")] {
            pub use super::std::*;
        }
    }
}
