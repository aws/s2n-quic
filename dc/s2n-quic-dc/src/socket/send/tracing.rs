// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Socket;
use crate::{msg::addr::Addr, tracing::trace};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::io::{self, IoSlice};

pub struct Tracing<S>(pub S);

impl<S: crate::socket::LocalAddr> crate::socket::LocalAddr for Tracing<S> {
    #[inline]
    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.0.local_addr()
    }
}

impl<S: Socket> Socket for Tracing<S> {
    #[inline]
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        let result = self.0.send_msg(addr, payload, segment_size, ecn);

        trace!(
            local_addr = %self.0.local_addr().unwrap_or_else(|_| ([0, 0, 0, 0], 0).into()),
            peer_addr = %addr.get(),
            ?ecn,
            segments = payload.len(),
            segment_size,
            total_len = payload.iter().map(|s| s.len()).sum::<usize>(),
            ?result,
            "send_msg"
        );

        result
    }
}
