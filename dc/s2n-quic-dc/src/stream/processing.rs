// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::open;

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum Error {
    #[error("packet could not be decrypted: {0}")]
    Crypto(open::Error),
    #[error("packet has already been processed")]
    Duplicate,
    #[error("the crypto key has been replayed and is invalid")]
    KeyReplayPrevented,
    #[error("the crypto key has been potentially replayed (gap: {gap:?}) and is invalid")]
    KeyReplayPotentiallyPrevented { gap: Option<u64> },
}

impl From<open::Error> for Error {
    fn from(value: open::Error) -> Self {
        match value {
            open::Error::ReplayDefinitelyDetected => Self::KeyReplayPrevented,
            open::Error::ReplayPotentiallyDetected { gap } => {
                Self::KeyReplayPotentiallyPrevented { gap }
            }
            error => Self::Crypto(error),
        }
    }
}
