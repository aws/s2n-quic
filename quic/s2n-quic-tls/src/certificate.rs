// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::Bytes;
use s2n_tls::error::Error;

impl Format {
    pub fn as_pem(&self) -> Option<&[u8]> {
        if let Format::Pem(bytes) = &self {
            Some(bytes.as_ref())
        } else {
            None
        }
    }

    #[allow(dead_code)] // remove if s2n-tls ever starts supporting DER certs
    pub fn as_der(&self) -> Option<&[u8]> {
        if let Format::Der(bytes) = &self {
            Some(bytes.as_ref())
        } else {
            None
        }
    }
}

pub(crate) enum Format {
    Pem(Bytes),
    Der(Bytes),
    #[allow(dead_code)] // Only used if private key offloading supported
    None,
}

macro_rules! cert_type {
    ($name:ident, $trait:ident, $method:ident) => {
        pub struct $name(pub(crate) Format);

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
                let bytes = self.into_bytes();
                let bytes = Bytes::from(bytes);
                let bytes = Format::Pem(bytes);
                Ok($name(bytes))
            }
        }

        impl $trait for &String {
            fn $method(self) -> Result<$name, Error> {
                let bytes = self.as_bytes();
                let bytes = Bytes::copy_from_slice(bytes);
                let bytes = Format::Pem(bytes);
                Ok($name(bytes))
            }
        }

        impl $trait for &str {
            fn $method(self) -> Result<$name, Error> {
                let bytes = self.as_bytes();
                let bytes = Bytes::copy_from_slice(bytes);
                let bytes = Format::Pem(bytes);
                Ok($name(bytes))
            }
        }

        impl $trait for Vec<u8> {
            fn $method(self) -> Result<$name, Error> {
                let bytes = Bytes::from(self);
                let bytes = Format::Der(bytes);
                Ok($name(bytes))
            }
        }

        impl $trait for &[u8] {
            fn $method(self) -> Result<$name, Error> {
                let bytes = Bytes::copy_from_slice(self);
                let bytes = Format::Der(bytes);
                Ok($name(bytes))
            }
        }

        impl $trait for &std::path::Path {
            fn $method(self) -> Result<$name, Error> {
                match self.extension() {
                    Some(ext) if ext == "der" => {
                        let der = std::fs::read(self).map_err(|err| Error::io_error(err))?;
                        der.$method()
                    }
                    // assume it's in pem format
                    _ => {
                        let pem =
                            std::fs::read_to_string(self).map_err(|err| Error::io_error(err))?;
                        pem.$method()
                    }
                }
            }
        }
    };
}

cert_type!(PrivateKey, IntoPrivateKey, into_private_key);
cert_type!(Certificate, IntoCertificate, into_certificate);

#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_private_key")))]
pub const OFFLOAD_PRIVATE_KEY: PrivateKey = PrivateKey(Format::None);
