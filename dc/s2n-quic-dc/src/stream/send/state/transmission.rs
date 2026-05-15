// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::stream::PacketSpace,
    socket::{
        pool::descriptor,
        send::{self, completion},
    },
    stream::shared::Half,
};
use bitflags::bitflags;
use core::{fmt, ops::Bound};
use s2n_quic_core::{inet::ExplicitCongestionNotification, time::Timestamp, varint::VarInt};
use std::sync::Weak;

pub type Completion = Weak<dyn Notify>;

pub trait Notify: 'static + Send + Sync {
    fn complete(&self, entry: Entry);
}

impl completion::Completion<PacketInfo, Meta> for Completion {
    type Completer = std::sync::Arc<dyn Notify>;

    fn upgrade(&self) -> Option<Self::Completer> {
        Weak::upgrade(self)
    }

    fn is_alive(&self) -> bool {
        self.strong_count() > 0
    }
}

impl completion::Completer<PacketInfo, Meta, Completion> for std::sync::Arc<dyn Notify> {
    fn complete(self, entry: Entry) {
        Notify::complete(&*self, entry);
    }
}

pub type Entry = send::transmission::Entry<PacketInfo, Meta, Completion>;

pub type Transmission = send::transmission::Transmission<PacketInfo, Meta, Completion>;

pub type Builder = send::transmission::Builder<PacketInfo, Meta, Completion>;

/// An intrusive queue of transmission entries, used for batch submission.
pub type EntryQueue =
    crate::intrusive_queue::Queue<send::transmission::Transmission<PacketInfo, Meta, Completion>>;

pub type PacketInfo = (VarInt, Info);

pub struct Event {
    pub packet_number: VarInt,
    pub info: Info,
    pub meta: Meta,
}

pub type Queue = completion::Queue<PacketInfo, Meta, Weak<dyn Notify>>;
pub type CompleteTransmission<'a> = completion::CompleteTransmission<'a, PacketInfo, Meta>;

#[derive(Clone)]
pub struct SenderSpan {
    #[cfg(debug_assertions)]
    #[allow(dead_code)]
    span: tracing::Span,
}

impl fmt::Debug for SenderSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SenderSpan").finish()
    }
}

impl Default for SenderSpan {
    fn default() -> Self {
        Self {
            #[cfg(debug_assertions)]
            span: tracing::warn_span!("sender"),
        }
    }
}

#[derive(Debug)]
pub struct Meta {
    pub packet_space: PacketSpace,
    pub final_offset: Option<VarInt>,
    pub half: Half,
    pub span: SenderSpan,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            packet_space: PacketSpace::Stream,
            final_offset: None,
            half: Half::Write,
            span: SenderSpan::default(),
        }
    }
}

bitflags! {
    #[derive(Copy, Clone, Debug, Default)]
    pub struct Flags: u8 {
        const PROBE = 1 << 0;
        const INCLUDED_FINAL_OFFSET = 1 << 1;
        const INCLUDED_FINAL_BYTE = 1 << 2;
        const INCLUDED_RESET = 1 << 3;
    }
}

impl Flags {
    pub fn is_probe(&self) -> bool {
        self.contains(Self::PROBE)
    }

    pub fn with_probe(mut self, enabled: bool) -> Self {
        self.set(Self::PROBE, enabled);
        self
    }

    pub fn included_final_offset(&self) -> bool {
        self.contains(Self::INCLUDED_FINAL_OFFSET)
    }

    pub fn with_included_final_offset(mut self, enabled: bool) -> Self {
        self.set(Self::INCLUDED_FINAL_OFFSET, enabled);
        self
    }

    pub fn included_final_byte(&self) -> bool {
        self.contains(Self::INCLUDED_FINAL_BYTE)
    }

    pub fn with_included_final_byte(mut self, enabled: bool) -> Self {
        self.set(Self::INCLUDED_FINAL_BYTE, enabled);
        self
    }

    pub fn included_reset(&self) -> bool {
        self.contains(Self::INCLUDED_RESET)
    }

    pub fn with_included_reset(mut self, enabled: bool) -> Self {
        self.set(Self::INCLUDED_RESET, enabled);
        self
    }
}

#[derive(Debug)]
pub struct Info {
    pub packet_len: u16,
    pub descriptor: Option<descriptor::Filled>,
    pub stream_offset: VarInt,
    pub payload_len: u16,
    pub flags: Flags,
    pub time_sent: Timestamp,
    pub ecn: ExplicitCongestionNotification,
}

impl Info {
    #[inline]
    pub fn cca_len(&self) -> u16 {
        if self.payload_len == 0 {
            self.packet_len
        } else {
            self.payload_len
        }
    }

    pub fn is_probe(&self) -> bool {
        self.flags.is_probe()
    }

    #[inline]
    pub fn range(&self) -> core::ops::Range<VarInt> {
        self.stream_offset..self.end_offset()
    }

    /// Similar to range but extends to [`VarInt::MAX`] if `included_fin` is true
    #[inline]
    pub fn tracking_range(&self) -> (Bound<VarInt>, Bound<VarInt>) {
        let start = Bound::Included(self.stream_offset);
        let end = if self.flags.included_final_byte() {
            Bound::Included(VarInt::MAX)
        } else {
            Bound::Excluded(self.end_offset())
        };
        (start, end)
    }

    /// Non-inclusive offset
    #[inline]
    pub fn end_offset(&self) -> VarInt {
        self.stream_offset + VarInt::from_u16(self.payload_len)
    }
}

impl Info {
    #[inline]
    pub fn try_retransmit(&mut self) -> Option<super::retransmission::Segment> {
        let descriptor = self.descriptor.take()?;

        let retransmission = super::retransmission::Segment {
            descriptor,
            stream_offset: self.stream_offset,
            payload_len: self.payload_len,
            flags: self.flags,
        };

        Some(retransmission)
    }

    /// Creates a retransmission segment by deep-copying the descriptor, leaving
    /// the original in place. This is used for proactive retransmission on
    /// PTO where the packet hasn't been declared lost - we're just eagerly
    /// retransmitting to avoid an extra round trip.
    #[inline]
    pub fn retransmit_copy(&self) -> Option<super::retransmission::Segment> {
        let descriptor = self.descriptor.as_ref()?.deep_copy()?;

        let retransmission = super::retransmission::Segment {
            descriptor,
            stream_offset: self.stream_offset,
            payload_len: self.payload_len,
            flags: self.flags,
        };

        Some(retransmission)
    }
}
