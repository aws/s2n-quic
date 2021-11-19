// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{certificate::IntoCertificate, keylog::KeyLogHandle, params::Params, session::Session};
use s2n_codec::EncoderValue;
use s2n_quic_core::{application::Sni, connection::InitialId, crypto::tls, endpoint};
use s2n_tls::{
    config::{self, Config},
    error::Error,
};
use std::sync::Arc;

pub struct Client {
    config: Config,
    #[allow(dead_code)] // we need to hold on to the handle to ensure it is cleaned up correctly
    keylog: Option<KeyLogHandle>,
    params: Params,
}

impl Client {
    pub fn builder() -> Builder {
        Builder::default()
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("could not create a default client")
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
        // https://github.com/awslabs/s2n/blob/main/docs/USAGE-GUIDE.md#s2n_config_set_cipher_preferences
        config.set_cipher_preference("default_tls13").unwrap();
        config.set_alpn_preference(&[b"h3"]).unwrap();

        Self {
            config,
            keylog: None,
        }
    }
}

impl Builder {
    pub fn with_alpn_protocols<P: IntoIterator<Item = I>, I: AsRef<[u8]>>(
        mut self,
        protocols: P,
    ) -> Result<Self, Error> {
        self.config.set_alpn_preference(protocols)?;
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
            config: self.config.build()?,
            keylog: self.keylog,
            params: Default::default(),
        })
    }
}

impl tls::Endpoint for Client {
    type Session = Session;

    fn new_server_session<Params: EncoderValue>(
        &mut self,
        _params: &Params,
        _initial_id: InitialId,
    ) -> Self::Session {
        panic!("cannot create a server session from a client config");
    }

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        params: &Params,
        sni: Sni,
        initial_id: InitialId,
    ) -> Self::Session {
        let config = self.config.clone();
        self.params.with(params, |params| {
            let mut session =
                Session::new(endpoint::Type::Client, config, params, initial_id).unwrap();
            session
                .connection
                .set_sni(sni.as_bytes())
                .expect("invalid sni value");
            session
        })
    }

    fn max_tag_length(&self) -> usize {
        s2n_quic_ring::MAX_TAG_LEN
    }
}
