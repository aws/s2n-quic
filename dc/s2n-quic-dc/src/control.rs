// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{packet::secret_control, path::secret::Map};
use s2n_codec::DecoderBufferMut;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

/// How long the receive loop blocks before re-checking the shutdown flag.
///
/// [`Control::drop`] also sends a wakeup datagram to unblock the receive
/// immediately, so this only bounds how long shutdown takes if that datagram is
/// lost.
///
/// We keep this pretty high since it's this loop is expected to normally be close to idle;
/// spurious wakeups as such carry a relatively high cost.
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct Control {
    socket: Arc<std::net::UdpSocket>,
    port: u16,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Control {
    pub fn new(address: SocketAddr, map: Map) -> std::io::Result<Self> {
        let socket = Arc::new(std::net::UdpSocket::bind(address)?);
        let port = socket.local_addr()?.port();
        socket.set_read_timeout(Some(SHUTDOWN_POLL_INTERVAL))?;

        let shutdown = Arc::new(AtomicBool::new(false));

        let handle = {
            let socket = socket.clone();
            let shutdown = shutdown.clone();
            std::thread::spawn(move || {
                while !shutdown.load(Ordering::Relaxed) {
                    let mut buffer = vec![0; 10_000];
                    let (src, packet) = match socket.recv_from(&mut buffer) {
                        Ok((length, src)) => (src, DecoderBufferMut::new(&mut buffer[..length])),
                        // This covers both the read timeout expiring and any
                        // other receive error; either way we re-check the
                        // shutdown flag and try again.
                        Err(_) => continue,
                    };
                    // Check before decoding to avoid spurious error metrics if this is the
                    // sentinel packet.
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    let packet = secret_control::Packet::decode(packet);
                    match packet {
                        Ok((packet, _remaining)) => map.handle_control_packet(&packet, &src),
                        Err(_) => continue,
                    }
                }
            })
        };

        Ok(Control {
            socket,
            port,
            shutdown,
            handle: Some(handle),
        })
    }

    pub fn send_to(&self, dest: SocketAddr, packet: &[u8]) {
        // Our callers can't usefully handle errors either, so we just swallow them for now.
        let _ = self.socket.send_to(packet, dest);
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);

        // Unblock a receive that's currently in progress.
        if let Ok(mut local) = self.socket.local_addr() {
            // A socket bound to the wildcard address isn't necessarily
            // addressable via that address, so aim at loopback instead.
            if local.ip().is_unspecified() {
                local.set_ip(match local.ip() {
                    IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
                    IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
                });
            }
            // Send some arbitrary contents. This is just trying to avoid any issues from
            // zero-length datagrams being ignored by any layer of the stack (in principle should
            // be fine, but cheap to guarantee that doesn't happen).
            let _ = self.socket.send_to(&[0], local);
        }

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub trait Controller {
    /// Returns the source port to which control/reset messages should be sent
    fn source_port(&self) -> u16;
}

impl Controller for u16 {
    #[inline]
    fn source_port(&self) -> u16 {
        *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> Map {
        Map::for_test_with_peers(vec![(
            crate::path::secret::schedule::Ciphersuite::AES_GCM_128_SHA256,
            s2n_quic_core::dc::SUPPORTED_VERSIONS[0],
            SocketAddr::from(([127, 0, 0, 1], 1234)),
        )])
        .0
    }

    /// Drops a `Control` bound to `address`, which joins its receive thread.
    ///
    /// If shutdown were broken the join would hang and the test would time out
    /// rather than fail, but either way CI reports it.
    fn shutdown(address: SocketAddr) {
        let control = Control::new(address, map()).unwrap();
        assert_ne!(control.port(), 0);
        drop(control);
    }

    #[test]
    fn shutdown_loopback() {
        shutdown(SocketAddr::from(([127, 0, 0, 1], 0)));
    }

    #[test]
    fn shutdown_wildcard() {
        shutdown(SocketAddr::from(([0, 0, 0, 0], 0)));
    }
}
