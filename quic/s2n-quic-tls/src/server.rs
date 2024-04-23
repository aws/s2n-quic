// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    certificate::{Format, IntoCertificate, IntoPrivateKey},
    keylog::KeyLogHandle,
    params::Params,
    session::Session,
    ConfigLoader,
};
use s2n_codec::EncoderValue;
use s2n_quic_core::{application::ServerName, crypto::tls, endpoint};
#[cfg(any(test, feature = "unstable_client_hello"))]
use s2n_tls::callbacks::ClientHelloCallback;
#[cfg(any(test, feature = "unstable_private_key"))]
use s2n_tls::callbacks::PrivateKeyCallback;
use s2n_tls::{
    callbacks::VerifyHostNameCallback,
    config::{self, Config},
    enums::ClientAuthType,
    error::Error,
};
use std::sync::Arc;

pub struct Server<L: ConfigLoader = Config> {
    loader: L,
    #[allow(dead_code)] // we need to hold on to the handle to ensure it is cleaned up correctly
    keylog: Option<KeyLogHandle>,
    params: Params,
}

impl Server {
    pub fn builder() -> Builder {
        Builder::default()
    }
}

impl<L: ConfigLoader> Server<L> {
    /// Creates a [`Server`] from a [`ConfigLoader`]
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

impl Default for Server {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("could not create a default server")
    }
}

impl<L: ConfigLoader> ConfigLoader for Server<L> {
    #[inline]
    fn load(&mut self, cx: crate::ConnectionContext) -> s2n_tls::config::Config {
        self.loader.load(cx)
    }
}

impl<L: ConfigLoader> From<Server<L>> for Config {
    fn from(mut server: Server<L>) -> Self {
        server.load(crate::ConnectionContext { server_name: None })
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

    #[cfg(any(test, feature = "unstable_client_hello"))]
    pub fn with_client_hello_handler<T: 'static + ClientHelloCallback>(
        mut self,
        handler: T,
    ) -> Result<Self, Error> {
        self.config.set_client_hello_callback(handler)?;
        Ok(self)
    }

    #[cfg(any(test, feature = "unstable_private_key"))]
    pub fn with_private_key_handler<T: 'static + PrivateKeyCallback>(
        mut self,
        handler: T,
    ) -> Result<Self, Error> {
        self.config.set_private_key_callback(handler)?;
        Ok(self)
    }

    pub fn with_application_protocols<P: IntoIterator<Item = I>, I: AsRef<[u8]>>(
        mut self,
        protocols: P,
    ) -> Result<Self, Error> {
        self.config.set_application_protocol_preference(protocols)?;
        Ok(self)
    }

    pub fn with_certificate<C: IntoCertificate, PK: IntoPrivateKey>(
        mut self,
        certificate: C,
        private_key: PK,
    ) -> Result<Self, Error> {
        let private_key = private_key.into_private_key()?.0;
        let certificate = certificate.into_certificate()?.0;
        let certificate = certificate
            .as_pem()
            .expect("pem is currently the only certificate format supported");
        match private_key {
            Format::Pem(bytes) => self.config.load_pem(certificate, bytes.as_ref())?,
            Format::None => self.config.load_public_pem(certificate)?,
            Format::Der(_) => panic!("der private keys not supported"),
        };
        Ok(self)
    }

    pub fn with_trusted_certificate<C: IntoCertificate>(
        mut self,
        certificate: C,
    ) -> Result<Self, Error> {
        let certificate = certificate.into_certificate()?;
        let certificate = certificate
            .0
            .as_pem()
            .expect("pem is currently the only certificate format supported");
        self.config.trust_pem(certificate)?;
        Ok(self)
    }

    /// Clears the default trust store for this client.
    ///
    /// By default, the trust store is initialized with common
    /// trust store locations for the host operating system.
    /// By invoking this method, the trust store will be cleared.
    ///
    /// Note that call ordering matters. The caller should call this
    /// method before making any calls to `with_trusted_certificate()`.
    /// Calling this method after a method that modifies the trust store will clear it.
    pub fn with_empty_trust_store(mut self) -> Result<Self, Error> {
        self.config.wipe_trust_store()?;
        Ok(self)
    }

    /// Configures this server instance to require client authentication (mutual TLS).
    pub fn with_client_authentication(mut self) -> Result<Self, Error> {
        self.config.set_client_auth_type(ClientAuthType::Required)?;
        Ok(self)
    }

    /// Set the application level certificate verification handler which will be invoked on this
    /// server instance when a client certificate is presented during the mutual TLS handshake.
    #[deprecated(note = "use `with_verify_host_name_callback` instead")]
    pub fn with_verify_client_certificate_handler<T: 'static + VerifyHostNameCallback>(
        mut self,
        handler: T,
    ) -> Result<Self, Error> {
        self.config.set_verify_host_callback(handler)?;
        Ok(self)
    }

    /// Set the host name verification callback.
    ///
    /// This will be invoked when a client certificate is presented during a mutual TLS
    /// handshake.
    pub fn with_verify_host_name_callback<T: 'static + VerifyHostNameCallback>(
        mut self,
        handler: T,
    ) -> Result<Self, Error> {
        self.config.set_verify_host_callback(handler)?;
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

    pub fn build(self) -> Result<Server, Error> {
        Ok(Server {
            loader: self.config.build()?,
            keylog: self.keylog,
            params: Default::default(),
        })
    }
}

impl<L: ConfigLoader> tls::Endpoint for Server<L> {
    type Session = Session;

    fn new_server_session<Params: EncoderValue>(&mut self, params: &Params) -> Self::Session {
        let config = self
            .loader
            .load(crate::ConnectionContext { server_name: None });
        self.params.with(params, |params| {
            Session::new(endpoint::Type::Server, config, params, None).unwrap()
        })
    }

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        _transport_parameters: &Params,
        _erver_name: ServerName,
    ) -> Self::Session {
        panic!("cannot create a client session from a server config");
    }

    fn max_tag_length(&self) -> usize {
        s2n_quic_crypto::MAX_TAG_LEN
    }
}
