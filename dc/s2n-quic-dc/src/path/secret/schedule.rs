// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{Credentials, Id},
    crypto::awslc::{DecryptKey, EncryptKey},
};
use aws_lc_rs::{
    aead::{self, NONCE_LEN},
    hkdf::{self, Prk},
};
use s2n_quic_core::{dc, varint::VarInt};

pub use s2n_quic_core::endpoint;
pub const MAX_KEY_LEN: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum Ciphersuite {
    AES_GCM_128_SHA256,
    #[allow(dead_code)]
    AES_GCM_256_SHA384,
}

impl Ciphersuite {
    #[inline]
    pub fn aead(&self) -> &'static aead::Algorithm {
        match self {
            Self::AES_GCM_128_SHA256 => &aead::AES_128_GCM,
            Self::AES_GCM_256_SHA384 => &aead::AES_256_GCM,
        }
    }

    #[inline]
    pub fn hkdf(&self) -> hkdf::Algorithm {
        match self {
            Self::AES_GCM_128_SHA256 => hkdf::HKDF_SHA256,
            Self::AES_GCM_256_SHA384 => hkdf::HKDF_SHA384,
        }
    }
}

impl hkdf::KeyType for Ciphersuite {
    #[inline]
    fn len(&self) -> usize {
        match self {
            Self::AES_GCM_128_SHA256 => 16,
            Self::AES_GCM_256_SHA384 => 32,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Initiator {
    Local,
    Remote,
}

impl Initiator {
    #[inline]
    fn label(self, endpoint: endpoint::Type) -> &'static [u8] {
        use endpoint::Type::*;
        use Initiator::*;

        match (endpoint, self) {
            (Client, Local) | (Server, Remote) => b" client",
            (Server, Local) | (Client, Remote) => b" server",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Send,
    Receive,
}

impl Direction {
    #[inline]
    fn label(self, endpoint: endpoint::Type) -> &'static [u8] {
        use endpoint::Type::*;
        use Direction::*;

        match (endpoint, self) {
            (Client, Send) | (Server, Receive) => b" client",
            (Server, Send) | (Client, Receive) => b" server",
        }
    }
}

pub const EXPORT_SECRET_LEN: usize = 32;
pub type ExportSecret = [u8; 32];

#[derive(Debug)]
pub struct Secret {
    id: Id,
    prk: Prk,
    endpoint: endpoint::Type,
    ciphersuite: Ciphersuite,
}

impl Secret {
    #[inline]
    pub fn new(
        ciphersuite: Ciphersuite,
        _version: dc::Version,
        endpoint: endpoint::Type,
        export_secret: &ExportSecret,
    ) -> Self {
        let prk = Prk::new_less_safe(ciphersuite.hkdf(), export_secret);

        let mut v = Self {
            id: Default::default(),
            prk,
            endpoint,
            ciphersuite,
        };

        let mut id = Id::default();
        v.expand(&[&[16], b" pid"], &mut *id);
        v.id = id;

        v
    }

    #[inline]
    pub fn id(&self) -> &Id {
        &self.id
    }

    #[inline]
    pub fn application_pair(
        &self,
        key_id: VarInt,
        initiator: Initiator,
    ) -> (EncryptKey, DecryptKey) {
        let creds = Credentials {
            id: self.id,
            key_id,
        };

        let ciphersuite = &self.ciphersuite;
        let mut out = [0u8; (NONCE_LEN + MAX_KEY_LEN) * 2];
        let key_len = hkdf::KeyType::len(ciphersuite);
        let out_len = (NONCE_LEN + key_len) * 2;
        let (out, _) = out.split_at_mut(out_len);
        self.expand(
            &[
                &[out_len as u8],
                b" bidi",
                initiator.label(self.endpoint),
                &key_id.to_be_bytes(),
            ],
            out,
        );
        // if the hash is ever broken, it's better to put the "more secret" data at the beginning
        //
        // here we derive
        //
        // (client_key, server_key, client_iv, server_iv)
        let (client_key, out) = out.split_at(key_len);
        let (server_key, out) = out.split_at(key_len);
        let (client_iv, server_iv) = out.split_at(NONCE_LEN);
        let client_iv = client_iv.try_into().unwrap();
        let server_iv = server_iv.try_into().unwrap();
        let aead = ciphersuite.aead();

        match self.endpoint {
            endpoint::Type::Client => {
                let sealer = EncryptKey::new(creds, client_key, client_iv, aead);
                let opener = DecryptKey::new(creds, server_key, server_iv, aead);
                (sealer, opener)
            }
            endpoint::Type::Server => {
                let sealer = EncryptKey::new(creds, server_key, server_iv, aead);
                let opener = DecryptKey::new(creds, client_key, client_iv, aead);
                (sealer, opener)
            }
        }
    }

    #[inline]
    pub fn application_sealer(&self, key_id: VarInt) -> EncryptKey {
        let creds = Credentials {
            id: self.id,
            key_id,
        };

        self.derive_application_key(Direction::Send, key_id, |alg, key, iv| {
            EncryptKey::new(creds, key, iv, alg)
        })
    }

    #[inline]
    pub fn application_opener(&self, key_id: VarInt) -> DecryptKey {
        let creds = Credentials {
            id: self.id,
            key_id,
        };

        self.derive_application_key(Direction::Receive, key_id, |alg, key, iv| {
            DecryptKey::new(creds, key, iv, alg)
        })
    }

    #[inline]
    fn derive_application_key<F, R>(&self, direction: Direction, key_id: VarInt, f: F) -> R
    where
        F: FnOnce(&'static aead::Algorithm, &[u8], [u8; NONCE_LEN]) -> R,
    {
        let mut out = [0u8; NONCE_LEN + MAX_KEY_LEN];
        let key_len = hkdf::KeyType::len(&self.ciphersuite);
        let out_len = NONCE_LEN + key_len;
        let (out, _) = out.split_at_mut(out_len);
        self.expand(
            &[
                &[out_len as u8],
                b" uni",
                direction.label(self.endpoint),
                &key_id.to_be_bytes(),
            ],
            out,
        );
        // if the hash is ever broken, it's better to put the "more secret" data at the beginning
        let (key, iv) = out.split_at(key_len);
        let iv = iv.try_into().unwrap();
        f(self.ciphersuite.aead(), key, iv)
    }

    pub fn control_sealer(&self) -> EncryptKey {
        let creds = Credentials {
            id: *self.id(),
            key_id: VarInt::ZERO,
        };

        self.derive_control_key(Direction::Send, |alg, key, iv| {
            EncryptKey::new(creds, key, iv, alg)
        })
    }

    pub fn control_opener(&self) -> DecryptKey {
        let creds = Credentials {
            id: *self.id(),
            key_id: VarInt::ZERO,
        };

        self.derive_control_key(Direction::Receive, |alg, key, iv| {
            DecryptKey::new(creds, key, iv, alg)
        })
    }

    #[inline]
    fn derive_control_key<F, R>(&self, direction: Direction, f: F) -> R
    where
        F: FnOnce(&'static aead::Algorithm, &[u8], [u8; NONCE_LEN]) -> R,
    {
        let mut out = [0u8; NONCE_LEN + MAX_KEY_LEN];
        let key_len = hkdf::KeyType::len(&self.ciphersuite);
        let out_len = NONCE_LEN + key_len;
        let (out, _) = out.split_at_mut(out_len);
        self.expand(
            &[&[out_len as u8], b" ctl", direction.label(self.endpoint)],
            out,
        );
        // if the hash is ever broken, it's better to put the "more secret" data at the beginning
        let (key, iv) = out.split_at(key_len);
        let iv = iv.try_into().unwrap();
        f(self.ciphersuite.aead(), key, iv)
    }

    #[inline]
    fn expand(&self, label: &[&[u8]], out: &mut [u8]) {
        self.prk
            .expand(label, OutLen(out.len()))
            .unwrap()
            .fill(out)
            .unwrap();
    }
}

#[derive(Clone, Copy)]
pub struct OutLen(pub usize);

impl hkdf::KeyType for OutLen {
    #[inline]
    fn len(&self) -> usize {
        self.0
    }
}
