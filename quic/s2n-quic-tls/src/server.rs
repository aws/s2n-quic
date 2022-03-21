// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    certificate::{IntoCertificate, IntoPrivateKey},
    keylog::KeyLogHandle,
    params::Params,
    session::Session,
    verify_host::{VerifyHostContext, VerifyHostContextHandle, VerifyHostContextWrapper},
};
use s2n_codec::EncoderValue;
use s2n_quic_core::{application::ServerName, crypto::tls, endpoint};
use s2n_tls::raw::{
    config::{self, Config},
    error::Error,
    ffi::s2n_cert_auth_type,
    security,
};
use std::sync::Arc;

pub struct Server {
    config: Config,
    #[allow(dead_code)] // we need to hold on to the handle to ensure it is cleaned up correctly
    keylog: Option<KeyLogHandle>,
    params: Params,
    #[allow(dead_code)] // we need to hold on to the handle to ensure it is cleaned up correctly
    verify_host_context: Option<VerifyHostContextHandle>,
}

impl Server {
    pub fn builder() -> Builder {
        Builder::default()
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("could not create a default server")
    }
}

pub struct Builder {
    config: config::Builder,
    keylog: Option<KeyLogHandle>,
    verify_host_context: Option<VerifyHostContextHandle>,
}

impl Default for Builder {
    fn default() -> Self {
        let mut config = config::Builder::default();
        config.enable_quic().unwrap();
        // https://github.com/aws/s2n-tls/blob/main/docs/USAGE-GUIDE.md#s2n_config_set_cipher_preferences
        config
            .set_security_policy(&security::DEFAULT_TLS13)
            .unwrap();
        config
            .set_application_protocol_preference(&[b"h3"])
            .unwrap();

        Self {
            config,
            keylog: None,
            verify_host_context: None,
        }
    }
}

impl Builder {
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
        Ok(self)
    }

    pub fn with_trust_client_certificate_signed_by<C: IntoCertificate>(
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

    pub fn with_client_authentication(mut self) -> Result<Self, Error> {
        self.config
            .set_client_auth_type(s2n_cert_auth_type::REQUIRED)?;
        Ok(self)
    }

    /// # Safety:
    /// The verify_host_context is passed to the callback as a raw mutable pointer. No thread
    /// safety is maintained on this pointer and it is the responsibility of the callback passed in
    /// to enforce mutual exclusion on the memory pointed to by the context handle.
    pub fn with_verify_host_callback(
        mut self,
        context: Box<dyn VerifyHostContext>,
    ) -> Result<Self, Error> {
        self.verify_host_context = Some(VerifyHostContextWrapper::new(context));
        if let Some(verify_host_context) = self.verify_host_context.as_ref() {
            unsafe {
                self.config.set_verify_host_callback(
                    Some(VerifyHostContextWrapper::verify_host_callback),
                    Arc::as_ptr(verify_host_context) as *mut _,
                )?;
            }
        }
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
            config: self.config.build()?,
            keylog: self.keylog,
            params: Default::default(),
            verify_host_context: self.verify_host_context,
        })
    }
}

impl tls::Endpoint for Server {
    type Session = Session;

    fn new_server_session<Params: EncoderValue>(&mut self, params: &Params) -> Self::Session {
        let config = self.config.clone();
        self.params.with(params, |params| {
            Session::new(endpoint::Type::Server, config, params).unwrap()
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
