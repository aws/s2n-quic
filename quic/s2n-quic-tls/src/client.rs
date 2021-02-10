use crate::{certificate::IntoCertificate, params::Params, session::Session};
use s2n_codec::EncoderValue;
use s2n_quic_core::{crypto::tls, endpoint};
use s2n_tls::{
    config::{self, Config},
    error::Error,
};

pub struct Client {
    config: Config,
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

    pub fn with_certificate<C: IntoCertificate>(mut self, certificate: C) -> Result<Self, Error> {
        let certificate = certificate.into_certificate()?;
        let certificate = certificate
            .0
            .as_pem()
            .expect("pem is currently the only certificate format supported");
        self.config.trust_pem(certificate)?;
        Ok(self)
    }

    pub fn build(self) -> Result<Client, Error> {
        Ok(Client {
            config: self.config.build()?,
            params: Default::default(),
        })
    }
}

impl tls::Endpoint for Client {
    type Session = Session;

    fn new_server_session<Params: EncoderValue>(&mut self, _params: &Params) -> Self::Session {
        panic!("cannot create a server session from a client config");
    }

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        params: &Params,
        sni: &[u8],
    ) -> Self::Session {
        let config = self.config.clone();
        self.params.with(params, |params| {
            let mut session = Session::new(endpoint::Type::Client, config, params).unwrap();
            session.connection.set_sni(sni).expect("invalid sni value");
            session
        })
    }

    fn max_tag_length(&self) -> usize {
        s2n_quic_ring::MAX_TAG_LEN
    }
}
