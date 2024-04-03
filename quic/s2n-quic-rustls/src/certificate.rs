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
                        let der =
                            std::fs::read(self).map_err(|err| Error::General(err.to_string()))?;
                        der.$method()
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

        rustls_pemfile::certs(&mut cursor)
            .map(|cert| cert.map_err(|_| Error::General("Could not read certificate".to_string())))
            .collect()
    }

    pub fn into_private_key(contents: &[u8]) -> Result<PrivateKeyDer<'static>, Error> {
        let mut cursor = std::io::Cursor::new(contents);

        macro_rules! parse_key {
            ($parser:ident, $key_type:expr) => {
                cursor.set_position(0);

                let keys: Result<Vec<_>, Error> = rustls_pemfile::$parser(&mut cursor)
                    .map(|key| {
                        key.map_err(|_| {
                            Error::General("Could not load any private keys".to_string())
                        })
                    })
                    .collect();
                match keys {
                    // try the next parser
                    Err(_) => (),
                    // try the next parser
                    Ok(keys) if keys.is_empty() => (),
                    Ok(mut keys) if keys.len() == 1 => {
                        return Ok($key_type(keys.pop().unwrap()));
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

        // attempt to parse PKCS8 encoded key. Returns early if a key is found
        parse_key!(pkcs8_private_keys, PrivateKeyDer::Pkcs8);
        // attempt to parse RSA key. Returns early if a key is found
        parse_key!(rsa_private_keys, PrivateKeyDer::Pkcs1);
        // attempt to parse a SEC1-encoded EC key. Returns early if a key is found
        parse_key!(ec_private_keys, PrivateKeyDer::Sec1);

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
        // PKCS #8 is used since it's capable of encoding RSA as well as other key
        // types (eg. ECDSA). Additionally, multiple attacks have been discovered
        // against PKCS #1 so PKCS #8 should be preferred.
        //
        // https://stackoverflow.com/a/48960291
        Ok(PrivateKeyDer::Pkcs8(contents.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::crypto::tls::testing::certificates::*;

    #[test]
    fn load_pem() {
        let _ = CERT_PEM.into_certificate().unwrap();
        let _ = CERT_PKCS1_PEM.into_certificate().unwrap();
        // PKCS #8 encoded key
        let _ = KEY_PEM.into_private_key().unwrap();
        // PKCS #1 encoded key
        let _ = KEY_PKCS1_PEM.into_private_key().unwrap();
    }

    #[test]
    fn load_der() {
        let _ = CERT_DER.into_certificate().unwrap();
        let _ = KEY_DER.into_private_key().unwrap();
    }
}
