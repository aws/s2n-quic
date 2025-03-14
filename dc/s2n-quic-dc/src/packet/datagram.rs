// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{tag::Common, HeaderLen, PacketNumber, PayloadLen};
use core::fmt;
use s2n_quic_core::packet::KeyPhase;
use zerocopy::{FromBytes, Unaligned};

pub mod decoder;
pub mod encoder;

#[derive(Clone, Copy, PartialEq, Eq, FromBytes, Unaligned)]
#[repr(C)]
pub struct Tag(Common);

impl_tag_codec!(Tag);

impl Default for Tag {
    #[inline]
    fn default() -> Self {
        Self(Common(0b0100_0000))
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("datagram::Tag")
            .field("ack_eliciting", &self.ack_eliciting())
            .field("is_connected", &self.is_connected())
            .field("has_application_header", &self.has_application_header())
            .field("key_phase", &self.key_phase())
            .finish()
    }
}

impl Tag {
    pub const ACK_ELICITING_MASK: u8 = 0b1000;
    pub const IS_CONNECTED_MASK: u8 = 0b0100;
    pub const HAS_APPLICATION_HEADER_MASK: u8 = 0b0010;
    pub const KEY_PHASE_MASK: u8 = 0b0001;

    pub const MIN: u8 = 0b0100_0000;
    pub const MAX: u8 = 0b0100_1111;

    #[inline]
    pub fn set_ack_eliciting(&mut self, enabled: bool) {
        self.0.set(Self::ACK_ELICITING_MASK, enabled)
    }

    #[inline]
    pub fn ack_eliciting(&self) -> bool {
        self.0.get(Self::ACK_ELICITING_MASK)
    }

    #[inline]
    pub fn set_is_connected(&mut self, enabled: bool) {
        self.0.set(Self::IS_CONNECTED_MASK, enabled)
    }

    #[inline]
    pub fn is_connected(&self) -> bool {
        self.0.get(Self::IS_CONNECTED_MASK)
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
        s2n_codec::decoder_invariant!(range.contains(&(self.0).0), "invalid datagram bit pattern");
        Ok(())
    }
}
