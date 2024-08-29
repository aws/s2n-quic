// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use bytes::buf::UninitSlice;
use core::fmt;
pub use s2n_quic_core::packet::KeyPhase;

pub mod awslc;

pub mod seal {
    use super::*;

    pub trait Application {
        fn key_phase(&self) -> KeyPhase;

        fn tag_len(&self) -> usize;

        /// Encrypt a payload
        fn encrypt(
            &self,
            packet_number: u64,
            header: &[u8],
            extra_payload: Option<&[u8]>,
            payload_and_tag: &mut [u8],
        );
    }

    pub trait Control {
        fn tag_len(&self) -> usize;

        fn sign(&self, header: &[u8], tag: &mut [u8]);
    }

    pub mod control {
        use super::*;

        /// Marker trait for keys to be used with stream control packets
        pub trait Stream: Control {
            fn retransmission_tag(
                &self,
                original_packet_number: u64,
                retransmission_packet_number: u64,
                tag_out: &mut [u8],
            );
        }

        /// Marker trait for keys to be used with secret control packets
        pub trait Secret: Control {}
    }
}

pub mod open {
    use super::*;

    #[derive(PartialEq, Eq, Clone, Copy, Debug)]
    pub enum Error {
        // None if we don't even try to look (e.g., poisoned the tracker).
        ReplayPotentiallyDetected { gap: Option<u64> },
        ReplayDefinitelyDetected,
        InvalidTag,
        SingleUseKey,
        UnsupportedOperation,
        MacOnly,
        RotationNotSupported,
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self {
                Self::ReplayDefinitelyDetected => "key replay prevented".fmt(f),
                Self::ReplayPotentiallyDetected { gap: Some(gap) } => write!(
                    f,
                    "key replay potentially detected: {gap} from latest entry"
                ),
                Self::ReplayPotentiallyDetected { gap: None } => {
                    write!(f, "key replay potentially detected: unknown gap")
                }
                Self::InvalidTag => "invalid tag".fmt(f),
                Self::SingleUseKey => "this key can only be used once".fmt(f),
                Self::UnsupportedOperation => {
                    "this key cannot be used with the given operation".fmt(f)
                }
                Self::MacOnly => "this key is only capable of generating MACs".fmt(f),
                Self::RotationNotSupported => "this key does not support key rotation".fmt(f),
            }
        }
    }

    impl std::error::Error for Error {}

    pub type Result<T = (), E = Error> = core::result::Result<T, E>;

    pub trait Application {
        fn tag_len(&self) -> usize;

        /// Decrypt a payload
        fn decrypt(
            &self,
            key_phase: KeyPhase,
            packet_number: u64,
            header: &[u8],
            payload_in: &[u8],
            tag: &[u8],
            payload_out: &mut UninitSlice,
        ) -> Result;

        /// Decrypt a payload
        fn decrypt_in_place(
            &self,
            key_phase: KeyPhase,
            packet_number: u64,
            header: &[u8],
            payload_and_tag: &mut [u8],
        ) -> Result;
    }

    pub trait Control {
        fn tag_len(&self) -> usize;

        fn verify(&self, header: &[u8], tag: &[u8]) -> Result;
    }

    pub mod control {
        use super::*;

        /// Marker trait for keys to be used with stream control packets
        pub trait Stream: Control {
            fn retransmission_tag(
                &self,
                original_packet_number: u64,
                retransmission_packet_number: u64,
                tag_out: &mut [u8],
            ) -> Result;
        }

        pub mod stream {
            /// A no-op implementation for reliable transports
            #[derive(Clone, Default)]
            pub struct Reliable(());

            impl super::Control for Reliable {
                #[inline]
                fn tag_len(&self) -> usize {
                    16
                }

                #[inline]
                fn verify(&self, _header: &[u8], _tag: &[u8]) -> super::Result {
                    // this method should not be used on reliable transports
                    Err(super::Error::UnsupportedOperation)
                }
            }

            impl super::Stream for Reliable {
                #[inline]
                fn retransmission_tag(
                    &self,
                    _original_packet_number: u64,
                    _retransmission_packet_number: u64,
                    _tag_out: &mut [u8],
                ) -> super::Result {
                    // this method should not be used on reliable transports
                    Err(super::Error::UnsupportedOperation)
                }
            }
        }

        /// Marker trait for keys to be used with secret control packets
        pub trait Secret: Control {}
    }
}

pub trait IntoNonce {
    fn into_nonce(self) -> [u8; 12];
}

impl IntoNonce for u64 {
    #[inline]
    fn into_nonce(self) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&self.to_be_bytes());
        nonce
    }
}

impl IntoNonce for [u8; 12] {
    #[inline]
    fn into_nonce(self) -> [u8; 12] {
        self
    }
}
