// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::ServerName,
    crypto::{
        header_crypto::{LONG_HEADER_MASK, SHORT_HEADER_MASK},
        tls, CryptoSuite, HeaderKey, Key,
    },
    endpoint, transport,
};
use alloc::sync::Arc;
use bytes::Bytes;
use core::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    task::{Poll, Waker},
};
use futures_test::task::new_count_waker;
use s2n_codec::EncoderValue;
use std::collections::VecDeque;

pub mod certificates {
    macro_rules! pem {
        ($name:ident, $path:expr) => {
            pub static $name: &str =
                include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/certs/", $path));
        };
    }
    macro_rules! der {
        ($name:ident, $path:expr) => {
            pub static $name: &[u8] =
                include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/certs/", $path));
        };
    }

    pem!(KEY_PEM, "key.pem");
    pem!(CERT_PEM, "cert.pem");
    der!(KEY_DER, "key.der");
    der!(CERT_DER, "cert.der");
}

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
        _server_name: ServerName,
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
    fn poll<C: tls::Context<Self>>(
        &mut self,
        _context: &mut C,
    ) -> Poll<Result<(), transport::Error>> {
        todo!("implement dummy handshake")
    }
}

impl CryptoSuite for Session {
    type HandshakeKey = crate::crypto::key::testing::Key;
    type HandshakeHeaderKey = crate::crypto::key::testing::HeaderKey;
    type InitialKey = crate::crypto::key::testing::Key;
    type InitialHeaderKey = crate::crypto::key::testing::HeaderKey;
    type ZeroRttKey = crate::crypto::key::testing::Key;
    type ZeroRttHeaderKey = crate::crypto::key::testing::HeaderKey;
    type OneRttKey = crate::crypto::key::testing::Key;
    type OneRttHeaderKey = crate::crypto::key::testing::HeaderKey;
    type RetryKey = crate::crypto::key::testing::Key;
}

#[derive(Debug)]
pub struct TlsEndpoint<S: tls::Session> {
    pub session: S,
    pub context: Context<S>,
}

impl<S: tls::Session> TlsEndpoint<S> {
    fn new(session: S, context: Context<S>) -> Self {
        Self { session, context }
    }
}

/// A pair of TLS sessions and contexts being driven to completion
#[derive(Debug)]
pub struct Pair<S: tls::Session, C: tls::Session> {
    pub server: TlsEndpoint<S>,
    pub client: TlsEndpoint<C>,
    pub server_name: ServerName,
}

const TEST_SERVER_TRANSPORT_PARAMS: &[u8] = &[1, 2, 3];
const TEST_CLIENT_TRANSPORT_PARAMS: &[u8] = &[3, 2, 1];

impl<S: tls::Session, C: tls::Session> Pair<S, C> {
    pub fn new<SE, CE>(
        server_endpoint: &mut SE,
        client_endpoint: &mut CE,
        server_name: ServerName,
    ) -> Self
    where
        SE: tls::Endpoint<Session = S>,
        CE: tls::Endpoint<Session = C>,
    {
        use crate::crypto::InitialKey;

        let server = server_endpoint.new_server_session(&TEST_SERVER_TRANSPORT_PARAMS);
        let mut server_context = Context::new(endpoint::Type::Server);
        server_context.initial.crypto = Some(S::InitialKey::new_server(server_name.as_bytes()));

        let client =
            client_endpoint.new_client_session(&TEST_CLIENT_TRANSPORT_PARAMS, server_name.clone());
        let mut client_context = Context::new(endpoint::Type::Client);
        client_context.initial.crypto = Some(C::InitialKey::new_client(server_name.as_bytes()));

        Self {
            server: TlsEndpoint::new(server, server_context),
            client: TlsEndpoint::new(client, client_context),
            server_name,
        }
    }

    /// Returns true if `poll` should be called
    pub fn is_handshaking(&self) -> bool {
        !(self.server.context.handshake_complete && self.client.context.handshake_complete)
    }

    /// Continues progress of the handshake
    pub fn poll(
        &mut self,
        client_hello_cb_done: Option<Arc<AtomicBool>>,
    ) -> Result<(), transport::Error> {
        match self.client.session.poll(&mut self.client.context) {
            Poll::Ready(res) => res?,
            Poll::Pending => (),
        }
        match self.server.session.poll(&mut self.server.context) {
            Poll::Ready(res) => res?,
            Poll::Pending => (),
        }
        self.client.context.transfer(&mut self.server.context);

        eprintln!("1/2 RTT");
        if let Some(cb_done) = client_hello_cb_done {
            // If the server is processing the async client hello callback, then return early
            // and poll it until it completes
            if !cb_done.load(Ordering::SeqCst)
                && matches!(
                    self.server.context.server_state,
                    ServerState::ClientHelloRead
                )
            {
                return Ok(());
            };
        }
        self.check_progress();

        Ok(())
    }

    fn check_progress(&mut self) {
        match (
            &self.client.context.client_state,
            &self.server.context.server_state,
        ) {
            (ClientState::ClientHelloSent, ServerState::WaitingClientHello) => {
                self.client.context.client_state.transition();
                self.server.context.server_state.transition();
                assert!(
                    !self.server.context.initial.rx.is_empty(),
                    "client should send ClientHello"
                );
            }
            (ClientState::WaitingServerHello, ServerState::ClientHelloRead) => {
                self.client.context.client_state.transition();
                self.server.context.server_state.transition();
                assert!(
                    self.server.context.handshake.crypto.is_some(),
                    "server should have handshake keys after sending the ServerHello"
                );
                assert!(
                    self.server.context.application.crypto.is_some(),
                    "server should have application keys after sending a ServerFinished"
                );
                assert!(!self.server.context.handshake_complete);
                assert!(!self.client.context.handshake_complete);
            }
            (ClientState::ServerHelloRead, ServerState::WaitingClientFinish) => {
                self.server.context.server_state.transition();
                self.client.context.client_state.transition();
                assert!(
                    self.client.context.handshake.crypto.is_some(),
                    "client should have handshake keys after reading the ServerHello"
                );
                assert!(
                    self.client.context.application.crypto.is_some(),
                    "client should have application keys after reading the ServerFinished"
                );
                assert!(
                    self.client.context.handshake_complete,
                    "client should complete the handshake"
                );
            }
            (ClientState::ClientFinishSent, ServerState::ClientFinishRead) => {
                // Finally transition to HandshakeComplete
                self.server.context.server_state.transition();
                self.client.context.client_state.transition();
                assert!(
                    self.server.context.handshake_complete,
                    "server should finish after reading the ClientFinished"
                );
            }
            _ => unreachable!("handshake made too many iterations"),
        };
    }

    /// Finished the test
    pub fn finish(&self) {
        self.client.context.finish(&self.server.context);

        assert_eq!(
            self.client.context.transport_parameters.as_ref().unwrap(),
            TEST_SERVER_TRANSPORT_PARAMS,
            "client did not receive the server transport parameters"
        );
        assert_eq!(
            self.server.context.transport_parameters.as_ref().unwrap(),
            TEST_CLIENT_TRANSPORT_PARAMS,
            "server did not receive the client transport parameters"
        );
        assert_eq!(
            self.client
                .context
                .server_name
                .as_ref()
                .expect("missing SNI on client"),
            &self.server_name[..]
        );
        assert_eq!(
            self.server
                .context
                .server_name
                .as_ref()
                .expect("missing ServerName on server"),
            &self.server_name[..]
        );
        assert!(
            matches!(
                self.client.context.client_state,
                ClientState::HandshakeComplete
            ),
            "client state did not complete: current state: {:?}",
            self.client.context.client_state,
        );
        assert!(
            matches!(
                self.server.context.server_state,
                ServerState::HandshakeComplete
            ),
            "server state did not complete: current state: {:?}",
            self.server.context.server_state,
        );
        assert!(
            matches!(self.server.context.client_state, ClientState::Unused)
                && matches!(self.client.context.server_state, ServerState::Unused),
            "Expect other state to be unused. server {:?}, client {:?}",
            self.server.context.client_state,
            self.client.context.server_state,
        );

        // TODO check 0-rtt keys
    }
}

#[derive(Debug)]
pub enum ClientState {
    Unused,
    ClientHelloSent,
    WaitingServerHello,
    ServerHelloRead,
    ClientFinishSent,
    HandshakeComplete,
}

impl ClientState {
    fn transition(&mut self) {
        let new_state = match &self {
            ClientState::Unused => Self::Unused,
            ClientState::ClientHelloSent => Self::WaitingServerHello,
            ClientState::WaitingServerHello => Self::ServerHelloRead,
            ClientState::ServerHelloRead => Self::ClientFinishSent,
            ClientState::ClientFinishSent => Self::HandshakeComplete,
            ClientState::HandshakeComplete => unreachable!("handshake made too many iterations"),
        };

        *self = new_state;
    }
}

#[derive(Debug)]
pub enum ServerState {
    Unused,
    WaitingClientHello,
    ClientHelloRead, // send server hello, handshake keys, 1-rtt keys
    WaitingClientFinish,
    ClientFinishRead,
    HandshakeComplete,
}

impl ServerState {
    fn transition(&mut self) {
        let new_state = match &self {
            ServerState::Unused => Self::Unused,
            ServerState::WaitingClientHello => Self::ClientHelloRead,
            ServerState::ClientHelloRead => Self::WaitingClientFinish,
            ServerState::WaitingClientFinish => Self::ClientFinishRead,
            ServerState::ClientFinishRead => Self::HandshakeComplete,
            ServerState::HandshakeComplete => unreachable!("handshake made too many iterations"),
        };

        *self = new_state;
    }
}

/// Harness to ensure a TLS implementation adheres to the session contract
pub struct Context<C: CryptoSuite> {
    pub initial: Space<C::InitialKey, C::InitialHeaderKey>,
    pub handshake: Space<C::HandshakeKey, C::HandshakeHeaderKey>,
    pub application: Space<C::OneRttKey, C::OneRttHeaderKey>,
    pub zero_rtt_crypto: Option<(C::ZeroRttKey, C::ZeroRttHeaderKey)>,
    pub handshake_complete: bool,
    pub server_name: Option<Bytes>,
    pub application_protocol: Option<Bytes>,
    pub transport_parameters: Option<Bytes>,
    endpoint: endpoint::Type,
    pub client_state: ClientState,
    pub server_state: ServerState,
    waker: Waker,
}

impl<C: CryptoSuite> fmt::Debug for Context<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Context")
            .field("initial", &self.initial)
            .field("handshake", &self.handshake)
            .field("application", &self.application)
            .field("zero_rtt_crypto", &self.zero_rtt_crypto.is_some())
            .field("handshake_complete", &self.handshake_complete)
            .field("sni", &self.server_name)
            .field("application_protocol", &self.application_protocol)
            .field("transport_parameters", &self.transport_parameters)
            .finish()
    }
}

impl<C: CryptoSuite> Context<C> {
    fn new(endpoint: endpoint::Type) -> Self {
        let (waker, _wake_counter) = new_count_waker();
        let (client_state, server_state) = match endpoint {
            endpoint::Type::Client => (ClientState::ClientHelloSent, ServerState::Unused),
            endpoint::Type::Server => (ClientState::Unused, ServerState::WaitingClientHello),
        };
        Self {
            initial: Space::default(),
            handshake: Space::default(),
            application: Space::default(),
            zero_rtt_crypto: None,
            handshake_complete: false,
            server_name: None,
            application_protocol: None,
            transport_parameters: None,
            client_state,
            server_state,
            endpoint,
            waker,
        }
    }

    /// Transfers incoming and outgoing buffers between two contexts
    pub fn transfer<O: CryptoSuite>(&mut self, other: &mut Context<O>) {
        self.initial.transfer(&mut other.initial);
        self.handshake.transfer(&mut other.handshake);
        self.application.transfer(&mut other.application);
    }

    /// Finishes the test and asserts consistency
    pub fn finish<O: CryptoSuite>(&self, other: &Context<O>) {
        self.assert_done();
        other.assert_done();

        assert_eq!(
            self.server_name, other.server_name,
            "sni is not consistent between endpoints"
        );
        assert_eq!(
            self.application_protocol, other.application_protocol,
            "application_protocol is not consistent between endpoints"
        );

        assert_eq!(
            self.zero_rtt_crypto.is_some(),
            other.zero_rtt_crypto.is_some(),
            "0-rtt keys are not consistent between endpoints"
        );

        self.initial.finish(&other.initial);
        self.handshake.finish(&other.handshake);
        self.application.finish(&other.application);
    }

    fn assert_done(&self) {
        assert!(self.initial.crypto.is_some(), "missing initial crypto");
        assert!(self.handshake.crypto.is_some(), "missing handshake crypto");
        assert!(
            self.application.crypto.is_some(),
            "missing application crypto"
        );
        assert!(self.handshake_complete);
        assert!(self.application_protocol.is_some());
        assert!(self.transport_parameters.is_some());
    }

    fn on_application_params(&mut self, params: tls::ApplicationParameters) {
        self.transport_parameters = Some(Bytes::copy_from_slice(params.transport_parameters));
    }

    fn log(&self, event: &str) {
        eprintln!(
            "{:?}: {}: {}",
            self.endpoint,
            core::any::type_name::<C>(),
            event,
        );
    }
}

pub struct Space<K: Key, Hk: HeaderKey> {
    pub crypto: Option<(K, Hk)>,
    pub rx: VecDeque<Bytes>,
    pub tx: VecDeque<Bytes>,
}

impl<K: Key, Hk: HeaderKey> Default for Space<K, Hk> {
    fn default() -> Self {
        Self {
            crypto: None,
            rx: VecDeque::new(),
            tx: VecDeque::new(),
        }
    }
}

impl<K: Key, Hk: HeaderKey> fmt::Debug for Space<K, Hk> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Space")
            .field("crypto", &self.crypto.is_some())
            .field("rx", &self.rx)
            .field("tx", &self.tx)
            .finish()
    }
}

impl<K: Key, Hk: HeaderKey> Space<K, Hk> {
    pub fn transfer<O: Key, Ohk: HeaderKey>(&mut self, other: &mut Space<O, Ohk>) {
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

    fn finish<O: Key, Ohk: HeaderKey>(&self, other: &Space<O, Ohk>) {
        let (crypto_a, crypto_a_hk) = self.crypto.as_ref().expect("missing crypto");
        let (crypto_b, crypto_b_hk) = other.crypto.as_ref().expect("missing crypto");

        // ensure payloads can be encrypted and decrypted in both directions
        seal_open(crypto_a, crypto_b);
        seal_open(crypto_b, crypto_a);

        // ensure packet protection keys can be applied in both directions
        protect_unprotect(crypto_a_hk, crypto_b_hk, LONG_HEADER_MASK);
        protect_unprotect(crypto_b_hk, crypto_a_hk, LONG_HEADER_MASK);
        protect_unprotect(crypto_a_hk, crypto_b_hk, SHORT_HEADER_MASK);
        protect_unprotect(crypto_b_hk, crypto_a_hk, SHORT_HEADER_MASK);
    }
}

fn seal_open<S: Key, O: Key>(sealer: &S, opener: &O) {
    let packet_number = 123;
    let header = &[1, 2, 3, 4, 5, 6];

    let cleartext_payload = (0u16..1200).map(|i| i as u8).collect::<Vec<_>>();

    let mut encrypted_payload = cleartext_payload.clone();
    encrypted_payload.resize(cleartext_payload.len() + sealer.tag_len(), 0);

    let sealer_name = core::any::type_name::<S>();
    let opener_name = core::any::type_name::<O>();

    sealer
        .encrypt(packet_number, header, &mut encrypted_payload)
        .unwrap_or_else(|err| {
            panic!(
                "encryption error; opener={}, sealer={} - {:?}",
                opener_name, sealer_name, err
            )
        });
    opener
        .decrypt(packet_number, header, &mut encrypted_payload)
        .unwrap_or_else(|err| {
            panic!(
                "decryption error; opener={}, sealer={} - {:?}",
                opener_name, sealer_name, err
            )
        });

    assert_eq!(
        cleartext_payload[..],
        encrypted_payload[..cleartext_payload.len()]
    );
}

fn protect_unprotect<P: HeaderKey, U: HeaderKey>(protect: &P, unprotect: &U, tag_mask: u8) {
    let sample = [1u8; 1000];

    let mut protected_mask =
        protect.sealing_header_protection_mask(&sample[..protect.sealing_sample_len()]);
    let mut unprotected_mask =
        unprotect.opening_header_protection_mask(&sample[..unprotect.opening_sample_len()]);

    let protect_name = core::any::type_name::<P>();
    let unprotect_name = core::any::type_name::<U>();

    // we only care about certain bits being the same so mask out the others
    protected_mask[0] &= tag_mask;
    unprotected_mask[0] &= tag_mask;

    assert_eq!(
        &protected_mask, &unprotected_mask,
        "{} -> {}",
        protect_name, unprotect_name
    );
}

impl<C: CryptoSuite> tls::Context<C> for Context<C> {
    fn on_handshake_keys(
        &mut self,
        key: C::HandshakeKey,
        header_key: C::HandshakeHeaderKey,
    ) -> Result<(), transport::Error> {
        assert!(
            self.handshake.crypto.is_none(),
            "handshake keys emitted multiple times"
        );
        self.log("handshake keys");
        self.handshake.crypto = Some((key, header_key));
        Ok(())
    }

    fn on_zero_rtt_keys(
        &mut self,
        key: C::ZeroRttKey,
        header_key: C::ZeroRttHeaderKey,
        params: tls::ApplicationParameters,
    ) -> Result<(), transport::Error> {
        assert!(
            self.zero_rtt_crypto.is_none(),
            "0-rtt keys emitted multiple times"
        );
        self.log("0-rtt keys");
        self.zero_rtt_crypto = Some((key, header_key));
        self.on_application_params(params);
        Ok(())
    }

    fn on_one_rtt_keys(
        &mut self,
        key: C::OneRttKey,
        header_key: C::OneRttHeaderKey,
        params: tls::ApplicationParameters,
    ) -> Result<(), transport::Error> {
        assert!(
            self.application.crypto.is_none(),
            "1-rtt keys emitted multiple times"
        );
        self.log("1-rtt keys");
        self.application.crypto = Some((key, header_key));
        self.on_application_params(params);

        Ok(())
    }

    fn on_server_name(
        &mut self,
        server_name: crate::application::ServerName,
    ) -> Result<(), transport::Error> {
        self.log("server name");
        self.server_name = Some(server_name.into_bytes());
        Ok(())
    }

    fn on_application_protocol(
        &mut self,
        application_protocol: Bytes,
    ) -> Result<(), transport::Error> {
        self.log("application protocol");
        self.application_protocol = Some(application_protocol);
        Ok(())
    }

    fn on_handshake_complete(&mut self) -> Result<(), transport::Error> {
        assert!(
            !self.handshake_complete,
            "handshake complete called multiple times"
        );
        assert!(
            !self.application_protocol.as_ref().unwrap().is_empty(),
            "application_protocol is empty at handshake complete"
        );
        self.handshake_complete = true;
        self.log("handshake complete");
        Ok(())
    }

    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.log("rx initial");
        self.initial.rx(max_len)
    }

    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.log("rx handshake");
        self.handshake.rx(max_len)
    }

    fn receive_application(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.log("rx application");
        self.application.rx(max_len)
    }

    fn can_send_initial(&self) -> bool {
        true
    }

    fn send_initial(&mut self, transmission: Bytes) {
        self.log("tx initial");
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
        self.log("tx handshake");
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
        self.log("tx application");
        self.application.tx(transmission)
    }

    fn waker(&self) -> &Waker {
        &self.waker
    }
}
