use super::{
    NOTIFY_GENERATION_RANGE, REJECT_SEQUENCE_ID, REQUEST_ADDITIONAL_GENERATION, UNKNOWN_PATH_SECRET,
};
use crate::crypto::IntoNonce;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero::TypeGenerator))]
pub enum Nonce {
    UnknownPathSecret,
    NotifyGenerationRange {
        min_generation_id: u32,
        max_generation_id: u32,
    },
    RejectSequenceId {
        generation_id: u32,
        max_generation_id: u32,
        sequence_id: u16,
    },
    RequestAdditionalGeneration {
        generation_id: u32,
    },
}

impl IntoNonce for Nonce {
    #[inline]
    fn into_nonce(self) -> [u8; 12] {
        let mut nonce = [0; 12];
        match self {
            Self::UnknownPathSecret => {
                nonce[0] = UNKNOWN_PATH_SECRET;
            }
            Self::NotifyGenerationRange {
                min_generation_id,
                max_generation_id,
            } => {
                nonce[0] = NOTIFY_GENERATION_RANGE;
                nonce[4..8].copy_from_slice(&min_generation_id.to_be_bytes());
                nonce[8..].copy_from_slice(&max_generation_id.to_be_bytes());
            }
            Self::RejectSequenceId {
                generation_id,
                max_generation_id,
                sequence_id,
            } => {
                nonce[0] = REJECT_SEQUENCE_ID;
                nonce[2..4].copy_from_slice(&sequence_id.to_be_bytes());
                nonce[4..8].copy_from_slice(&generation_id.to_be_bytes());
                nonce[8..].copy_from_slice(&max_generation_id.to_be_bytes());
            }
            Self::RequestAdditionalGeneration { generation_id } => {
                nonce[0] = REQUEST_ADDITIONAL_GENERATION;
                nonce[8..].copy_from_slice(&generation_id.to_be_bytes());
            }
        }
        nonce
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    /// ensures output nonces are only equal if the messages are equal
    #[test]
    #[cfg_attr(kani, kani::proof, kani::solver(cadical))]
    fn nonce_uniqueness() {
        check!().with_type::<(Nonce, Nonce)>().for_each(|(a, b)| {
            if a == b {
                assert_eq!(a.into_nonce(), b.into_nonce());
            } else {
                assert_ne!(a.into_nonce(), b.into_nonce());
            }
        });
    }
}
