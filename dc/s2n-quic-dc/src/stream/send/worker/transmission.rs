// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ops::Bound;
use s2n_quic_core::{inet::ExplicitCongestionNotification, time::Timestamp, varint::VarInt};

#[derive(Debug)]
pub struct Info<Retransmission> {
    pub packet_len: u16,
    pub retransmission: Option<Retransmission>,
    pub stream_offset: VarInt,
    pub payload_len: u16,
    pub included_fin: bool,
    pub time_sent: Timestamp,
    pub ecn: ExplicitCongestionNotification,
}

impl<Retransmission> Info<Retransmission> {
    #[inline]
    pub fn map<R>(self, f: impl FnOnce(Retransmission) -> R) -> Info<R> {
        let Self {
            packet_len,
            retransmission,
            stream_offset,
            payload_len,
            included_fin,
            time_sent,
            ecn,
        } = self;
        let retransmission = retransmission.map(f);
        Info {
            packet_len,
            retransmission,
            stream_offset,
            payload_len,
            included_fin,
            time_sent,
            ecn,
        }
    }

    #[inline]
    pub fn cca_len(&self) -> u16 {
        if self.payload_len == 0 {
            self.packet_len
        } else {
            self.payload_len
        }
    }

    #[inline]
    pub fn range(&self) -> core::ops::Range<VarInt> {
        self.stream_offset..self.end_offset()
    }

    /// Similar to range but extends to [`VarInt::MAX`] if `included_fin` is true
    #[inline]
    pub fn tracking_range(&self) -> (Bound<VarInt>, Bound<VarInt>) {
        let start = Bound::Included(self.stream_offset);
        let end = if self.included_fin {
            Bound::Included(VarInt::MAX)
        } else {
            Bound::Excluded(self.end_offset())
        };
        (start, end)
    }

    #[inline]
    pub fn end_offset(&self) -> VarInt {
        self.stream_offset + VarInt::from_u16(self.payload_len)
    }
}
