// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{handle::Transmission, Protocol, Socket, TransportFeatures};
use crate::{
    msg::{addr::Addr, cmsg},
    socket::channel::intrusive_queue,
    stream::send::state::transmission,
};
use core::task::{Context, Poll};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
};

/// A Socket implementation that sends transmissions into an intrusive queue channel.
///
/// This is the producer side - it implements `Socket` and allows streams to send
/// transmissions via `send_transmission` and `send_transmission_batch`. The consumer
/// side receives these transmissions from a channel and feeds them into a timing wheel.
#[derive(Clone)]
pub struct Wheel {
    sender: intrusive_queue::sync::Sender<transmission::Transmission>,
    local_addr: SocketAddr,
}

impl Wheel {
    pub fn new(
        sender: intrusive_queue::sync::Sender<transmission::Transmission>,
        local_addr: SocketAddr,
    ) -> Self {
        Self { sender, local_addr }
    }
}

impl Socket for Wheel {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local_addr)
    }

    #[inline]
    fn protocol(&self) -> Protocol {
        Protocol::Udp
    }

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
    }

    #[inline]
    fn poll_peek_len(&self, _cx: &mut Context) -> Poll<io::Result<usize>> {
        unimplemented!("Wheel socket is send-only")
    }

    #[inline]
    fn poll_recv(
        &self,
        _cx: &mut Context,
        _addr: &mut Addr,
        _cmsg: &mut cmsg::Receiver,
        _buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        unimplemented!("Wheel socket is send-only")
    }

    #[inline]
    fn send_transmission(&self, entry: Transmission) {
        let _ = self.sender.send_entry(entry);
    }

    #[inline]
    fn send_transmission_batch(&self, batch: transmission::EntryQueue) {
        let _ = self.sender.send_batch(batch);
    }

    #[inline]
    fn try_send(
        &self,
        _addr: &Addr,
        _ecn: ExplicitCongestionNotification,
        _buffer: &[IoSlice],
    ) -> io::Result<usize> {
        unimplemented!("Wheel socket uses send_transmission instead of try_send")
    }

    #[inline]
    fn poll_send(
        &self,
        _cx: &mut Context,
        _addr: &Addr,
        _ecn: ExplicitCongestionNotification,
        _buffer: &[IoSlice],
    ) -> Poll<io::Result<usize>> {
        unimplemented!("Wheel socket uses send_transmission instead of poll_send")
    }

    #[inline]
    fn send_finish(&self) -> io::Result<()> {
        // No shutdown needed for wheel-based sending
        Ok(())
    }
}
