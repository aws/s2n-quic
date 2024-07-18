// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::tag::Common;
use core::fmt;
use s2n_quic_core::packet::KeyPhase;
use zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned};

const NONCE_MASK: u64 = 1 << 63;

pub mod decoder;
pub mod encoder;

#[derive(Clone, Copy, PartialEq, Eq, AsBytes, FromBytes, FromZeroes, Unaligned)]
#[repr(C)]
pub struct Tag(Common);

impl_tag_codec!(Tag);

impl Default for Tag {
    #[inline]
    fn default() -> Self {
        Self(Common(0b0101_0000))
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("control::Tag")
            .field("is_stream", &self.is_stream())
            .field("has_application_header", &self.has_application_header())
            .field("key_phase", &self.key_phase())
            .finish()
    }
}

impl Tag {
    pub const IS_STREAM_MASK: u8 = 0b0100;
    pub const HAS_APPLICATION_HEADER_MASK: u8 = 0b0010;
    pub const KEY_PHASE_MASK: u8 = 0b0001;

    pub const MIN: u8 = 0b0101_0000;
    pub const MAX: u8 = 0b0101_1111;

    #[inline]
    pub fn set_is_stream(&mut self, enabled: bool) {
        self.0.set(Self::IS_STREAM_MASK, enabled)
    }

    #[inline]
    pub fn is_stream(&self) -> bool {
        self.0.get(Self::IS_STREAM_MASK)
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
    pub fn set_key_phase(&mut self, key_phase: KeyPhase) {
        let v = match key_phase {
            KeyPhase::Zero => false,
            KeyPhase::One => true,
        };
        self.0.set(Self::KEY_PHASE_MASK, v)
    }

    #[inline]
    pub fn key_phase(&self) -> KeyPhase {
        if self.0.get(Self::KEY_PHASE_MASK) {
            KeyPhase::One
        } else {
            KeyPhase::Zero
        }
    }

    #[inline]
    fn validate(&self) -> Result<(), s2n_codec::DecoderError> {
        let range = Self::MIN..=Self::MAX;
        s2n_codec::decoder_invariant!(range.contains(&(self.0).0), "invalid control bit pattern");
        Ok(())
    }
}
