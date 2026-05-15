// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{tag::Common, HeaderLen, PacketNumber, PayloadLen};
use core::fmt;
use s2n_quic_core::packet::KeyPhase;
use zerocopy::{FromBytes, Unaligned};

pub mod decoder;
pub mod encoder;
mod routing_info;

pub use routing_info::{QueuePair, ResetTarget, RoutingInfo};

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
            .field("has_routing_info", &self.has_routing_info())
            .field("has_packet_number", &self.has_packet_number())
            .field("payload_encrypted", &self.payload_encrypted())
            .field("key_phase", &self.key_phase())
            .finish()
    }
}

impl Tag {
    pub const HAS_ROUTING_INFO_MASK: u8 = 0b1000;
    pub const HAS_PACKET_NUMBER_MASK: u8 = 0b0100;
    pub const PAYLOAD_ENCRYPTED_MASK: u8 = 0b0010;
    pub const KEY_PHASE_MASK: u8 = 0b0001;

    pub const MIN: u8 = 0b0100_0000;
    pub const MAX: u8 = 0b0100_1111;

    #[inline]
    pub fn set_has_routing_info(&mut self, enabled: bool) {
        self.0.set(Self::HAS_ROUTING_INFO_MASK, enabled)
    }

    #[inline]
    pub fn has_routing_info(&self) -> bool {
        self.0.get(Self::HAS_ROUTING_INFO_MASK)
    }

    #[inline]
    pub fn set_has_packet_number(&mut self, enabled: bool) {
        self.0.set(Self::HAS_PACKET_NUMBER_MASK, enabled)
    }

    #[inline]
    pub fn has_packet_number(&self) -> bool {
        self.0.get(Self::HAS_PACKET_NUMBER_MASK)
    }

    #[inline]
    pub fn set_payload_encrypted(&mut self, enabled: bool) {
        self.0.set(Self::PAYLOAD_ENCRYPTED_MASK, enabled)
    }

    #[inline]
    pub fn payload_encrypted(&self) -> bool {
        self.0.get(Self::PAYLOAD_ENCRYPTED_MASK)
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
