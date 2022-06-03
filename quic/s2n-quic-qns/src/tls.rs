// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use s2n_quic::provider::tls::rustls::certificate::{
    Certificate as RustlsCertificate, PrivateKey as RustlsPrivateKey,
};
use std::{path::PathBuf, str::FromStr};

#[derive(Clone, Copy, Debug)]
pub enum TlsProviders {
    /// Use s2n-tls as the tls provider
    #[cfg(unix)]
    S2N,
    /// Use rustls as the tls provider
    Rustls,
}

impl Default for TlsProviders {
    fn default() -> Self {
        #[cfg(unix)]
        return Self::S2N;
        #[cfg(not(unix))]
        return Self::Rustls;
    }
}

impl ToString for TlsProviders {
    fn to_string(&self) -> String {
        match self {
            #[cfg(unix)]
            TlsProviders::S2N => String::from("s2n-tls"),
            TlsProviders::Rustls => String::from("rustls"),
        }
    }
}

impl FromStr for TlsProviders {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use TlsProviders::*;

        Ok(match s {
            "rustls" => Rustls,
            #[cfg(unix)]
            "s2n-tls" => S2N,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Unsupported tls provider: {}", s),
                )
                .into())
            }
        })
    }
}

pub mod default {
    #[cfg(not(unix))]
    pub use super::rustls::*;
    #[cfg(unix)]
    pub use super::s2n::*;
}

#[cfg(unix)]
pub mod s2n {
    use super::*;
    use s2n_quic::provider::tls::s2n_tls::certificate::{
        Certificate as S2nCertificate, PrivateKey as S2nPrivateKey,
    };

    pub fn ca(ca: Option<&PathBuf>) -> Result<S2nCertificate> {
        use s2n_quic::provider::tls::s2n_tls::certificate::IntoCertificate;
        Ok(if let Some(pathbuf) = ca.as_ref() {
            pathbuf.into_certificate()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.into_certificate()?
        })
    }

    pub fn private_key(private_key: Option<&PathBuf>) -> Result<S2nPrivateKey> {
        use s2n_quic::provider::tls::s2n_tls::certificate::IntoPrivateKey;
        Ok(if let Some(pathbuf) = private_key.as_ref() {
            pathbuf.into_private_key()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.into_private_key()?
        })
    }
}

pub mod rustls {
    use super::*;

    pub fn ca(ca: Option<&PathBuf>) -> Result<RustlsCertificate> {
        use s2n_quic::provider::tls::rustls::certificate::IntoCertificate;
        Ok(if let Some(pathbuf) = ca.as_ref() {
            pathbuf.into_certificate()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.into_certificate()?
        })
    }

    pub fn private_key(private_key: Option<&PathBuf>) -> Result<RustlsPrivateKey> {
        use s2n_quic::provider::tls::rustls::certificate::IntoPrivateKey;
        Ok(if let Some(pathbuf) = private_key.as_ref() {
            pathbuf.into_private_key()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.into_private_key()?
        })
    }
}
