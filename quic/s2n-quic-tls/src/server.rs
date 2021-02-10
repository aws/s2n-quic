use crate::{
    certificate::{IntoCertificate, IntoPrivateKey},
    params::Params,
    session::Session,
};
use s2n_codec::EncoderValue;
use s2n_quic_core::{crypto::tls, endpoint};
use s2n_tls::{
    config::{self, Config},
    error::Error,
};

pub struct Server {
    config: Config,
    params: Params,
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
}

impl Default for Builder {
    fn default() -> Self {
        let mut config = config::Builder::default();
        config.enable_quic().unwrap();
        config.set_cipher_preference("default_tls13").unwrap();
        config.set_alpn_preference(&[b"h3"]).unwrap();

        Self { config }
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

    pub fn build(self) -> Result<Server, Error> {
        Ok(Server {
            config: self.config.build()?,
            params: Default::default(),
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
        _sni: &[u8],
    ) -> Self::Session {
        panic!("cannot create a client session from a server config");
    }

    fn max_tag_length(&self) -> usize {
        s2n_quic_ring::MAX_TAG_LEN
    }
}
