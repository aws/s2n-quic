// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::TransportFeatures;
use arrayvec::ArrayVec;
use core::ops::Deref;
use s2n_quic_core::{ensure, inet::ExplicitCongestionNotification};
use s2n_quic_platform::features;
use std::io::IoSlice;

/// The maximum size of a IP+UDP header
const IPV4_HEADER_LEN: u16 = 20;
const IPV6_HEADER_LEN: u16 = 40;
const UDP_HEADER_LEN: u16 = 8;

const fn min_u16(a: u16, b: u16) -> u16 {
    if a < b {
        a
    } else {
        b
    }
}

/// The maximum number of segments in sendmsg calls
///
/// From <https://elixir.bootlin.com/linux/v6.8.7/source/include/uapi/linux/uio.h#L28>
/// > #define UIO_MAXIOV  1024
pub const MAX_COUNT: usize = if features::gso::IS_SUPPORTED {
    // base the max segments on the max datagram size for the default ethernet mtu
    let max_datagram_size = 1500 - min_u16(IPV4_HEADER_LEN, IPV6_HEADER_LEN) - UDP_HEADER_LEN;

    (MAX_TOTAL / max_datagram_size) as _
} else {
    // only a single segment can be sent per syscall
    1
};

/// The maximum payload allowed in sendmsg calls using IPv4+UDP
const MAX_TOTAL_IPV4: u16 = if cfg!(target_os = "linux") {
    // From <https://github.com/torvalds/linux/blob/8cd26fd90c1ad7acdcfb9f69ca99d13aa7b24561/net/ipv4/ip_output.c#L987-L995>
    // > Linux enforces a u16::MAX - IP_HEADER_LEN - UDP_HEADER_LEN
    u16::MAX - IPV4_HEADER_LEN - UDP_HEADER_LEN
} else {
    9001 - IPV4_HEADER_LEN - UDP_HEADER_LEN
};

/// The maximum payload allowed in sendmsg calls using IPv6+UDP
const MAX_TOTAL_IPV6: u16 = if cfg!(target_os = "linux") {
    // IPv6 doesn't include the IP header size in the calculation
    u16::MAX - UDP_HEADER_LEN
} else {
    9001 - IPV6_HEADER_LEN - UDP_HEADER_LEN
};

/// The minimum payload size between the IPv4 and IPv6 sizes
pub const MAX_TOTAL: u16 = min_u16(MAX_TOTAL_IPV4, MAX_TOTAL_IPV6);

#[test]
fn max_total_test() {
    let tests = [("127.0.0.1:0", MAX_TOTAL_IPV4), ("[::1]:0", MAX_TOTAL_IPV6)];

    for (addr, total) in tests {
        let socket = std::net::UdpSocket::bind(addr).unwrap();
        let addr = socket.local_addr().unwrap();

        let mut send_buffer = vec![0u8; total as usize + 1];
        let mut recv_buffer = vec![0u8; total as usize];

        // This behavior may not be consistent across kernel versions so the check is disabled by default
        let _ = socket.send_to(&send_buffer, addr);

        send_buffer.pop().unwrap();
        socket
            .send_to(&send_buffer, addr)
            .expect("send should succeed when limited to MAX_TOTAL");

        let size_of_recv = socket.recv(&mut recv_buffer).unwrap();
        assert_eq!(total as usize, size_of_recv);
    }
}

type Segments<'a> = ArrayVec<IoSlice<'a>, MAX_COUNT>;

pub struct Batch<'a> {
    segments: Segments<'a>,
    ecn: ExplicitCongestionNotification,
}

impl<'a> Deref for Batch<'a> {
    type Target = [IoSlice<'a>];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.segments
    }
}

impl<'a> Batch<'a> {
    #[inline]
    pub fn new<Q>(queue: Q, features: &TransportFeatures) -> Self
    where
        Q: IntoIterator<Item = (ExplicitCongestionNotification, &'a [u8])>,
    {
        // this value is replaced by the first segment
        let mut ecn = ExplicitCongestionNotification::Ect0;
        let mut total_len = 0u32;
        let mut segments = Segments::new();

        for segment in queue {
            let packet_len = segment.1.len();
            debug_assert!(
                packet_len <= u16::MAX as usize,
                "segments should not exceed the maximum datagram size"
            );
            let packet_len = packet_len as u16;

            let new_total_len = total_len + packet_len as u32;

            if !features.is_stream() {
                // make sure we don't exceed the max allowed payload size
                ensure!(new_total_len < MAX_TOTAL as u32, break);
            }

            // track if the current segment is undersized from the previous
            let mut undersized_segment = false;

            // make sure we're compatible with the previous segment
            if let Some(first_segment) = segments.first() {
                ensure!(first_segment.len() >= packet_len as usize, break);
                // this is the last segment we can push if the segment is undersized
                undersized_segment = first_segment.len() > packet_len as usize;
                // make sure ecn doesn't change with this transmission
                ensure!(ecn == segment.0, break);
            } else {
                // update the ecn value with the first segment
                ecn = segment.0;
            }

            // update the total len once we confirm this segment can be written
            total_len = new_total_len;

            let iovec = std::io::IoSlice::new(segment.1);
            segments.push(iovec);

            // if this segment was undersized, then bail
            ensure!(!undersized_segment, break);

            // make sure we have capacity before looping back around
            ensure!(!segments.is_full(), break);
        }

        Self { segments, ecn }
    }

    #[inline]
    pub fn ecn(&self) -> ExplicitCongestionNotification {
        self.ecn
    }
}
