// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Tag;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//# Key Phase:  The next bit (0x04) of byte 0 indicates the key phase,
//# which allows a recipient of a packet to identify the packet
//# protection keys that are used to protect the packet.

const KEY_PHASE_MASK: u8 = 0x04;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProtectedKeyPhase;

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    any(test, feature = "bolero-generator"),
    derive(bolero_generator::TypeGenerator)
)]
pub enum KeyPhase {
    Zero,
    One,
}

impl Default for KeyPhase {
    #[inline]
    fn default() -> Self {
        Self::Zero
    }
}

const PHASES: [KeyPhase; 2] = [KeyPhase::Zero, KeyPhase::One];
impl From<u8> for KeyPhase {
    #[inline]
    fn from(v: u8) -> Self {
        // Will only be 0 or 1. Invalid phases may result in a failed decryption, still in constant
        // time.
        PHASES[(v & 0x01) as usize]
    }
}

impl KeyPhase {
    #[inline]
    pub fn from_tag(tag: Tag) -> Self {
        let phase = (tag & KEY_PHASE_MASK) >> 2;
        PHASES[phase as usize]
    }

    #[inline]
    pub fn into_packet_tag_mask(self) -> u8 {
        match self {
            Self::One => KEY_PHASE_MASK,
            Self::Zero => 0,
        }
    }

    #[must_use]
    #[inline]
    pub fn next_phase(self) -> Self {
        PHASES[(((self as u8) + 1) % 2) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyphase_from_tag() {
        for i in 0..255 {
            let phase = KeyPhase::from_tag(i);
            assert_eq!(phase.into_packet_tag_mask(), i & KEY_PHASE_MASK);
        }
    }

    #[test]
    fn test_next_phase() {
        for i in 0..254 {
            let phase = KeyPhase::from(i);
            let next_phase = KeyPhase::from(i + 1);
            assert_eq!(phase.next_phase(), next_phase);
        }
    }
}
