// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::tag::Common;
use core::fmt;
use s2n_quic_core::{probe, varint::VarInt};
use zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned};

pub mod decoder;
pub mod encoder;
pub mod id;

type RelativeRetransmissionOffset = u32;

pub use id::Id;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(
    any(feature = "testing", test),
    derive(bolero_generator::TypeGenerator)
)]
pub enum PacketSpace {
    Stream,
    Recovery,
}

impl PacketSpace {
    #[inline]
    pub fn packet_number_into_nonce(&self, packet_number: VarInt) -> u64 {
        let mut nonce = packet_number.as_u64();
        if let Self::Recovery = self {
            nonce |= 1 << 62;
        }
        nonce
    }
}

impl probe::Arg for PacketSpace {
    #[inline]
    fn into_usdt(self) -> isize {
        match self {
            Self::Stream => 0,
            Self::Recovery => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, AsBytes, FromBytes, FromZeroes, Unaligned)]
#[repr(C)]
pub struct Tag(Common);

impl_tag_codec!(Tag);

impl Default for Tag {
    #[inline]
    fn default() -> Self {
        Self(Common(0b0000_0000))
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("stream::Tag")
            .field("has_source_stream_port", &self.has_source_stream_port())
            .field("has_control_data", &self.has_control_data())
            .field("packet_space", &self.packet_space())
            .field("has_final_offset", &self.has_final_offset())
            .field("has_application_header", &self.has_application_header())
            .finish()
    }
}

impl Tag {
    pub const HAS_SOURCE_STREAM_PORT: u8 = 0b01_0000;
    pub const IS_RECOVERY_PACKET: u8 = 0b00_1000;
    pub const HAS_CONTROL_DATA_MASK: u8 = 0b00_0100;
    pub const HAS_FINAL_OFFSET_MASK: u8 = 0b00_0010;
    pub const HAS_APPLICATION_HEADER_MASK: u8 = 0b00_0001;

    pub const MIN: u8 = 0b0000_0000;
    pub const MAX: u8 = 0b0011_1111;

    #[inline]
    pub fn set_has_source_stream_port(&mut self, enabled: bool) {
        self.0.set(Self::HAS_SOURCE_STREAM_PORT, enabled)
    }

    #[inline]
    pub fn has_source_stream_port(&self) -> bool {
        self.0.get(Self::HAS_SOURCE_STREAM_PORT)
    }

    #[inline]
    pub fn set_packet_space(&mut self, space: PacketSpace) {
        let enabled = matches!(space, PacketSpace::Recovery);
        self.0.set(Self::IS_RECOVERY_PACKET, enabled)
    }

    #[inline]
    pub fn packet_space(&self) -> PacketSpace {
        if self.0.get(Self::IS_RECOVERY_PACKET) {
            PacketSpace::Recovery
        } else {
            PacketSpace::Stream
        }
    }

    #[inline]
    pub fn set_has_control_data(&mut self, enabled: bool) {
        self.0.set(Self::HAS_CONTROL_DATA_MASK, enabled)
    }

    #[inline]
    pub fn has_control_data(&self) -> bool {
        self.0.get(Self::HAS_CONTROL_DATA_MASK)
    }

    #[inline]
    pub fn set_has_final_offset(&mut self, enabled: bool) {
        self.0.set(Self::HAS_FINAL_OFFSET_MASK, enabled)
    }

    #[inline]
    pub fn has_final_offset(&self) -> bool {
        self.0.get(Self::HAS_FINAL_OFFSET_MASK)
    }

    #[inline]
    pub fn set_has_application_header(&mut self, enabled: bool) {
        self.0.set(Self::HAS_APPLICATION_HEADER_MASK, enabled)
    }

    #[inline]
    pub fn has_application_header(&self) -> bool {
        self.0.get(Self::HAS_APPLICATION_HEADER_MASK)
    }

    #[inline]
    fn validate(&self) -> Result<(), s2n_codec::DecoderError> {
        let range = Self::MIN..=Self::MAX;
        s2n_codec::decoder_invariant!(range.contains(&(self.0).0), "invalid stream bit pattern");
        Ok(())
    }
}
