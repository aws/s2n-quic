// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{packet::secret_control, path::secret::Map};
use s2n_codec::DecoderBufferMut;
use std::{net::SocketAddr, sync::Arc};

pub struct Control {
    socket: Arc<std::net::UdpSocket>,
    port: u16,
}

impl Control {
    pub fn new(address: SocketAddr, map: Map) -> std::io::Result<Self> {
        let socket = Arc::new(std::net::UdpSocket::bind(address)?);
        let port = socket.local_addr().unwrap().port();

        {
            let socket = socket.clone();
            std::thread::spawn(move || loop {
                let mut buffer = vec![0; 10_000];
                let (src, packet) = match socket.recv_from(&mut buffer) {
                    Ok((length, src)) => (src, DecoderBufferMut::new(&mut buffer[..length])),
                    Err(_) => continue,
                };
                let packet = secret_control::Packet::decode(packet);
                match packet {
                    Ok((packet, _remaining)) => map.handle_control_packet(&packet, &src),
                    Err(_) => continue,
                }
            });
        }

        Ok(Control { socket, port })
    }

    pub fn send_to(&self, dest: SocketAddr, packet: &[u8]) {
        // Our callers can't usefully handle errors either, so we just swallow them for now.
        let _ = self.socket.send_to(packet, dest);
    }

    pub fn port(&self) -> u16 {
        self.port
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
