// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[derive(Debug, Default)]
pub struct Builder {
    pub(super) handle: Option<Handle>,
    pub(super) rx_socket: Option<socket2::Socket>,
    pub(super) tx_socket: Option<socket2::Socket>,
    pub(super) recv_addr: Option<std::net::SocketAddr>,
    pub(super) send_addr: Option<std::net::SocketAddr>,
    pub(super) socket_recv_buffer_size: Option<usize>,
    pub(super) socket_send_buffer_size: Option<usize>,
    pub(super) queue_recv_buffer_size: Option<u32>,
    pub(super) queue_send_buffer_size: Option<u32>,
    pub(super) max_mtu: MaxMtu,
    pub(super) max_segments: gso::MaxSegments,
    pub(super) gro_enabled: Option<bool>,
    pub(super) reuse_address: bool,
    pub(super) reuse_port: bool,
}

impl Builder {
    #[must_use]
    pub fn with_handle(mut self, handle: Handle) -> Self {
        self.handle = Some(handle);
        self
    }

    /// Sets the local address for the runtime to listen on. If no send address
    /// or tx socket is specified, this address will also be used for transmitting from.
    ///
    /// NOTE: this method is mutually exclusive with `with_rx_socket`
    pub fn with_receive_address(mut self, addr: std::net::SocketAddr) -> io::Result<Self> {
        debug_assert!(self.rx_socket.is_none(), "rx socket has already been set");
        self.recv_addr = Some(addr);
        Ok(self)
    }

    /// Sets the local address for the runtime to transmit from. If no send address
    /// or tx socket is specified, the receive_address will be used for transmitting.
    ///
    /// NOTE: this method is mutually exclusive with `with_tx_socket`
    pub fn with_send_address(mut self, addr: std::net::SocketAddr) -> io::Result<Self> {
        debug_assert!(self.tx_socket.is_none(), "tx socket has already been set");
        self.send_addr = Some(addr);
        Ok(self)
    }

    /// Sets the socket used for receiving for the runtime. If no tx_socket or send address is
    /// specified, this socket will be used for transmitting.
    ///
    /// NOTE: this method is mutually exclusive with `with_receive_address`
    pub fn with_rx_socket(mut self, socket: std::net::UdpSocket) -> io::Result<Self> {
        debug_assert!(
            self.recv_addr.is_none(),
            "recv address has already been set"
        );
        self.rx_socket = Some(socket.into());
        Ok(self)
    }

    /// Sets the socket used for transmitting on for the runtime. If no tx_socket or send address is
    /// specified, the rx_socket will be used for transmitting.
    ///
    /// NOTE: this method is mutually exclusive with `with_send_address`
    pub fn with_tx_socket(mut self, socket: std::net::UdpSocket) -> io::Result<Self> {
        debug_assert!(
            self.send_addr.is_none(),
            "send address has already been set"
        );
        self.tx_socket = Some(socket.into());
        Ok(self)
    }

    /// Sets the size of the operating system’s send buffer associated with the tx socket
    pub fn with_send_buffer_size(mut self, send_buffer_size: usize) -> io::Result<Self> {
        self.socket_send_buffer_size = Some(send_buffer_size);
        Ok(self)
    }

    /// Sets the size of the operating system’s receive buffer associated with the rx socket
    pub fn with_recv_buffer_size(mut self, recv_buffer_size: usize) -> io::Result<Self> {
        self.socket_recv_buffer_size = Some(recv_buffer_size);
        Ok(self)
    }

    /// Sets the size of the send buffer associated with the transmit side (internal to s2n-quic)
    pub fn with_internal_send_buffer_size(mut self, send_buffer_size: usize) -> io::Result<Self> {
        self.queue_send_buffer_size = Some(
            send_buffer_size
                .try_into()
                .map_err(|err| io::Error::new(ErrorKind::InvalidInput, format!("{err}")))?,
        );
        Ok(self)
    }

    /// Sets the size of the send buffer associated with the receive side (internal to s2n-quic)
    pub fn with_internal_recv_buffer_size(mut self, recv_buffer_size: usize) -> io::Result<Self> {
        self.queue_recv_buffer_size = Some(
            recv_buffer_size
                .try_into()
                .map_err(|err| io::Error::new(ErrorKind::InvalidInput, format!("{err}")))?,
        );
        Ok(self)
    }

    /// Sets the largest maximum transmission unit (MTU) that can be sent on a path
    pub fn with_max_mtu(mut self, max_mtu: u16) -> io::Result<Self> {
        self.max_mtu = max_mtu
            .try_into()
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, format!("{err}")))?;
        Ok(self)
    }

    /// Disables Generic Segmentation Offload (GSO)
    ///
    /// By default, GSO will be used unless the platform does not support it or an attempt to use
    /// GSO fails. If it is known that GSO is not available, set this option to explicitly disable it.
    pub fn with_gso_disabled(mut self) -> io::Result<Self> {
        self.max_segments = 1.try_into().expect("1 is always a valid MaxSegments value");
        Ok(self)
    }

    /// Configures Generic Segmentation Offload (GSO)
    ///
    /// By default, GSO will be used unless the platform does not support it or an attempt to use
    /// GSO fails. If it is known that GSO is not available, set this option to explicitly disable it.
    pub fn with_gso(self, enabled: bool) -> io::Result<Self> {
        if enabled {
            Ok(self)
        } else {
            self.with_gso_disabled()
        }
    }

    /// Disables Generic Receive Offload (GRO)
    ///
    /// By default, GRO will be used unless the platform does not support it. If it is known that
    /// GRO is not available, set this option to explicitly disable it.
    pub fn with_gro_disabled(mut self) -> io::Result<Self> {
        self.gro_enabled = Some(false);
        Ok(self)
    }

    /// Configures Generic Receive Offload (GRO)
    ///
    /// By default, GRO will be used unless the platform does not support it. If it is known that
    /// GRO is not available, set this option to explicitly disable it.
    pub fn with_gro(self, enabled: bool) -> io::Result<Self> {
        if enabled {
            Ok(self)
        } else {
            self.with_gro_disabled()
        }
    }

    /// Enables the address reuse (SO_REUSEADDR) socket option
    pub fn with_reuse_address(mut self, enabled: bool) -> io::Result<Self> {
        self.reuse_address = enabled;
        Ok(self)
    }

    /// Enables the port reuse (SO_REUSEPORT) socket option
    pub fn with_reuse_port(mut self) -> io::Result<Self> {
        if !cfg!(unix) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reuse_port is not supported on the current platform",
            ));
        }
        self.reuse_port = true;
        Ok(self)
    }

    pub fn build(self) -> io::Result<Io> {
        Ok(Io { builder: self })
    }
}
