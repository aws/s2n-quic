// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Error;

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
                        let pem = std::fs::read(self)?;
                        pem.$method()
                    }
                    _ => {
                        let pem = std::fs::read_to_string(self)?;
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
    rustls::PrivateKey
);
cert_type!(
    Certificate,
    IntoCertificate,
    into_certificate,
    Vec<rustls::Certificate>
);

impl IntoCertificate for Vec<Vec<u8>> {
    fn into_certificate(self) -> Result<Certificate, Error> {
        let mut certs = vec![];
        for der in self {
            let der = der.into_certificate()?;
            certs.extend(der.0);
        }
        Ok(Certificate(certs))
    }
}

mod pem {
    use super::*;

    pub fn into_certificate(contents: &[u8]) -> Result<Vec<rustls::Certificate>, Error> {
        let mut cursor = std::io::Cursor::new(contents);
        let certs = rustls_pemfile::certs(&mut cursor)
            .map(|certs| certs.into_iter().map(rustls::Certificate).collect())?;
        Ok(certs)
    }

    pub fn into_private_key(contents: &[u8]) -> Result<rustls::PrivateKey, Error> {
        let mut cursor = std::io::Cursor::new(contents);

        let parsers = [
            rustls_pemfile::rsa_private_keys,
            rustls_pemfile::pkcs8_private_keys,
            rustls_pemfile::ec_private_keys,
        ];

        for parser in parsers.iter() {
            cursor.set_position(0);

            match parser(&mut cursor) {
                Ok(keys) if keys.is_empty() => continue,
                Ok(mut keys) if keys.len() == 1 => {
                    return Ok(rustls::PrivateKey(keys.pop().unwrap()))
                }
                Ok(keys) => {
                    return Err(rustls::Error::General(format!(
                        "Unexpected number of keys: {} (only 1 supported)",
                        keys.len()
                    ))
                    .into());
                }
                // try the next parser
                Err(_) => continue,
            }
        }

        Err(rustls::Error::General("could not load any valid private keys".to_string()).into())
    }
}

mod der {
    use super::*;

    pub fn into_certificate(contents: Vec<u8>) -> Result<Vec<rustls::Certificate>, Error> {
        // der files only have a single cert
        Ok(vec![rustls::Certificate(contents)])
    }

    pub fn into_private_key(contents: Vec<u8>) -> Result<rustls::PrivateKey, Error> {
        Ok(rustls::PrivateKey(contents))
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
