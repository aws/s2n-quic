// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::tag::Common;
use core::fmt;
use zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned};

pub mod decoder;
pub mod encoder;
mod id;

pub use id::Id;

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
            .field("has_control_data", &self.has_control_data())
            .field("has_final_offset", &self.has_final_offset())
            .field("has_application_header", &self.has_application_header())
            .finish()
    }
}

impl Tag {
    const HAS_SOURCE_STREAM_PORT: u8 = 0b01_0000;
    const HAS_CONTROL_DATA_MASK: u8 = 0b00_0100;
    const HAS_FINAL_OFFSET_MASK: u8 = 0b00_0010;
    const HAS_APPLICATION_HEADER_MASK: u8 = 0b00_0001;

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
