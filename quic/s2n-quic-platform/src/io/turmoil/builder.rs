// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[derive(Default)]
pub struct Builder {
    pub(super) handle: Option<Handle>,
    pub(super) socket: Option<UdpSocket>,
    pub(super) addr: Option<Box<dyn turmoil::ToSocketAddrs + Send + Sync + 'static>>,
    pub(super) mtu_config_builder: mtu::Builder,
}

impl Builder {
    #[must_use]
    pub fn with_handle(mut self, handle: Handle) -> Self {
        self.handle = Some(handle);
        self
    }

    /// Sets the local address for the runtime to listen on.
    ///
    /// NOTE: this method is mutually exclusive with `with_socket`
    pub fn with_address<A: turmoil::ToSocketAddrs + Send + Sync + 'static>(
        mut self,
        addr: A,
    ) -> io::Result<Self> {
        debug_assert!(self.socket.is_none(), "socket has already been set");
        self.addr = Some(Box::new(addr));
        Ok(self)
    }

    /// Sets the socket used for sending and receiving for the runtime.
    ///
    /// NOTE: this method is mutually exclusive with `with_address`
    pub fn with_socket(mut self, socket: UdpSocket) -> io::Result<Self> {
        debug_assert!(self.addr.is_none(), "address has already been set");
        self.socket = Some(socket);
        Ok(self)
    }

    /// Sets the largest maximum transmission unit (MTU) that can be sent on a path
    pub fn with_max_mtu(mut self, max_mtu: u16) -> io::Result<Self> {
        self.mtu_config_builder = self
            .mtu_config_builder
            .with_max_mtu(max_mtu)
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, format!("{err}")))?;
        Ok(self)
    }

    pub fn build(self) -> io::Result<Io> {
        Ok(Io { builder: self })
    }
}
