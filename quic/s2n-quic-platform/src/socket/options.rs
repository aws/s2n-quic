// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::syscall;
use s2n_quic_core::inet::SocketAddress;
use std::{
    ffi::CString,
    io,
    net::{SocketAddr, TcpListener, UdpSocket},
};

#[derive(Clone, Copy, Debug, Default)]
pub enum ReusePort {
    #[default]
    Disabled,
    /// Enables reuse port before binding the socket
    ///
    /// NOTE: the provided `addr` must not be bound to a random port (`0`)
    BeforeBind,
    /// Enables reuse port after binding the socket
    AfterBind,
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Options {
    pub addr: SocketAddr,
    pub bind_interface: Option<CString>,
    pub reuse_address: bool,
    pub reuse_port: ReusePort,
    pub gro: bool,
    pub blocking: bool,
    pub delay: bool,
    pub send_buffer: Option<usize>,
    pub recv_buffer: Option<usize>,
    pub backlog: usize,
    pub only_v6: bool,
}

impl Default for Options {
    #[inline]
    fn default() -> Self {
        Self {
            addr: SocketAddress::default().into(),
            bind_interface: None,
            reuse_address: false,
            reuse_port: Default::default(),
            gro: true,
            blocking: false,
            send_buffer: None,
            recv_buffer: None,
            delay: false,
            backlog: 4096,
            only_v6: false,
        }
    }
}

impl Options {
    #[inline]
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            ..Default::default()
        }
    }

    #[inline]
    pub fn build_udp(&self) -> io::Result<UdpSocket> {
        let socket = syscall::udp_socket(self.addr, self.only_v6)?;

        if self.gro {
            let _ = syscall::configure_gro(&socket);
        }

        let _ = syscall::configure_tos(&socket);
        let _ = syscall::configure_mtu_disc(&socket);

        self.build_common(&socket)?;

        let socket = socket.into();
        Ok(socket)
    }

    #[inline]
    pub fn build_tcp_listener(&self) -> io::Result<TcpListener> {
        let domain = socket2::Domain::for_address(self.addr);
        let ty = socket2::Type::STREAM;
        let protocol = socket2::Protocol::TCP;

        let socket = socket2::Socket::new(domain, ty, Some(protocol))?;

        socket.set_tcp_nodelay(!self.delay)?;

        self.build_common(&socket)?;

        socket.listen(self.backlog.try_into().unwrap_or(core::ffi::c_int::MAX))?;

        Ok(socket.into())
    }

    fn build_common(&self, socket: &socket2::Socket) -> io::Result<()> {
        socket.set_reuse_address(self.reuse_address)?;
        socket.set_nonblocking(!self.blocking)?;

        if let Some(send_buffer) = self.send_buffer {
            let _ = socket.set_send_buffer_size(send_buffer);
        }

        if let Some(recv_buffer) = self.recv_buffer {
            let _ = socket.set_recv_buffer_size(recv_buffer);
        }

        if let ReusePort::BeforeBind = self.reuse_port {
            assert_ne!(self.addr.port(), 0);
            set_reuse_port(socket)?;
        }

        if let Some(interface_name) = &self.bind_interface {
            bind_to_interface(socket, interface_name)?;
        }

        socket.bind(&self.addr.into())?;

        if let ReusePort::AfterBind = self.reuse_port {
            set_reuse_port(socket)?;
        }

        #[cfg(target_os = "linux")]
        fn bind_to_interface(socket: &socket2::Socket, interface_name: &CString) -> io::Result<()> {
            use std::os::fd::AsRawFd;

            let ret = unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_BINDTODEVICE,
                    interface_name.as_ptr() as *const _,
                    interface_name.as_bytes_with_nul().len() as _,
                )
            };

            if ret < 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(())
        }

        #[cfg(not(target_os = "linux"))]
        fn bind_to_interface(
            _socket: &socket2::Socket,
            _interface_name: &CString,
        ) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "binding to a NIC is only supported on Linux",
            ))
        }

        Ok(())
    }
}

#[cfg(windows)]
fn set_reuse_port(_socket: &socket2::Socket) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "reuse port is not supported on windows",
    ))
}

#[cfg(not(windows))]
fn set_reuse_port(socket: &socket2::Socket) -> io::Result<()> {
    socket.set_reuse_port(true)
}
