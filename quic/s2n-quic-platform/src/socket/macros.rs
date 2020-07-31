#![allow(unused_macros)]

macro_rules! impl_socket {
    ($inner:ty, $builder:ident) => {
        pub struct Socket(pub(crate) $inner);

        impl Socket {
            pub fn bind<A: ::std::net::ToSocketAddrs>(addr: A) -> ::std::io::Result<Self> {
                Self::builder()?.with_address(addr)?.build()
            }

            /// Creates a builder for a socket
            pub fn builder() -> ::std::io::Result<$builder> {
                $builder::new()
            }
        }

        impl From<$inner> for Socket {
            fn from(socket: $inner) -> Self {
                Self(socket)
            }
        }
    };
}

macro_rules! impl_socket_deref {
    ($target:ty, |$self:ident| $deref:expr, |$self_mut:ident| $deref_mut:expr) => {
        impl core::ops::Deref for Socket {
            type Target = $target;

            fn deref(&$self) -> &Self::Target {
                $deref
            }
        }

        impl core::ops::DerefMut for Socket {
            fn deref_mut(&mut $self_mut) -> &mut Self::Target {
                $deref_mut
            }
        }
    };
}

macro_rules! impl_socket2_builder {
    ($name:ident) => {
        pub struct $name {
            pub(crate) socket: ::socket2::Socket,
        }

        impl $name {
            pub fn new() -> ::std::io::Result<Self> {
                use cfg_if::cfg_if;
                use socket2::{Domain, Protocol, Socket, Type};

                let domain = if cfg!(feature = "ipv6") {
                    Domain::ipv6()
                } else {
                    Domain::ipv4()
                };
                let socket_type = Type::dgram();
                let protocol = Some(Protocol::udp());

                cfg_if! {
                    if #[cfg(any(
                        target_os = "android",
                        target_os = "dragonfly",
                        target_os = "freebsd",
                        target_os = "linux",
                        target_os = "netbsd",
                        target_os = "openbsd"
                    ))] {
                        let socket_type = socket_type.non_blocking();
                        let socket = Socket::new(domain, socket_type, protocol)?;
                    } else {
                        let socket = Socket::new(domain, socket_type, protocol)?;
                        socket.set_nonblocking(true)?;
                    }
                };

                #[cfg(feature = "ipv6")]
                socket.set_only_v6(false)?;

                Ok(Self { socket })
            }

            pub fn with_address<A: ::std::net::ToSocketAddrs>(
                mut self,
                addr: A,
            ) -> ::std::io::Result<Self> {
                let socket = &mut self.socket;

                for addr in addr.to_socket_addrs()? {
                    let addr = if cfg!(feature = "ipv6") {
                        // TODO uncomment when https://github.com/awslabs/s2n-quic/pull/63 is merged
                        // use ::s2n_quic_core::inet::SocketAddress;
                        // use ::std::net::SocketAddr;
                        //
                        // let addr: SocketAddress = addr.into();
                        // let addr: SocketAddr = addr.to_v6_mapped().into();
                        addr
                    } else {
                        addr
                    }
                    .into();

                    socket.bind(&addr)?;
                }

                Ok(self)
            }

            pub fn with_address_reuse(mut self) -> ::std::io::Result<Self> {
                let socket = &mut self.socket;
                socket.set_reuse_address(true)?;

                #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos")),))]
                socket.set_reuse_port(true)?;

                Ok(self)
            }

            pub fn with_ttl(mut self, ttl: u32) -> ::std::io::Result<Self> {
                let socket = &mut self.socket;
                socket.set_ttl(ttl)?;
                Ok(self)
            }

            pub fn with_recv_buffer_size(mut self, size: usize) -> ::std::io::Result<Self> {
                let socket = &mut self.socket;
                socket.set_recv_buffer_size(size)?;
                Ok(self)
            }

            pub fn with_send_buffer_size(mut self, size: usize) -> ::std::io::Result<Self> {
                let socket = &mut self.socket;
                socket.set_send_buffer_size(size)?;
                Ok(self)
            }
        }
    };
}

macro_rules! impl_socket_raw_delegate {
    (impl[$($gen:tt)*] $impl:ty, |$self:ident| $field:expr) => {
        #[cfg(all(target_family = "unix", feature = "std"))]
        impl<$($gen)*> ::std::os::unix::io::AsRawFd for $impl {
            fn as_raw_fd(&$self) -> $crate::socket::raw::Socket {
                $crate::socket::raw::AsRaw::as_raw($field)
            }
        }
    };
}

macro_rules! impl_socket_mio_delegate {
    (impl[$($gen:tt)*] $impl:ty, |$self:ident| $field:expr ) => {
        #[cfg(feature = "mio")]
        impl<$($gen)*> ::mio::Evented for $impl {
            fn register(
                &$self,
                poll: &::mio::Poll,
                token: ::mio::Token,
                interest: ::mio::Ready,
                opts: ::mio::PollOpt,
            ) -> ::std::io::Result<()> {
                ::mio::Evented::register($field, poll, token, interest, opts)
            }

            fn reregister(
                &$self,
                poll: &::mio::Poll,
                token: ::mio::Token,
                interest: ::mio::Ready,
                opts: ::mio::PollOpt,
            ) -> ::std::io::Result<()> {
                ::mio::Evented::reregister($field, poll, token, interest, opts)
            }

            fn deregister(&$self, poll: &mio::Poll) -> ::std::io::Result<()> {
                ::mio::Evented::deregister($field, poll)
            }
        }
    };
}
