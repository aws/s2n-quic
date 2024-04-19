// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::decrypt;

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum Error {
    #[error("packet could not be decrypted")]
    Decrypt,
    #[error("packet has already been processed")]
    Duplicate,
    #[error("the crypto key has been replayed and is invalid")]
    KeyReplayPrevented,
    #[error("the crypto key has been potentially replayed and is invalid")]
    KeyReplayPotentiallyPrevented,
}

impl From<decrypt::Error> for Error {
    fn from(value: decrypt::Error) -> Self {
        match value {
            decrypt::Error::ReplayDefinitelyDetected => Self::KeyReplayPrevented,
            decrypt::Error::ReplayPotentiallyDetected => Self::KeyReplayPotentiallyPrevented,
            decrypt::Error::InvalidTag => Self::Decrypt,
        }
    }
}
