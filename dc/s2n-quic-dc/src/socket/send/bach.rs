// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! `socket::send::Socket` implementation for `bach::net::UdpSocket`.
//!
//! Used in deterministic simulation tests where real OS sockets are replaced by
//! Bach's in-process simulated network.

use crate::msg::addr::Addr;
use bach::net::{socket::SendOptions, UdpSocket};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    io::{self, IoSlice},
    net::SocketAddr,
};

impl super::Socket for UdpSocket {
    #[inline]
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        // No point sending empty data.
        if payload.is_empty() {
            return Ok(0);
        }

        let dest: SocketAddr = addr.get().into();
        let mut opts = SendOptions::default();
        opts.ecn = ecn as u8;

        // More than one buffer slice means GSO-style segmentation; tell Bach the
        // per-segment size so it can reconstruct individual datagrams.
        if payload.len() > 1 && segment_size > 0 {
            opts.segment_len = Some(segment_size as usize);
        }

        UdpSocket::try_send_msg(self, dest, payload, opts)
    }

    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        UdpSocket::local_addr(self)
    }
}
