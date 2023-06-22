// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::io::tokio::Clock;
use s2n_quic_core::{
    endpoint::Endpoint, inet::SocketAddress, io::event_loop::EventLoop, path::MaxMtu,
};

pub use s2n_quic_core::{
    io::rx,
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

pub mod tx {
    pub use s2n_quic_core::io::tx::*;

    pub fn channel(
        socket: ::std::net::UdpSocket,
    ) -> (
        impl Tx<PathHandle = crate::message::default::Handle>,
        impl core::future::Future<Output = ::std::io::Result<()>>,
    ) {
        // Initial packets don't need to be bigger than the minimum
        let max_mtu = s2n_quic_core::path::MaxMtu::MIN;

        // It's unlikely the initial packets will utilize, so just disable it
        let gso = crate::features::Gso::default();
        gso.disable();

        // compute the payload size for each message from the number of GSO segments we can
        // fill
        let payload_len = {
            let max_mtu: u16 = max_mtu.into();
            (max_mtu as u32 * gso.max_segments() as u32).min(u16::MAX as u32)
        };

        // 512Kb
        let tx_buffer_size: u32 = 1 << 19;
        let entries = tx_buffer_size / payload_len;
        let entries = if entries.is_power_of_two() {
            entries
        } else {
            // round up to the nearest power of two, since the ring buffers require it
            entries.next_power_of_two()
        };

        let mut producers = vec![];

        let (producer, consumer) = crate::socket::ring::pair(entries, payload_len);
        producers.push(producer);

        // spawn a task that actually flushes the ring buffer to the socket
        let task = crate::io::tokio::task::tx(socket, consumer, gso.clone());

        // construct the TX side for the endpoint event loop
        let io = crate::socket::io::tx::Tx::new(producers, gso, max_mtu);

        (io, task)
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
