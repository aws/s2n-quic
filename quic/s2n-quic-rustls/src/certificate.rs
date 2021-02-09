#![allow(dead_code)]

use crate::TLSError as Error;

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
                    Some(ext) if ext == "pem" => {
                        let pem = std::fs::read_to_string(self)
                            .map_err(|err| Error::General(err.to_string()))?;
                        pem.$method()
                    }
                    Some(ext) if ext == "der" => {
                        let pem =
                            std::fs::read(self).map_err(|err| Error::General(err.to_string()))?;
                        pem.$method()
                    }
                    Some(ext) => Err(Error::General(format!("unknown extension: {:?}", ext))),
                    None => Err(Error::General(
                        "cannot not infer certificate type without extension".to_string(),
                    )),
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

mod pem {
    use super::*;

    pub fn into_certificate(contents: &[u8]) -> Result<Vec<rustls::Certificate>, Error> {
        let mut cursor = std::io::Cursor::new(contents);
        let certs = rustls::internal::pemfile::certs(&mut cursor)
            .map_err(|_| Error::General("Could not read certificate".to_string()))?;
        Ok(certs)
    }

    pub fn into_private_key(contents: &[u8]) -> Result<rustls::PrivateKey, Error> {
        let mut cursor = std::io::Cursor::new(contents);

        let parsers = [
            rustls::internal::pemfile::rsa_private_keys,
            rustls::internal::pemfile::pkcs8_private_keys,
        ];

        for parser in parsers.iter() {
            cursor.set_position(0);

            match parser(&mut cursor) {
                Ok(keys) if keys.is_empty() => continue,
                Ok(mut keys) if keys.len() == 1 => return Ok(keys.pop().unwrap()),
                Ok(keys) => {
                    return Err(Error::General(format!(
                        "Unexpected number of keys: {} (only 1 supported)",
                        keys.len()
                    )));
                }
                // try the next parser
                Err(_) => continue,
            }
        }

        Err(Error::General(
            "could not load any valid private keys".to_string(),
        ))
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
