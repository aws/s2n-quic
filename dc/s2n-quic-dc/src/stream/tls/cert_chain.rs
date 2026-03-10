// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Wraps the `s2n_tls::cert_chain` module to avoid placing those s2n-tls types in the public API.
//!
//! Unlike the s2n-tls configuration which is shared across connections, once a connection is
//! established we'd expect to drop the s2n-tls Connection struct eventually. So we'll need our own
//! container for any information that we wish to retain after the connection ends.

#[derive(Clone)]
pub struct CertificateChain {
    der_certs: Vec<Vec<u8>>,
}

impl CertificateChain {
    pub(crate) fn new(
        chain: s2n_tls::cert_chain::CertificateChain<'_>,
    ) -> Result<Self, s2n_tls::error::Error> {
        let mut der_certs = Vec::with_capacity(chain.len());
        for cert in chain.iter() {
            der_certs.push(cert?.der()?.to_vec());
        }
        Ok(Self { der_certs })
    }

    pub fn iter_der(&self) -> impl Iterator<Item = &'_ [u8]> + '_ {
        self.der_certs.iter().map(|v| &v[..])
    }
}
