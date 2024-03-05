// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]

use rustls::Error;

macro_rules! cert_type {
    ($name:ident, $trait:ident, $method:ident, $inner:ty) => {
        pub struct $name(pub(crate) $inner);

        pub trait $trait {
            fn $method(self) -> Result<$name, Error>;
        }

        impl $trait for $name {
            fn $method(self) -> Result<$name, Error> {
                Ok(self)
            }
        }

        impl $trait for String {
            fn $method(self) -> Result<$name, Error> {
                let cert = pem::$method(self.as_bytes())?;
                Ok($name(cert))
            }
        }

        impl $trait for &String {
            fn $method(self) -> Result<$name, Error> {
                let cert = pem::$method(self.as_bytes())?;
                Ok($name(cert))
            }
        }

        impl $trait for &str {
            fn $method(self) -> Result<$name, Error> {
                let cert = pem::$method(self.as_bytes())?;
                Ok($name(cert))
            }
        }

        impl $trait for Vec<u8> {
            fn $method(self) -> Result<$name, Error> {
                let cert = der::$method(self)?;
                Ok($name(cert))
            }
        }

        impl $trait for &[u8] {
            fn $method(self) -> Result<$name, Error> {
                self.to_vec().$method()
            }
        }

        impl $trait for &std::path::Path {
            fn $method(self) -> Result<$name, Error> {
                match self.extension() {
                    Some(ext) if ext == "der" => {
                        let pem =
                            std::fs::read(self).map_err(|err| Error::General(err.to_string()))?;
                        pem.$method()
                    }
                    _ => {
                        let pem = std::fs::read_to_string(self)
                            .map_err(|err| Error::General(err.to_string()))?;
                        pem.$method()
                    }
                }
            }
        }
    };
}

cert_type!(
    PrivateKey,
    IntoPrivateKey,
    into_private_key,
    rustls::pki_types::PrivateKeyDer<'static>
);
cert_type!(
    Certificate,
    IntoCertificate,
    into_certificate,
    Vec<rustls::pki_types::CertificateDer<'static>>
);

mod pem {
    use rustls::{
        pki_types::{CertificateDer, PrivateKeyDer},
        Error,
    };

    pub fn into_certificate(contents: &[u8]) -> Result<Vec<CertificateDer<'static>>, Error> {
        let mut cursor = std::io::Cursor::new(contents);
        let certs = rustls_pemfile::certs(&mut cursor)
            .map(|certs| certs.into_iter().map(CertificateDer::from).collect())
            .map_err(|_| Error::General("Could not read certificate".to_string()))?;
        Ok(certs)
    }

    fn construct_pkcs1_key(key: Vec<u8>) -> Result<PrivateKeyDer<'static>, Error> {
        Ok(PrivateKeyDer::Pkcs1(key.into()))
    }

    fn construct_pkcs8_key(key: Vec<u8>) -> Result<PrivateKeyDer<'static>, Error> {
        Ok(PrivateKeyDer::Pkcs8(key.into()))
    }

    pub fn into_private_key(contents: &[u8]) -> Result<PrivateKeyDer<'static>, Error> {
        let mut cursor = std::io::Cursor::new(contents);

        macro_rules! parse_key {
            ($parser:ident, $constructor:ident) => {
                cursor.set_position(0);

                match rustls_pemfile::$parser(&mut cursor) {
                    // try the next parser
                    Err(_) => (),
                    // try the next parser
                    Ok(keys) if keys.is_empty() => (),
                    Ok(mut keys) if keys.len() == 1 => {
                        return $constructor(keys.pop().unwrap());
                    }
                    Ok(keys) => {
                        return Err(Error::General(format!(
                            "Unexpected number of keys: {} (only 1 supported)",
                            keys.len()
                        )));
                    }
                }
            };
        }

        // attempt to parse PKCS8 encoded key. returns early if a key is found
        parse_key!(pkcs8_private_keys, construct_pkcs8_key);
        // attempt to parse RSA key. returns early if a key is found
        parse_key!(rsa_private_keys, construct_pkcs1_key);

        Err(Error::General(
            "could not load any valid private keys".to_string(),
        ))
    }
}

mod der {
    use rustls::{
        pki_types::{CertificateDer, PrivateKeyDer},
        Error,
    };

    pub fn into_certificate(contents: Vec<u8>) -> Result<Vec<CertificateDer<'static>>, Error> {
        // der files only have a single cert
        Ok(vec![CertificateDer::from(contents)])
    }

    pub fn into_private_key(contents: Vec<u8>) -> Result<PrivateKeyDer<'static>, Error> {
        Ok(PrivateKeyDer::Pkcs8(contents.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::crypto::tls::testing::certificates::*;

    #[test]
    fn load() {
        let _ = CERT_PEM.into_certificate().unwrap();
        let _ = CERT_DER.into_certificate().unwrap();

        let _ = KEY_PEM.into_private_key().unwrap();
        let _ = KEY_DER.into_private_key().unwrap();
    }
}
