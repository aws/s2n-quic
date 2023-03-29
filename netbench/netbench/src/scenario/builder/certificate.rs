// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::scenario;
use rcgen::SignatureAlgorithm;
use std::{collections::HashMap, sync::Arc};

static DEFAULT_ALG: &SignatureAlgorithm = &rcgen::PKCS_ECDSA_P256_SHA256;

#[derive(Clone, Debug, Hash)]
pub(crate) enum Certificate {
    Authority {
        alg: &'static SignatureAlgorithm,
    },
    PrivateKey {
        alg: &'static SignatureAlgorithm,
        authority: u64,
        intermediates: Vec<&'static SignatureAlgorithm>,
    },
    // Placeholder for a public cert
    Public,
}

fn create_ca(domain: &str, name: String, alg: &'static SignatureAlgorithm) -> rcgen::Certificate {
    use rcgen::{
        BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, IsCa,
        KeyUsagePurpose,
    };

    let mut params = CertificateParams::new(vec![domain.to_string()]);
    params.alg = alg;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(DnType::CountryName, "US");
    params.distinguished_name.push(DnType::CommonName, name);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    Certificate::from_params(params).unwrap()
}

fn create_cert(domain: &str, name: String, alg: &'static SignatureAlgorithm) -> rcgen::Certificate {
    use rcgen::{
        Certificate, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
        KeyUsagePurpose,
    };

    let mut params = CertificateParams::new(vec![domain.to_string(), format!("*.{domain}")]);
    params.alg = alg;
    params.use_authority_key_identifier_extension = true;
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(DnType::CountryName, "US");
    params.distinguished_name.push(DnType::CommonName, name);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
        ExtendedKeyUsagePurpose::ClientAuth,
    ];
    Certificate::from_params(params).unwrap()
}

impl Certificate {
    pub(crate) fn build_all(
        certs: Vec<Self>,
        id: &scenario::Id,
    ) -> Vec<Arc<scenario::Certificate>> {
        let mut cas = HashMap::new();
        let mut ias = HashMap::new();
        let mut out = vec![];

        let domain = format!("{id}.net");
        for (cert_idx, cert) in certs.into_iter().enumerate() {
            match cert {
                Self::Authority { alg } => {
                    let cert = create_ca(&domain, format!("netbench CA {cert_idx}"), alg);
                    let pem = cert.serialize_pem().unwrap();

                    out.push(Arc::new(scenario::Certificate {
                        pem,
                        pkcs12: vec![],
                    }));

                    cas.insert(cert_idx as u64, cert);
                }
                Self::PrivateKey {
                    alg,
                    authority,
                    intermediates,
                } => {
                    // create any IAs we need
                    for (idx, alg) in intermediates.iter().copied().enumerate() {
                        ias.entry((authority, idx, alg)).or_insert_with(|| {
                            create_ca(&domain, format!("netbench IA {authority} {idx}"), alg)
                        });
                    }

                    // create a reverse chain of authorities that need to sign this cert
                    let ca = cas.get(&authority).unwrap();
                    let authorities = intermediates
                        .iter()
                        .enumerate()
                        .rev()
                        .map(|(idx, alg)| ias.get(&(authority, idx, alg)).unwrap())
                        .chain(Some(ca));

                    let cert = create_cert(&domain, format!("netbench Leaf {cert_idx}"), alg);
                    let mut chain = String::new();
                    let private_key = cert.serialize_private_key_pem();

                    let mut current_cert = &cert;
                    for authority in authorities {
                        let public = current_cert.serialize_pem_with_signer(authority).unwrap();
                        chain.push_str(&public);
                        current_cert = authority;
                    }

                    let pkcs12 = {
                        let public = openssl::x509::X509::from_pem(chain.as_bytes()).unwrap();
                        let key = openssl::pkey::PKey::private_key_from_pem(private_key.as_bytes())
                            .unwrap();
                        openssl::pkcs12::Pkcs12::builder()
                            .pkey(&key)
                            .cert(&public)
                            .build2("")
                            .unwrap()
                            .to_der()
                            .unwrap()
                    };

                    out.push(Arc::new(scenario::Certificate {
                        pem: private_key,
                        pkcs12,
                    }));
                    out.push(Arc::new(scenario::Certificate {
                        pem: chain,
                        pkcs12: vec![],
                    }));
                }
                Self::Public => {
                    // noop - handled by private key
                }
            }
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct Authority {
    id: u64,
    state: super::State,
}

impl Authority {
    pub(crate) fn new<F: FnOnce(&mut AuthorityBuilder)>(state: super::State, f: F) -> Self {
        let mut builder = AuthorityBuilder { alg: DEFAULT_ALG };
        f(&mut builder);

        let id = state
            .certificates
            .push(Certificate::Authority { alg: builder.alg }) as u64;

        Self { id, state }
    }

    pub fn key_pair(&self) -> KeyPair {
        self.key_pair_with(|_| {})
    }

    pub fn key_pair_with<F: FnOnce(&mut KeyPairBuilder)>(&self, f: F) -> KeyPair {
        let mut builder = KeyPairBuilder {
            authority: self.id,
            intermediates: vec![],
            alg: DEFAULT_ALG,
        };

        f(&mut builder);

        let KeyPairBuilder {
            authority,
            intermediates,
            alg,
        } = builder;

        let private_key = self.state.certificates.push(Certificate::PrivateKey {
            alg,
            authority,
            intermediates,
        }) as u64;
        let certificate = self.state.certificates.push(Certificate::Public) as u64;

        KeyPair {
            private_key,
            certificate,
            authority,
        }
    }
}

#[derive(Debug)]
pub struct AuthorityBuilder {
    alg: &'static SignatureAlgorithm,
}

macro_rules! authority {
    ($(($alg:ident, $lower:ident)),* $(,)?) => {
        impl AuthorityBuilder {
            $(
                pub fn $lower(&mut self) -> &mut Self {
                    self.alg = &rcgen::$alg;
                    self
                }
            )*
        }
    };
}

authority!(
    (PKCS_ECDSA_P256_SHA256, ecdsa),
    (PKCS_ED25519, ed25519),
    (PKCS_RSA_SHA256, rsa_256),
    (PKCS_RSA_SHA384, rsa_384),
    (PKCS_RSA_SHA512, rsa_512),
);

#[derive(Copy, Clone, Debug)]
pub struct KeyPair {
    pub(crate) authority: u64,
    pub(crate) private_key: u64,
    pub(crate) certificate: u64,
}

#[derive(Debug)]
pub struct KeyPairBuilder {
    authority: u64,
    intermediates: Vec<&'static SignatureAlgorithm>,
    alg: &'static SignatureAlgorithm,
}

impl KeyPairBuilder {
    pub fn push_ia(&mut self) -> &mut Self {
        self.push_ia_with(|_| {})
    }

    pub fn push_ia_with<F: FnOnce(&mut AuthorityBuilder)>(&mut self, f: F) -> &mut Self {
        let mut builder = AuthorityBuilder { alg: self.alg };
        f(&mut builder);
        self.intermediates.push(builder.alg);
        self
    }
}
