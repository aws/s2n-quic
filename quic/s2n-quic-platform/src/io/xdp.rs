// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::io::tokio::Clock;
use s2n_quic_core::{
    endpoint::Endpoint, inet::SocketAddress, io::event_loop::EventLoop, path::MaxMtu,
};

pub use s2n_quic_core::{
    io::{rx, tx},
    sync::{spsc, worker},
    xdp::path::Tuple as PathHandle,
};
pub use s2n_quic_xdp::*;

// export the encoder configuration for writing packets
pub mod encoder {
    pub use s2n_quic_core::xdp::encoder::State as Config;
}

// export socket types and helpers
pub mod socket {
    pub use s2n_quic_xdp::socket::*;

    /// Binds a UDP socket to a particular interface and socket address
    pub fn bind_udp(
        interface: &::std::ffi::CStr,
        addr: ::std::net::SocketAddr,
    ) -> ::std::io::Result<::std::net::UdpSocket> {
        let socket = crate::syscall::udp_socket(addr)?;

        // associate the socket with a single interface
        crate::syscall::bind_to_interface(&socket, interface)?;

        socket.bind(&addr.into())?;

        Ok(socket.into())
    }
}

impl From<PathHandle> for crate::message::msg::Handle {
    #[inline]
    fn from(handle: PathHandle) -> Self {
        let remote_address = handle.remote_address.into();
        let local_address = handle.local_address.into();
        crate::message::msg::Handle {
            remote_address,
            local_address,
        }
    }
}

impl From<&PathHandle> for crate::message::msg::Handle {
    #[inline]
    fn from(handle: &PathHandle) -> Self {
        let remote_address = handle.remote_address.into();
        let local_address = handle.local_address.into();
        crate::message::msg::Handle {
            remote_address,
            local_address,
        }
    }
}

mod builder;
pub use builder::Builder;

pub struct Provider<Rx, Tx> {
    rx: Rx,
    tx: Tx,
    max_mtu: MaxMtu,
    handle: Option<tokio::runtime::Handle>,
}

impl Provider<(), ()> {
    /// Creates a builder to construct an XDP provider
    pub fn builder() -> Builder {
        Builder::default()
    }
}

impl<Rx, Tx> Provider<Rx, Tx>
where
    Rx: 'static + rx::Rx + Send,
    Tx: 'static + tx::Tx<PathHandle = Rx::PathHandle> + Send,
{
    pub fn start<E: Endpoint<PathHandle = Rx::PathHandle>>(
        self,
        mut endpoint: E,
    ) -> std::io::Result<(tokio::task::JoinHandle<()>, SocketAddress)> {
        let Self {
            tx,
            rx,
            max_mtu,
            handle,
        } = self;

        // tell the endpoint what our MTU is
        endpoint.set_max_mtu(max_mtu);

        // create a tokio clock
        let clock = Clock::new();

        // create an event loop
        let event_loop = EventLoop {
            endpoint,
            clock,
            rx,
            tx,
        };

        // spawn the event loop on to the tokio handle
        let task = if let Some(handle) = handle {
            handle.spawn(event_loop.start())
        } else {
            tokio::spawn(event_loop.start())
        };

        Ok((task, SocketAddress::default()))
    }
}
