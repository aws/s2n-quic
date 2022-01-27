// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use s2n_quic::provider::tls::{
    rustls::certificate::{Certificate as RustlsCertificate, PrivateKey as RustlsPrivateKey},
    s2n_tls::certificate::{Certificate as S2nCertificate, PrivateKey as S2nPrivateKey},
};
use std::{path::PathBuf, str::FromStr};

#[derive(Clone, Copy, Debug)]
pub enum TlsProviders {
    /// Use s2n-tls as the tls provider
    S2N,
    /// Use rustls as the tls provider
    Rustls,
}

impl FromStr for TlsProviders {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use TlsProviders::*;

        Ok(match s {
            "rustls" => Rustls,
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

pub fn s2n_ca(ca: Option<&PathBuf>) -> Result<S2nCertificate> {
    use s2n_quic::provider::tls::s2n_tls::certificate::IntoCertificate;
    Ok(if let Some(pathbuf) = ca.as_ref() {
        pathbuf.into_certificate()?
    } else {
        s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.into_certificate()?
    })
}

pub fn rustls_ca(ca: Option<&PathBuf>) -> Result<RustlsCertificate> {
    use s2n_quic::provider::tls::rustls::certificate::IntoCertificate;
    Ok(if let Some(pathbuf) = ca.as_ref() {
        pathbuf.into_certificate()?
    } else {
        s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.into_certificate()?
    })
}

pub fn s2n_private_key(private_key: Option<&PathBuf>) -> Result<S2nPrivateKey> {
    use s2n_quic::provider::tls::s2n_tls::certificate::IntoPrivateKey;
    Ok(if let Some(pathbuf) = private_key.as_ref() {
        pathbuf.into_private_key()?
    } else {
        s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.into_private_key()?
    })
}

pub fn rustls_private_key(private_key: Option<&PathBuf>) -> Result<RustlsPrivateKey> {
    use s2n_quic::provider::tls::rustls::certificate::IntoPrivateKey;
    Ok(if let Some(pathbuf) = private_key.as_ref() {
        pathbuf.into_private_key()?
    } else {
        s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.into_private_key()?
    })
}
