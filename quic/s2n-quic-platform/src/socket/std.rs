// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::Buffer,
    message::{
        queue,
        simple::{Message, Ring},
        Message as _,
    },
};
use s2n_quic_core::{
    inet::SocketAddress,
    io::{rx, tx},
};

pub trait Socket {
    type Error: Error;

    /// Receives a payload and returns the length and source address
    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddress>), Self::Error>;

    /// Sends a payload to the given address and returns the length of the sent payload
    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> Result<usize, Self::Error>;
}

#[cfg(feature = "std")]
impl Socket for std::net::UdpSocket {
    type Error = std::io::Error;

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddress>), Self::Error> {
        debug_assert!(!buf.is_empty());
        let (len, addr) = self.recv_from(buf)?;
        Ok((len, Some(addr.into())))
    }

    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> Result<usize, Self::Error> {
        debug_assert!(!buf.is_empty());
        let addr: std::net::SocketAddr = (*addr).into();
        self.send_to(buf, &addr)
    }
}

pub trait Error {
    fn would_block(&self) -> bool;
}

#[cfg(feature = "std")]
impl Error for std::io::Error {
    fn would_block(&self) -> bool {
        self.kind() == std::io::ErrorKind::WouldBlock
    }
}

#[derive(Debug, Default)]
pub struct Queue<B: Buffer>(queue::Queue<Ring<B>>);

impl<B: Buffer> Queue<B> {
    pub fn new(buffer: B) -> Self {
        let queue = queue::Queue::new(Ring::new(buffer));

        Self(queue)
    }

    pub fn tx<S: Socket>(&mut self, socket: &S) -> Result<usize, S::Error> {
        let mut count = 0;
        let mut entries = self.0.free_mut();

        for entry in entries.as_mut() {
            if let Some(remote_address) = entry.remote_address() {
                match socket.send_to(entry.payload_mut(), &remote_address) {
                    Ok(_) => {
                        count += 1;
                    }
                    Err(err) => {
                        if count > 0 && err.would_block() {
                            break;
                        } else {
                            entries.finish(count);
                            return Err(err);
                        }
                    }
                }
            }
        }

        entries.finish(count);

        Ok(count)
    }

    pub fn rx<S: Socket>(&mut self, socket: &S) -> Result<usize, S::Error> {
        let mut count = 0;
        let mut entries = self.0.occupied_wipe_mut();

        while let Some(entry) = entries.get_mut(count) {
            match socket.recv_from(entry.payload_mut()) {
                Ok((payload_len, Some(remote_address))) => {
                    entry.set_remote_address(&remote_address);
                    unsafe {
                        // Safety: The payload_len should not be bigger than the number of
                        // allocated bytes.

                        debug_assert!(payload_len < entry.payload_len());
                        let payload_len = payload_len.min(entry.payload_len());

                        entry.set_payload_len(payload_len);
                    }
                    count += 1;
                }
                Ok((_payload_len, None)) => {}
                Err(err) => {
                    if count > 0 && err.would_block() {
                        break;
                    } else {
                        entries.finish(count);
                        return Err(err);
                    }
                }
            }
        }

        entries.finish(count);

        Ok(count)
    }
}

impl<'a, B: Buffer> tx::Tx<'a> for Queue<B> {
    type Queue = queue::Free<'a, Message>;

    fn queue(&'a mut self) -> Self::Queue {
        self.0.free_mut()
    }

    fn len(&self) -> usize {
        self.0.occupied_len()
    }
}

impl<'a, B: Buffer> rx::Rx<'a> for Queue<B> {
    type Queue = queue::OccupiedWipe<'a, Message>;

    fn queue(&'a mut self) -> Self::Queue {
        self.0.occupied_wipe_mut()
    }

    fn len(&self) -> usize {
        self.0.free_len()
    }
}
