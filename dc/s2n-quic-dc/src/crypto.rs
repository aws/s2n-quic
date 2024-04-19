use crate::credentials::Credentials;
pub use bytes::buf::UninitSlice;
use core::fmt;
pub use s2n_quic_core::packet::KeyPhase;

pub mod awslc;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub mod encrypt {
    use super::*;

    pub trait Key {
        fn credentials(&self) -> &Credentials;

        fn tag_len(&self) -> usize;

        /// Encrypt a payload
        fn encrypt<N: IntoNonce>(
            &self,
            nonce: N,
            header: &[u8],
            extra_payload: Option<&[u8]>,
            payload_and_tag: &mut [u8],
        );

        fn retransmission_tag(
            &self,
            original_packet_number: u64,
            retransmission_packet_number: u64,
            tag_out: &mut [u8],
        );
    }
}

pub mod decrypt {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    pub enum Error {
        ReplayPotentiallyDetected,
        ReplayDefinitelyDetected,
        InvalidTag,
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self {
                Self::ReplayDefinitelyDetected => "key replay prevented",
                Self::ReplayPotentiallyDetected => "key replay potentially detected",
                Self::InvalidTag => "invalid tag",
            }
            .fmt(f)
        }
    }

    impl std::error::Error for Error {}

    pub type Result<T = (), E = Error> = core::result::Result<T, E>;

    pub trait Key {
        fn credentials(&self) -> &Credentials;

        fn tag_len(&self) -> usize;

        /// Decrypt a payload
        fn decrypt<N: IntoNonce>(
            &mut self,
            nonce: N,
            header: &[u8],
            payload_in: &[u8],
            tag: &[u8],
            payload_out: &mut UninitSlice,
        ) -> Result;

        /// Decrypt a payload
        fn decrypt_in_place<N: IntoNonce>(
            &mut self,
            nonce: N,
            header: &[u8],
            payload_and_tag: &mut [u8],
        ) -> Result;

        fn retransmission_tag(
            &mut self,
            original_packet_number: u64,
            retransmission_packet_number: u64,
            tag_out: &mut [u8],
        );
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
