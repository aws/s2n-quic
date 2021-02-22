use super::Tag;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.3
//# Key Phase:  The next bit (0x04) of byte 0 indicates the key phase,
//#    which allows a recipient of a packet to identify the packet
//#    protection keys that are used to protect the packet.

const KEY_PHASE_MASK: u8 = 0x04;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProtectedKeyPhase;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyPhase {
    Zero,
    One,
}

impl Default for KeyPhase {
    fn default() -> Self {
        Self::Zero
    }
}

const PHASES: [KeyPhase; 2] = [KeyPhase::Zero, KeyPhase::One];
impl From<u8> for KeyPhase {
    fn from(v: u8) -> Self {
        // Will only be 0 or 1. Invalid phases may result in a failed decryption, still in constant
        // time.
        PHASES[(v & 0x01) as usize]
    }
}

impl KeyPhase {
    pub fn from_tag(tag: Tag) -> Self {
        if tag & KEY_PHASE_MASK == KEY_PHASE_MASK {
            Self::One
        } else {
            Self::Zero
        }
    }

    pub fn into_packet_tag_mask(self) -> u8 {
        match self {
            Self::One => KEY_PHASE_MASK,
            Self::Zero => 0,
        }
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
}
