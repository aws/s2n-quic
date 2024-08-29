// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Id,
    crypto::awslc::{open, seal},
};
use aws_lc_rs::{
    aead::{self, NONCE_LEN},
    hkdf::{self, Prk},
    hmac,
};
use s2n_quic_core::{dc, varint::VarInt};

pub use s2n_quic_core::endpoint;
pub const MAX_KEY_LEN: usize = 32;
const MAX_HMAC_KEY_LEN: usize = 1024 / 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
#[allow(non_camel_case_types)]
pub enum Ciphersuite {
    AES_GCM_128_SHA256,
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

    #[inline]
    pub fn hmac(&self) -> &'static hmac::Algorithm {
        match self {
            Self::AES_GCM_128_SHA256 => &hmac::HMAC_SHA256,
            Self::AES_GCM_256_SHA384 => &hmac::HMAC_SHA384,
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
        v.prk.expand_into(&[&[16], b" pid"], &mut *id);
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
    ) -> (seal::Application, SealUpdate, open::Application, OpenUpdate) {
        let ciphersuite = &self.ciphersuite;
        let mut out = [0u8; (NONCE_LEN + MAX_KEY_LEN) * 2 + MAX_KEY_LEN * 2];
        let key_len = hkdf::KeyType::len(ciphersuite);
        let out_len = (NONCE_LEN + key_len) * 2 + key_len * 2;

        debug_assert!(out_len <= u16::MAX as usize);

        let (out, _) = out.split_at_mut(out_len);
        self.prk.expand_into(
            &[
                &(out_len as u16).to_be_bytes(),
                b" bidi",
                initiator.label(self.endpoint),
                b" app",
                &key_id.to_be_bytes(),
            ],
            out,
        );

        // if the hash is ever broken, it's better to put the "more secret" data at the beginning
        //
        // here we derive:
        //
        // (client_ku, server_ku, client_key, server_key, client_iv, server_iv)
        let (client_ku, out) = out.split_at(key_len);
        let (server_ku, out) = out.split_at(key_len);
        let (client_key, out) = out.split_at(key_len);
        let (server_key, out) = out.split_at(key_len);
        let (client_iv, server_iv) = out.split_at(NONCE_LEN);
        let client_iv = client_iv.try_into().unwrap();
        let server_iv = server_iv.try_into().unwrap();
        let aead = ciphersuite.aead();

        let (sealer_ku, opener_ku, sealer_key, opener_key, sealer_iv, opener_iv) =
            match self.endpoint {
                endpoint::Type::Client => (
                    client_ku, server_ku, client_key, server_key, client_iv, server_iv,
                ),
                endpoint::Type::Server => (
                    server_ku, client_ku, server_key, client_key, server_iv, client_iv,
                ),
            };

        let sealer = seal::Application::new(sealer_key, sealer_iv, aead);
        let sealer_ku = SealUpdate::new(sealer_ku, ciphersuite);
        let opener = open::Application::new(opener_key, opener_iv, aead);
        let opener_ku = OpenUpdate::new(opener_ku, ciphersuite);
        (sealer, sealer_ku, opener, opener_ku)
    }

    #[inline]
    pub fn control_pair(
        &self,
        key_id: VarInt,
        initiator: Initiator,
    ) -> (seal::control::Stream, open::control::Stream) {
        let ciphersuite = &self.ciphersuite;
        let mut out = [0u8; MAX_HMAC_KEY_LEN * 2];
        let key_len = {
            // Use the block length for the key, instead of output length for stronger security and to
            // avoid padding.

            //= https://www.rfc-editor.org/rfc/rfc2104.html#section-2
            //# The authentication key K can be of any length up to B, the
            //# block length of the hash function.  Applications that use keys longer
            //# than B bytes will first hash the key using H and then use the
            //# resultant L byte string as the actual key to HMAC. In any case the
            //# minimal recommended length for K is L bytes (as the hash output
            //# length).
            ciphersuite.hmac().digest_algorithm().block_len()
        };
        let out_len = key_len * 2;

        debug_assert!(out_len <= u16::MAX as usize);

        let (out, _) = out.split_at_mut(out_len);
        self.prk.expand_into(
            &[
                &(out_len as u16).to_be_bytes(),
                b" bidi",
                initiator.label(self.endpoint),
                b" ctl",
                &key_id.to_be_bytes(),
            ],
            out,
        );

        let (client_key, server_key) = out.split_at(key_len);
        let hmac = ciphersuite.hmac();

        let (sealer_key, opener_key) = match self.endpoint {
            endpoint::Type::Client => (client_key, server_key),
            endpoint::Type::Server => (server_key, client_key),
        };

        let sealer = seal::control::Stream::new(sealer_key, hmac);
        let opener = open::control::Stream::new(opener_key, hmac);
        (sealer, opener)
    }

    #[inline]
    pub fn application_sealer(&self, key_id: VarInt) -> seal::Application {
        self.derive_application_key(Direction::Send, key_id, |alg, key, iv| {
            seal::Application::new(key, iv, alg)
        })
    }

    #[inline]
    pub fn application_opener(&self, key_id: VarInt) -> open::Application {
        self.derive_application_key(Direction::Receive, key_id, |alg, key, iv| {
            open::Application::new(key, iv, alg)
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
        debug_assert!(out_len <= u16::MAX as usize);

        let (out, _) = out.split_at_mut(out_len);
        self.prk.expand_into(
            &[
                &(out_len as u16).to_be_bytes(),
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

    pub fn control_sealer(&self) -> seal::control::Secret {
        self.derive_control_key(Direction::Send, seal::control::Secret::new)
    }

    pub fn control_opener(&self) -> open::control::Secret {
        self.derive_control_key(Direction::Receive, open::control::Secret::new)
    }

    #[inline]
    fn derive_control_key<F, R>(&self, direction: Direction, f: F) -> R
    where
        F: FnOnce(&[u8], &'static hmac::Algorithm) -> R,
    {
        let mut out = [0u8; MAX_HMAC_KEY_LEN];
        let key_len = {
            // Use the block length for the key, instead of output length for stronger security and to
            // avoid padding.

            //= https://www.rfc-editor.org/rfc/rfc2104.html#section-2
            //# The authentication key K can be of any length up to B, the
            //# block length of the hash function.  Applications that use keys longer
            //# than B bytes will first hash the key using H and then use the
            //# resultant L byte string as the actual key to HMAC. In any case the
            //# minimal recommended length for K is L bytes (as the hash output
            //# length).
            self.ciphersuite.hmac().digest_algorithm().block_len()
        };

        let out_len = key_len;
        debug_assert!(out_len <= u16::MAX as usize);

        let (out, _) = out.split_at_mut(out_len);
        self.prk.expand_into(
            &[
                &(out_len as u16).to_be_bytes(),
                b" ctl",
                direction.label(self.endpoint),
            ],
            out,
        );
        f(out, self.ciphersuite.hmac())
    }
}

trait PrkExt {
    fn expand_into(&self, label: &[&[u8]], out: &mut [u8]);
}

impl PrkExt for Prk {
    #[inline]
    fn expand_into(&self, label: &[&[u8]], out: &mut [u8]) {
        self.expand(label, OutLen(out.len()))
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

#[derive(Debug)]
pub struct SealUpdate(Updater);

impl SealUpdate {
    #[inline]
    pub fn new(secret: &[u8], ciphersuite: &Ciphersuite) -> Self {
        Self(Updater::new(secret, ciphersuite))
    }

    #[inline]
    pub fn next(&self) -> (seal::Application, SealUpdate) {
        self.0.next(|key, iv, updater| {
            let key = seal::Application::new(key, iv, updater.ciphersuite.aead());
            (key, Self(updater))
        })
    }
}

#[derive(Debug)]
pub struct OpenUpdate(Updater);

impl OpenUpdate {
    #[inline]
    pub fn new(secret: &[u8], ciphersuite: &Ciphersuite) -> Self {
        Self(Updater::new(secret, ciphersuite))
    }

    #[inline]
    pub fn next(&self) -> (open::Application, OpenUpdate) {
        self.0.next(|key, iv, updater| {
            let key = open::Application::new(key, iv, updater.ciphersuite.aead());
            (key, Self(updater))
        })
    }
}

#[derive(Debug)]
struct Updater {
    prk: Prk,
    ciphersuite: Ciphersuite,
}

impl Updater {
    #[inline]
    fn new(secret: &[u8], ciphersuite: &Ciphersuite) -> Self {
        let prk = Prk::new_less_safe(ciphersuite.hkdf(), secret);
        let ciphersuite = *ciphersuite;
        Self { prk, ciphersuite }
    }

    #[inline]
    fn next<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[u8], [u8; NONCE_LEN], Updater) -> R,
    {
        let ciphersuite = &self.ciphersuite;

        let mut out = [0u8; NONCE_LEN + MAX_KEY_LEN * 2];
        let key_len = hkdf::KeyType::len(ciphersuite);
        let out_len = NONCE_LEN + key_len * 2;
        let (out, _) = out.split_at_mut(out_len);
        self.prk
            .expand_into(&[&(out_len as u16).to_be_bytes(), b" ku"], out);

        // if the hash is ever broken, it's better to put the "more secret" data at the beginning
        //
        // here we derive:
        //
        // (key_update, key, iv)
        let (ku, out) = out.split_at(key_len);
        let (key, iv) = out.split_at(key_len);
        let iv = iv.try_into().unwrap();

        let ku = Self::new(ku, ciphersuite);

        f(key, iv, ku)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::secret::{
        map::Dedup, open::Application as Opener, seal::Application as Sealer,
    };
    use bolero::*;

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    struct Pair {
        ciphersuite: Ciphersuite,
        key_id: VarInt,
        initiator_is_client: bool,
    }

    impl Pair {
        fn initiator(&self) -> endpoint::Type {
            if self.initiator_is_client {
                endpoint::Type::Client
            } else {
                endpoint::Type::Server
            }
        }

        fn endpoints(&self) -> (Secret, Secret) {
            let secret = &[42; 32];
            let client = Secret::new(self.ciphersuite, 0, endpoint::Type::Client, secret);
            let server = Secret::new(self.ciphersuite, 0, endpoint::Type::Server, secret);
            (client, server)
        }

        fn check_app(self) {
            let (client, server) = self.endpoints();
            let (client_i, server_i) = match self.initiator() {
                endpoint::Type::Client => (Initiator::Local, Initiator::Remote),
                endpoint::Type::Server => (Initiator::Remote, Initiator::Local),
            };
            let mut client_app = Application::new(client.application_pair(self.key_id, client_i));
            let mut server_app = Application::new(server.application_pair(self.key_id, server_i));

            for i in 0..8 {
                client_app.send(&server_app).unwrap();
                server_app.send(&client_app).unwrap();

                // invalid sender/recipient should fail
                client_app.send(&client_app).unwrap_err();
                server_app.send(&server_app).unwrap_err();

                let (sender, receiver) = if i % 2 == 0 {
                    dbg!("client ku");
                    (&mut client_app, &mut server_app)
                } else {
                    dbg!("server ku");
                    (&mut server_app, &mut client_app)
                };

                sender.sealer.update();
                sender.send(receiver).unwrap();

                assert!(receiver.opener.needs_update());
                receiver.opener.update();
            }
        }

        fn check_control(self) {
            let (client, server) = self.endpoints();
            let (client_i, server_i) = match self.initiator() {
                endpoint::Type::Client => (Initiator::Local, Initiator::Remote),
                endpoint::Type::Server => (Initiator::Remote, Initiator::Local),
            };
            let client_app = Control::new(client.control_pair(self.key_id, client_i));
            let server_app = Control::new(server.control_pair(self.key_id, server_i));

            client_app.send(&server_app).unwrap();
            server_app.send(&client_app).unwrap();

            // invalid sender/recipient should fail
            client_app.send(&client_app).unwrap_err();
            server_app.send(&server_app).unwrap_err();
        }
    }

    struct Application {
        sealer: Sealer,
        opener: Opener,
    }

    impl Application {
        fn new(
            (sealer, sealer_ku, opener, opener_ku): (
                seal::Application,
                SealUpdate,
                open::Application,
                OpenUpdate,
            ),
        ) -> Self {
            let sealer = Sealer::new(sealer, sealer_ku);
            let opener = Opener::new(opener, opener_ku, Dedup::disabled());
            Self { sealer, opener }
        }

        fn send(&self, other: &Self) -> crate::crypto::open::Result {
            use crate::crypto::{open::Application as _, seal::Application as _};

            let msg = b"hello";
            let mut buf = [0u8; 5 + 16];

            let packet_number = 0u64;
            let header = &[];

            let key_phase = self.sealer.key_phase();
            self.sealer
                .encrypt(packet_number, header, Some(msg), &mut buf);

            assert_ne!(msg, &buf[..5]);

            other
                .opener
                .decrypt_in_place(key_phase, packet_number, header, &mut buf)?;

            assert_eq!(msg, &buf[..5]);

            Ok(())
        }
    }

    struct Control {
        sealer: seal::control::Stream,
        opener: open::control::Stream,
    }

    impl Control {
        fn new((sealer, opener): (seal::control::Stream, open::control::Stream)) -> Self {
            Self { sealer, opener }
        }

        fn send(&self, other: &Self) -> crate::crypto::open::Result {
            use crate::crypto::{open::Control as _, seal::Control as _};

            let msg = b"hello";
            let mut tag = [0u8; crate::packet::secret_control::TAG_LEN];

            self.sealer.sign(msg, &mut tag);

            other.opener.verify(msg, &tag)?;

            Ok(())
        }
    }

    #[test]
    fn application_pair() {
        bolero::check!()
            .with_type::<Pair>()
            .for_each(|input| input.check_app())
    }

    #[test]
    fn control_pair() {
        bolero::check!()
            .with_type::<Pair>()
            .for_each(|input| input.check_control())
    }
}
