use crate::{ciphersuite::NegotiatedCiphersuite as Ciphersuite, Algorithm, SecretPair};
use s2n_quic_core::{
    crypto::{CryptoError, HeaderCrypto, HeaderProtectionMask, Key},
    endpoint,
};

#[derive(Debug)]
pub struct RingNegotiatedCrypto {
    sealer: Ciphersuite,
    opener: Ciphersuite,
}

impl RingNegotiatedCrypto {
    /// Create a server ciphersuite with a given negotiated algorithm and secret
    pub fn new_server(algorithm: &Algorithm, secrets: SecretPair) -> Option<Self> {
        Self::new(endpoint::Type::Server, algorithm, secrets)
    }

    /// Create a client ciphersuite with a given negotiated algorithm and secret
    pub fn new_client(algorithm: &Algorithm, secrets: SecretPair) -> Option<Self> {
        Self::new(endpoint::Type::Client, algorithm, secrets)
    }

    fn new(endpoint: endpoint::Type, algorithm: &Algorithm, secrets: SecretPair) -> Option<Self> {
        let (sealer_secret, opener_secret) = match endpoint {
            endpoint::Type::Client => (secrets.client, secrets.server),
            endpoint::Type::Server => (secrets.server, secrets.client),
        };

        let sealer = Ciphersuite::new(algorithm, sealer_secret)?;
        let opener = Ciphersuite::new(algorithm, opener_secret)?;

        Some(Self { sealer, opener })
    }

    /// Update the ciphersuite as defined in
    /// https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6
    pub fn update(&self) -> Self {
        Self {
            sealer: self.sealer.update(),
            opener: self.opener.update(),
        }
    }
}

impl Key for RingNegotiatedCrypto {
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.opener.decrypt(packet_number, header, payload)
    }

    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.sealer.encrypt(packet_number, header, payload)
    }

    fn tag_len(&self) -> usize {
        self.sealer.tag_len()
    }

    fn aead_confidentiality_limit(&self) -> u64 {
        self.sealer.aead_confidentiality_limit()
    }

    fn aead_integrity_limit(&self) -> u64 {
        self.opener.aead_integrity_limit()
    }
}

impl HeaderCrypto for RingNegotiatedCrypto {
    fn opening_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.opener.opening_header_protection_mask(sample)
    }

    fn opening_sample_len(&self) -> usize {
        self.opener.opening_sample_len()
    }

    fn sealing_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.sealer.sealing_header_protection_mask(sample)
    }

    fn sealing_sample_len(&self) -> usize {
        self.sealer.sealing_sample_len()
    }
}

macro_rules! negotiated_crypto {
    ($name:ident) => {
        #[derive(Debug)]
        pub struct $name(crate::negotiated::RingNegotiatedCrypto);

        impl $name {
            /// Create a server ciphersuite with a given negotiated algorithm and secret
            pub fn new_server(
                algorithm: &$crate::Algorithm,
                secrets: $crate::SecretPair,
            ) -> Option<Self> {
                Some(Self(crate::negotiated::RingNegotiatedCrypto::new_server(
                    algorithm, secrets,
                )?))
            }

            /// Create a client ciphersuite with a given negotiated algorithm and secret
            pub fn new_client(
                algorithm: &$crate::Algorithm,
                secrets: $crate::SecretPair,
            ) -> Option<Self> {
                Some(Self(crate::negotiated::RingNegotiatedCrypto::new_client(
                    algorithm, secrets,
                )?))
            }

            /// Update the ciphersuite as defined in
            /// https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6
            pub fn update(&self) -> Self {
                Self(self.0.update())
            }
        }

        impl s2n_quic_core::crypto::Key for $name {
            fn decrypt(
                &self,
                packet_number: u64,
                header: &[u8],
                payload: &mut [u8],
            ) -> Result<(), s2n_quic_core::crypto::CryptoError> {
                self.0.decrypt(packet_number, header, payload)
            }

            fn encrypt(
                &self,
                packet_number: u64,
                header: &[u8],
                payload: &mut [u8],
            ) -> Result<(), s2n_quic_core::crypto::CryptoError> {
                self.0.encrypt(packet_number, header, payload)
            }

            fn tag_len(&self) -> usize {
                self.0.tag_len()
            }

            fn aead_confidentiality_limit(&self) -> u64 {
                self.0.aead_confidentiality_limit()
            }

            fn aead_integrity_limit(&self) -> u64 {
                self.0.aead_integrity_limit()
            }
        }

        impl s2n_quic_core::crypto::HeaderCrypto for $name {
            fn opening_header_protection_mask(
                &self,
                sample: &[u8],
            ) -> s2n_quic_core::crypto::HeaderProtectionMask {
                self.0.opening_header_protection_mask(sample)
            }

            fn opening_sample_len(&self) -> usize {
                self.0.opening_sample_len()
            }

            fn sealing_header_protection_mask(
                &self,
                sample: &[u8],
            ) -> s2n_quic_core::crypto::HeaderProtectionMask {
                self.0.sealing_header_protection_mask(sample)
            }

            fn sealing_sample_len(&self) -> usize {
                self.0.sealing_sample_len()
            }
        }
    };
}
