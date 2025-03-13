// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::TransportFeatures;
use arrayvec::ArrayVec;
use core::ops::Deref;
use s2n_quic_core::{ensure, inet::ExplicitCongestionNotification};
use std::io::IoSlice;

/// The maximum number of segments in sendmsg calls
///
/// From <https://elixir.bootlin.com/linux/v6.8.7/source/include/uapi/linux/uio.h#L27>
/// > #define UIO_FASTIOV 8
pub const MAX_COUNT: usize = if cfg!(target_os = "linux") { 8 } else { 1 };

/// The maximum payload allowed in sendmsg calls using UDP
///
/// From <https://github.com/torvalds/linux/blob/8cd26fd90c1ad7acdcfb9f69ca99d13aa7b24561/net/ipv4/ip_output.c#L987-L995>
/// > Linux enforces a u16::MAX - IP_HEADER_LEN - UDP_HEADER_LEN
pub const MAX_TOTAL: u16 = u16::MAX - 50;

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
