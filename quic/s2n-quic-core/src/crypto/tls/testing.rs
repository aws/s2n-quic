use crate::{
    crypto::{tls, CryptoSuite, Key},
    transport::error::TransportError,
};
use bytes::Bytes;
use core::fmt;
use s2n_codec::EncoderValue;
use std::collections::VecDeque;

#[derive(Debug)]
pub struct Endpoint;

impl super::Endpoint for Endpoint {
    type Session = Session;

    fn new_server_session<Params: EncoderValue>(
        &mut self,
        _transport_parameters: &Params,
    ) -> Self::Session {
        Session
    }

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        _transport_parameters: &Params,
        _sni: &[u8],
    ) -> Self::Session {
        Session
    }

    fn max_tag_length(&self) -> usize {
        16
    }
}

#[derive(Debug)]
pub struct Session;

impl super::Session for Session {
    fn poll<C: tls::Context<Self>>(&mut self, _context: &mut C) -> Result<(), TransportError> {
        todo!("implement dummy handshake")
    }
}

impl CryptoSuite for Session {
    type HandshakeCrypto = crate::crypto::key::testing::Key;
    type InitialCrypto = crate::crypto::key::testing::Key;
    type ZeroRTTCrypto = crate::crypto::key::testing::Key;
    type OneRTTCrypto = crate::crypto::key::testing::Key;
    type RetryCrypto = crate::crypto::key::testing::Key;
}

/// Harness to ensure a TLS implementation adheres to the session contract
pub struct Context<C: CryptoSuite> {
    pub initial: Space<C::InitialCrypto>,
    pub handshake: Space<C::HandshakeCrypto>,
    pub application: Space<C::OneRTTCrypto>,
    pub zero_rtt_crypto: Option<C::ZeroRTTCrypto>,
    pub handshake_done: bool,
    pub sni: Option<Bytes>,
    pub alpn: Option<Bytes>,
    pub transport_parameters: Option<Bytes>,
}

impl<C: CryptoSuite> Default for Context<C> {
    fn default() -> Self {
        Self {
            initial: Space::default(),
            handshake: Space::default(),
            application: Space::default(),
            zero_rtt_crypto: None,
            handshake_done: false,
            sni: None,
            alpn: None,
            transport_parameters: None,
        }
    }
}

impl<C: CryptoSuite> fmt::Debug for Context<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Context")
            .field("initial", &self.initial)
            .field("handshake", &self.handshake)
            .field("application", &self.application)
            .field("zero_rtt_crypto", &self.zero_rtt_crypto.is_some())
            .field("handshake_done", &self.handshake_done)
            .field("sni", &self.sni)
            .field("alpn", &self.alpn)
            .field("transport_parameters", &self.transport_parameters)
            .finish()
    }
}

impl<C: CryptoSuite> Context<C> {
    pub fn transfer<O: CryptoSuite>(&mut self, other: &mut Context<O>) {
        self.initial.transfer(&mut other.initial);
        self.handshake.transfer(&mut other.handshake);
        self.application.transfer(&mut other.application);
    }

    pub fn finish<O: CryptoSuite>(&self, other: &Context<O>) {
        self.assert_done();
        other.assert_done();

        //assert_eq!(
        //    self.sni, other.sni,
        //    "sni is not consistent between endpoints"
        //);
        assert_eq!(
            self.alpn, other.alpn,
            "alpn is not consistent between endpoints"
        );

        assert_eq!(
            self.zero_rtt_crypto.is_some(),
            other.zero_rtt_crypto.is_some(),
            "0-rtt keys are not consistent between endpoints"
        );

        // TODO make sure the keys actually encrypt/decrypt with each other
    }

    fn assert_done(&self) {
        assert!(self.handshake.crypto.is_some());
        assert!(self.application.crypto.is_some());
        assert!(self.handshake_done);
        assert!(self.alpn.is_some());
        assert!(self.transport_parameters.is_some());
    }

    fn on_application_params(&mut self, params: tls::ApplicationParameters) {
        self.alpn = Some(Bytes::copy_from_slice(params.alpn_protocol));
        self.sni = params.sni.map(Bytes::copy_from_slice);
        self.transport_parameters = Some(Bytes::copy_from_slice(params.transport_parameters));
    }
}

pub struct Space<K: Key> {
    pub crypto: Option<K>,
    pub rx: VecDeque<Bytes>,
    pub tx: VecDeque<Bytes>,
}

impl<K: Key> Default for Space<K> {
    fn default() -> Self {
        Self {
            crypto: None,
            rx: VecDeque::new(),
            tx: VecDeque::new(),
        }
    }
}

impl<K: Key> fmt::Debug for Space<K> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Space")
            .field("crypto", &self.crypto.is_some())
            .field("rx", &self.rx)
            .field("tx", &self.tx)
            .finish()
    }
}

impl<K: Key> Space<K> {
    pub fn transfer<O: Key>(&mut self, other: &mut Space<O>) {
        self.rx.extend(other.tx.drain(..));
        other.rx.extend(self.tx.drain(..));
    }

    fn tx(&mut self, bytes: Bytes) {
        self.tx.push_back(bytes)
    }

    fn rx(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        loop {
            let mut chunk = self.rx.pop_front()?;

            if chunk.is_empty() {
                continue;
            }

            let max_len = max_len.unwrap_or(usize::MAX);

            if chunk.len() > max_len {
                self.rx.push_front(chunk.split_off(max_len));
            }

            return Some(chunk);
        }
    }
}

impl<C: CryptoSuite> tls::Context<C> for Context<C> {
    fn on_handshake_keys(&mut self, keys: C::HandshakeCrypto) -> Result<(), TransportError> {
        assert!(
            self.handshake.crypto.is_none(),
            "handshake keys emitted multiple times"
        );
        self.handshake.crypto = Some(keys);
        Ok(())
    }

    fn on_zero_rtt_keys(
        &mut self,
        keys: C::ZeroRTTCrypto,
        params: tls::ApplicationParameters,
    ) -> Result<(), TransportError> {
        assert!(
            self.zero_rtt_crypto.is_none(),
            "0-rtt keys emitted multiple times"
        );
        self.zero_rtt_crypto = Some(keys);
        self.on_application_params(params);
        Ok(())
    }

    fn on_one_rtt_keys(
        &mut self,
        keys: C::OneRTTCrypto,
        params: tls::ApplicationParameters,
    ) -> Result<(), TransportError> {
        assert!(
            self.application.crypto.is_none(),
            "1-rtt keys emitted multiple times"
        );
        self.application.crypto = Some(keys);
        self.on_application_params(params);
        Ok(())
    }

    fn on_handshake_done(&mut self) -> Result<(), TransportError> {
        assert!(!self.handshake_done, "handshake done called multiple times");
        self.handshake_done = true;
        Ok(())
    }

    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.initial.rx(max_len)
    }

    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.handshake.rx(max_len)
    }

    fn receive_application(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.application.rx(max_len)
    }

    fn can_send_initial(&self) -> bool {
        true
    }

    fn send_initial(&mut self, transmission: Bytes) {
        self.initial.tx(transmission)
    }

    fn can_send_handshake(&self) -> bool {
        self.handshake.crypto.is_some()
    }

    fn send_handshake(&mut self, transmission: Bytes) {
        assert!(
            self.can_send_handshake(),
            "handshake keys need to be emitted before buffering handshake crypto"
        );
        self.handshake.tx(transmission)
    }

    fn can_send_application(&self) -> bool {
        self.application.crypto.is_some()
    }

    fn send_application(&mut self, transmission: Bytes) {
        assert!(
            self.can_send_application(),
            "1-rtt keys need to be emitted before buffering application crypto"
        );
        self.application.tx(transmission)
    }
}
