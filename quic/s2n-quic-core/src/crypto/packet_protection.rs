// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.1
//# The current encryption level secret and the label "quic key" are
//# input to the KDF to produce the AEAD key;
pub const QUIC_KEY_LABEL: [u8; 8] = *b"quic key";

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.1
//# the label "quic iv" is used
//# to derive the Initialization Vector (IV); see Section 5.3.
pub const QUIC_IV_LABEL: [u8; 7] = *b"quic iv";

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.1
//# The header protection key uses the "quic hp" label; see Section 5.4.
pub const QUIC_HP_LABEL: [u8; 7] = *b"quic hp";

use core::fmt;
use s2n_codec::DecoderError;

/// Error type for errors during removal of packet protection
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
pub struct Error {
    pub reason: &'static str,
}

impl Error {
    pub const DECODE_ERROR: Self = Self {
        reason: "DECODE_ERROR",
    };
    pub const DECRYPT_ERROR: Self = Self {
        reason: "DECRYPT_ERROR",
    };
    pub const INTERNAL_ERROR: Self = Self {
        reason: "INTERNAL_ERROR",
    };
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.reason.is_empty() {
            self.reason.fmt(f)
        } else {
            write!(f, "packet_protection::Error")
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("packet_protection::Error");

        if !self.reason.is_empty() {
            d.field("reason", &self.reason);
        }

        d.finish()
    }
}

impl From<DecoderError> for Error {
    fn from(decoder_error: DecoderError) -> Self {
        Self {
            reason: decoder_error.into(),
        }
    }
}
