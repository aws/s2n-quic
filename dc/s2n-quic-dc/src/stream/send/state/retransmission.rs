// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::send::transmission;
use core::cmp::Ordering;
use s2n_quic_core::varint::VarInt;

#[derive(Debug)]
pub struct Segment<S> {
    pub segment: S,
    pub ty: transmission::Type,
    pub stream_offset: VarInt,
    pub payload_len: u16,
    pub included_fin: bool,
}

impl<S> Segment<S> {
    pub fn range(&self) -> core::ops::Range<VarInt> {
        let start = self.stream_offset;
        let end = start + VarInt::from_u16(self.payload_len);
        start..end
    }
}

impl<S> PartialEq for Segment<S> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl<S> Eq for Segment<S> {}

impl<S> PartialOrd for Segment<S> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<S> Ord for Segment<S> {
    #[inline]
    fn cmp(&self, rhs: &Self) -> Ordering {
        self.ty
            .cmp(&rhs.ty)
            .then(self.stream_offset.cmp(&rhs.stream_offset))
            .then(self.payload_len.cmp(&rhs.payload_len))
            .reverse()
    }
}

impl<S> core::ops::Deref for Segment<S> {
    type Target = S;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.segment
    }
}
