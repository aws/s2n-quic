// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    certificate::{IntoCertificate, IntoPrivateKey},
    keylog::KeyLogHandle,
    params::Params,
    session::Session,
    ConfigLoader,
};
use s2n_codec::EncoderValue;
use s2n_quic_core::{application::ServerName, crypto::tls, endpoint};
use s2n_tls::{
    callbacks::VerifyHostNameCallback,
    config::{self, Config},
    enums::ClientAuthType,
    error::Error,
};
use std::sync::Arc;

pub struct Client<L: ConfigLoader = Config> {
    loader: L,
    #[allow(dead_code)] // we need to hold on to the handle to ensure it is cleaned up correctly
    keylog: Option<KeyLogHandle>,
    params: Params,
}

impl Client {
    pub fn builder() -> Builder {
        Builder::default()
    }
}

impl<L: ConfigLoader> Client<L> {
    /// Creates a [`Client`] from a [`ConfigLoader`]
    ///
    /// The caller is responsible for building the `Config`
    /// correctly for QUIC settings. This includes:
    /// * setting a security policy that supports TLS 1.3
    /// * enabling QUIC support
    /// * setting at least one application protocol
    pub fn from_loader(loader: L) -> Self {
        Self {
            loader,
            keylog: None,
            params: Default::default(),
        }
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("could not create a default client")
    }
}

impl<L: ConfigLoader> ConfigLoader for Client<L> {
    #[inline]
    fn load(&mut self, cx: crate::ConnectionContext) -> s2n_tls::config::Config {
        self.loader.load(cx)
    }
}

pub struct Builder {
    config: config::Builder,
    keylog: Option<KeyLogHandle>,
}

impl Default for Builder {
    fn default() -> Self {
        let mut config = config::Builder::default();
        config.enable_quic().unwrap();
        // https://github.com/aws/s2n-tls/blob/main/docs/USAGE-GUIDE.md#s2n_config_set_cipher_preferences
        config.set_security_policy(crate::DEFAULT_POLICY).unwrap();
        config.set_application_protocol_preference([b"h3"]).unwrap();

        Self {
            config,
            keylog: None,
        }
    }
}

impl Builder {
    pub fn config_mut(&mut self) -> &mut s2n_tls::config::Builder {
        &mut self.config
    }

    /// Use FIPS approved cryptography.
    ///
    /// By default s2n-quic negotiates AES-128, AES-256 and ChaCha20-Poly1305 for AEAD
    /// operations. In order to comply with FIPS, this option configures s2n-quic to not
    /// negotiate ChaCha20-Poly1305.
    #[cfg(any(test, feature = "fips"))]
    pub fn with_fips(mut self) -> Self {
        self.config
            .set_security_policy(crate::DEFAULT_FIPS_POLICY)
            .unwrap();
        self
    }

    pub fn with_application_protocols<P: IntoIterator<Item = I>, I: AsRef<[u8]>>(
        mut self,
        protocols: P,
    ) -> Result<Self, Error> {
        self.config.set_application_protocol_preference(protocols)?;
        Ok(self)
    }

    pub fn with_certificate<C: IntoCertificate>(mut self, certificate: C) -> Result<Self, Error> {
        let certificate = certificate.into_certificate()?;
        let certificate = certificate
            .0
            .as_pem()
            .expect("pem is currently the only certificate format supported");
        self.config.trust_pem(certificate)?;
        Ok(self)
    }

    /// Clears the default trust store for this client
    ///
    /// By default, the trust store is initialized with common
    /// trust store locations for the host operating system.
    /// By invoking this method, the trust store will be cleared.
    ///
    /// Note that call ordering matters. The caller should call this
    /// method before making any calls to `with_trust_client_certificate_signed_by()`.
    /// Calling this method after a method that modifies the trust store will clear it.
    pub fn with_empty_trust_store(mut self) -> Result<Self, Error> {
        self.config.wipe_trust_store()?;
        Ok(self)
    }

    /// Add the cert and key to the key store.
    ///
    /// This must be set when the server requires client authentication (mutual TLS).
    /// The client will offer the certificate to the server when it is requested
    /// as part of the TLS handshake.
    pub fn with_client_identity<C: IntoCertificate, PK: IntoPrivateKey>(
        mut self,
        certificate: C,
        private_key: PK,
    ) -> Result<Self, Error> {
        let certificate = certificate.into_certificate()?;
        let private_key = private_key.into_private_key()?;
        self.config.load_pem(
            certificate
                .0
                .as_pem()
                .expect("pem is currently the only certificate format supported"),
            private_key
                .0
                .as_pem()
                .expect("pem is currently the only certificate format supported"),
        )?;
        self.config.set_client_auth_type(ClientAuthType::Required)?;
        Ok(self)
    }

    /// Set the host name verification callback.
    ///
    /// This will be invoked when a server certificate is presented during a TLS
    /// handshake. If this function is invoked, the default server name validation
    /// logic is disabled; this should only be used in very specific cases where normal
    /// TLS hostname validation is not appropriate.
    pub fn with_verify_host_name_callback<T: 'static + VerifyHostNameCallback>(
        mut self,
        handler: T,
    ) -> Result<Self, Error> {
        self.config.set_verify_host_callback(handler)?;
        Ok(self)
    }

    pub fn with_max_cert_chain_depth(mut self, len: u16) -> Result<Self, Error> {
        self.config.set_max_cert_chain_depth(len)?;
        Ok(self)
    }

    pub fn with_key_logging(mut self) -> Result<Self, Error> {
        use crate::keylog::KeyLog;

        self.keylog = KeyLog::try_open();

        unsafe {
            // Safety: the KeyLog is stored on `self` to ensure it outlives `config`
            if let Some(keylog) = self.keylog.as_ref() {
                self.config
                    .set_key_log_callback(Some(KeyLog::callback), Arc::as_ptr(keylog) as *mut _)?;
            } else {
                // disable key logging if it failed to create a file
                self.config
                    .set_key_log_callback(None, core::ptr::null_mut())?;
            }
        }

        Ok(self)
    }

    pub fn build(self) -> Result<Client, Error> {
        Ok(Client {
            loader: self.config.build()?,
            keylog: self.keylog,
            params: Default::default(),
        })
    }
}

impl<L: ConfigLoader> tls::Endpoint for Client<L> {
    type Session = Session;

    fn new_server_session<Params: EncoderValue>(&mut self, _params: &Params) -> Self::Session {
        panic!("cannot create a server session from a client config");
    }

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        params: &Params,
        server_name: ServerName,
    ) -> Self::Session {
        let config = self.loader.load(crate::ConnectionContext {
            server_name: Some(&server_name),
        });
        self.params.with(params, |params| {
            Session::new(endpoint::Type::Client, config, params, Some(server_name)).unwrap()
        })
    }

    fn max_tag_length(&self) -> usize {
        s2n_quic_crypto::MAX_TAG_LEN
    }
}
