// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::callback::{self, Callback};
use bytes::BytesMut;
use core::{marker::PhantomData, task::Poll};
use s2n_quic_core::{
    application::ServerName,
    crypto::{tls, tls::CipherSuite, CryptoSuite},
    endpoint, ensure, transport,
};
use s2n_quic_crypto::Suite;
use s2n_tls::{
    config::Config,
    connection::Connection,
    enums::{Blinding, Mode},
    error::{Error, ErrorType},
};

#[derive(Debug)]
pub struct Session {
    endpoint: endpoint::Type,
    pub(crate) connection: Connection,
    state: callback::State,
    handshake_complete: bool,
    send_buffer: BytesMut,
    // This field is used to minimize allocations for the client.
    // No allocation needs to occur when the on_server_name callback triggers
    // since the client has already stored the server_name at the beginning
    // of a session.
    server_name: Option<ServerName>,
    received_ticket: bool,
    server_params: Vec<u8>,
}

impl Session {
    pub fn new(
        endpoint: endpoint::Type,
        config: Config,
        params: &[u8],
        server_name: Option<ServerName>,
    ) -> Result<Self, Error> {
        let mut connection = Connection::new(match endpoint {
            endpoint::Type::Server => Mode::Server,
            endpoint::Type::Client => Mode::Client,
        });

        connection.set_config(config)?;
        connection.enable_quic()?;

        let server_params = {
            if endpoint.is_client() {
                connection.set_quic_transport_parameters(params)?;
                Vec::new()
            } else {
                // Save the server's transport parameters for later, in case
                // additional values need to be appended
                params.to_vec()
            }
        };

        // QUIC handles sending alerts, so no need to apply TLS blinding
        connection.set_blinding(Blinding::SelfService)?;

        if let Some(server_name) = server_name.as_ref() {
            connection
                .set_server_name(server_name)
                .expect("invalid server name value");
        }

        Ok(Self {
            endpoint,
            connection,
            state: Default::default(),
            handshake_complete: false,
            send_buffer: BytesMut::new(),
            server_name,
            received_ticket: false,
            server_params,
        })
    }
}

impl CryptoSuite for Session {
    type HandshakeKey = <Suite as CryptoSuite>::HandshakeKey;
    type HandshakeHeaderKey = <Suite as CryptoSuite>::HandshakeHeaderKey;
    type InitialKey = <Suite as CryptoSuite>::InitialKey;
    type InitialHeaderKey = <Suite as CryptoSuite>::InitialHeaderKey;
    type OneRttKey = <Suite as CryptoSuite>::OneRttKey;
    type OneRttHeaderKey = <Suite as CryptoSuite>::OneRttHeaderKey;
    type ZeroRttKey = <Suite as CryptoSuite>::ZeroRttKey;
    type ZeroRttHeaderKey = <Suite as CryptoSuite>::ZeroRttHeaderKey;
    type RetryKey = <Suite as CryptoSuite>::RetryKey;
}

impl tls::TlsSession for Session {
    fn tls_exporter(
        &self,
        label: &[u8],
        context: &[u8],
        output: &mut [u8],
    ) -> Result<(), tls::TlsExportError> {
        self.connection
            .tls_exporter(label, context, output)
            .map_err(|_| tls::TlsExportError::failure())
    }

    fn cipher_suite(&self) -> CipherSuite {
        self.state.cipher_suite()
    }

    fn peer_cert_chain_der(&self) -> Result<Vec<Vec<u8>>, tls::ChainError> {
        self.connection
            .peer_cert_chain()
            .map_err(|_| tls::ChainError::failure())?
            .iter()
            .map(|v| Ok(v?.der()?.to_vec()))
            .collect::<Result<Vec<Vec<u8>>, s2n_tls::error::Error>>()
            .map_err(|_| tls::ChainError::failure())
    }
}

impl tls::Session for Session {
    fn poll<W>(&mut self, context: &mut W) -> Poll<Result<(), transport::Error>>
    where
        W: tls::Context<Self>,
    {
        let mut callback: Callback<W, Self> = Callback {
            context,
            endpoint: self.endpoint,
            state: &mut self.state,
            suite: PhantomData,
            err: None,
            send_buffer: &mut self.send_buffer,
            server_name: &mut self.server_name,
            server_params: &mut self.server_params,
        };

        unsafe {
            // Safety: the callback struct must live as long as the callbacks are
            // set on on the connection
            callback.set(&mut self.connection);
        }

        let result = self.connection.poll_negotiate().map_ok(|_| ());

        callback.unset(&mut self.connection)?;

        match result {
            Poll::Ready(Ok(())) => {
                // s2n-tls has indicated that the handshake is complete
                if !self.handshake_complete {
                    self.state.on_handshake_complete();
                    context.on_handshake_complete()?;
                    context.on_tls_exporter_ready(self)?;
                    self.handshake_complete = true;
                }
                // TODO Add new s2n-tls new api, take and put in quic::connection
                // let ctx: Option<Box<dyn Any + Send + Sync>> =
                //     self.connection.take_tls_context();
                // context.on_tls_context(ctx);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e
                .alert()
                .map(tls::Error::new)
                .unwrap_or(tls::Error::HANDSHAKE_FAILURE)
                .with_reason(e.message())
                .into())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn process_post_handshake_message<W>(&mut self, context: &mut W) -> Result<(), transport::Error>
    where
        W: tls::Context<Self>,
    {
        let mut callback: Callback<W, Self> = Callback {
            context,
            endpoint: self.endpoint,
            state: &mut self.state,
            suite: PhantomData,
            err: None,
            send_buffer: &mut self.send_buffer,
            server_name: &mut self.server_name,
            server_params: &mut self.server_params,
        };

        unsafe {
            // Safety: the callback struct must live as long as the callbacks are
            // set on on the connection
            callback.set(&mut self.connection);
        }

        let result = self
            .connection
            .quic_process_post_handshake_message()
            .map(|_| ());

        callback.unset(&mut self.connection)?;

        match result {
            Ok(_) => {
                self.received_ticket = true;
                Ok(())
            }
            Err(e) => {
                // Blocking errors are the only type of s2n-tls error
                // that can be retried.
                if matches!(e.kind(), ErrorType::Blocked) {
                    Ok(())
                } else {
                    Err(e
                        .alert()
                        .map(tls::Error::new)
                        .unwrap_or(tls::Error::HANDSHAKE_FAILURE)
                        .into())
                }
            }
        }
    }

    fn should_discard_session(&self) -> bool {
        // Only clients process post-handshake messages currently
        ensure!(self.endpoint.is_client(), true);

        // Clients that haven't enabled resumption can discard the session
        ensure!(self.connection.are_session_tickets_enabled(), true);

        // Discard the session once a ticket is received
        self.received_ticket
    }
}

pub struct SlowEndpoint {
    server_endpoint: Option<crate::Server>,
    client_endpoint: Option<crate::Client>,
}

impl SlowEndpoint {
    pub fn new_server(cert_pem: &str, key_pem: &str) -> Self {
        let server_endpoint = crate::server::Builder::default()
            .with_certificate(cert_pem, key_pem)
            .unwrap()
            .build()
            .unwrap();
        Self {
            server_endpoint: Some(server_endpoint),
            client_endpoint: None,
        }
    }

    pub fn new_client(cert_pem: &str) -> Self {
        let client_endpoint = crate::client::Builder::default()
            .with_certificate(cert_pem)
            .unwrap()
            .build()
            .unwrap();
        Self {
            server_endpoint: None,
            client_endpoint: Some(client_endpoint),
        }
    }
}

impl tls::Endpoint for SlowEndpoint {
    type Session = SlowSession<Session>;

    fn new_server_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session {
        let inner_session = self
            .server_endpoint
            .take()
            .expect("Server should exist")
            .new_server_session(transport_parameters);
        SlowSession {
            defer: 10,
            inner_session,
        }
    }

    fn new_client_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        server_name: s2n_quic_core::application::ServerName,
    ) -> Self::Session {
        let inner_session = self
            .client_endpoint
            .take()
            .expect("Client should exist")
            .new_client_session(transport_parameters, server_name);
        SlowSession {
            defer: 10,
            inner_session,
        }
    }

    fn max_tag_length(&self) -> usize {
        todo!()
    }
}

// SlowSession is a test TLS provider that is slow, namely, for each call to poll,
// it returns Poll::Pending three times before actually polling the real TLS library.
// This is used in an integration test to assert that our code is correct in the event
// of any random pendings/wakeups that might occur when negotiating TLS.
#[derive(Debug)]
pub struct SlowSession<S: tls::Session> {
    defer: u8,
    inner_session: S,
}

impl<S: tls::Session> tls::Session for SlowSession<S> {
    #[inline]
    fn poll<W>(&mut self, context: &mut W) -> Poll<Result<(), transport::Error>>
    where
        W: tls::Context<Self>,
    {
        // Self-wake and return Pending if defer is non-zero
        if let Some(d) = self.defer.checked_sub(1) {
            self.defer = d;
            context.waker().wake_by_ref();
            return Poll::Pending;
        }

        // Otherwise we'll call the function to actually make progress
        // in the TLS handshake and set up to defer again the next time
        // we're here.
        self.defer = 10;
        self.inner_session.poll(&mut SlowContext(context))
    }
}

impl<S: tls::Session> CryptoSuite for SlowSession<S> {
    type HandshakeKey = <S as CryptoSuite>::HandshakeKey;
    type HandshakeHeaderKey = <S as CryptoSuite>::HandshakeHeaderKey;
    type InitialKey = <S as CryptoSuite>::InitialKey;
    type InitialHeaderKey = <S as CryptoSuite>::InitialHeaderKey;
    type ZeroRttKey = <S as CryptoSuite>::ZeroRttKey;
    type ZeroRttHeaderKey = <S as CryptoSuite>::ZeroRttHeaderKey;
    type OneRttKey = <S as CryptoSuite>::OneRttKey;
    type OneRttHeaderKey = <S as CryptoSuite>::OneRttHeaderKey;
    type RetryKey = <S as CryptoSuite>::RetryKey;
}

struct SlowContext<'a, Inner>(&'a mut Inner);

impl<I, S: tls::Session> tls::Context<S> for SlowContext<'_, I>
where
    I: tls::Context<SlowSession<S>>,
{
    fn on_client_application_params(
        &mut self,
        client_params: tls::ApplicationParameters,
        server_params: &mut Vec<u8>,
    ) -> Result<(), s2n_quic_core::transport::Error> {
        self.0
            .on_client_application_params(client_params, server_params)
    }

    fn on_handshake_keys(
        &mut self,
        key: <S as CryptoSuite>::HandshakeKey,
        header_key: <S as CryptoSuite>::HandshakeHeaderKey,
    ) -> Result<(), s2n_quic_core::transport::Error> {
        self.0.on_handshake_keys(key, header_key)
    }

    fn on_zero_rtt_keys(
        &mut self,
        key: <S>::ZeroRttKey,
        header_key: <S>::ZeroRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), s2n_quic_core::transport::Error> {
        self.0
            .on_zero_rtt_keys(key, header_key, application_parameters)
    }

    fn on_one_rtt_keys(
        &mut self,
        key: <S>::OneRttKey,
        header_key: <S>::OneRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), s2n_quic_core::transport::Error> {
        self.0
            .on_one_rtt_keys(key, header_key, application_parameters)
    }

    fn on_server_name(
        &mut self,
        server_name: s2n_quic_core::application::ServerName,
    ) -> Result<(), s2n_quic_core::transport::Error> {
        self.0.on_server_name(server_name)
    }

    fn on_application_protocol(
        &mut self,
        application_protocol: tls::Bytes,
    ) -> Result<(), s2n_quic_core::transport::Error> {
        self.0.on_application_protocol(application_protocol)
    }

    fn on_handshake_complete(&mut self) -> Result<(), s2n_quic_core::transport::Error> {
        self.0.on_handshake_complete()
    }

    fn on_tls_exporter_ready(
        &mut self,
        session: &impl tls::TlsSession,
    ) -> Result<(), s2n_quic_core::transport::Error> {
        self.0.on_tls_exporter_ready(session)
    }

    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<tls::Bytes> {
        self.0.receive_initial(max_len)
    }

    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<tls::Bytes> {
        self.0.receive_handshake(max_len)
    }

    fn receive_application(&mut self, max_len: Option<usize>) -> Option<tls::Bytes> {
        self.0.receive_application(max_len)
    }

    fn can_send_initial(&self) -> bool {
        self.0.can_send_initial()
    }

    fn send_initial(&mut self, transmission: tls::Bytes) {
        self.0.send_initial(transmission);
    }

    fn can_send_handshake(&self) -> bool {
        self.0.can_send_handshake()
    }

    fn send_handshake(&mut self, transmission: tls::Bytes) {
        self.0.send_handshake(transmission);
    }

    fn can_send_application(&self) -> bool {
        self.0.can_send_application()
    }

    fn send_application(&mut self, transmission: tls::Bytes) {
        self.0.send_application(transmission)
    }

    fn waker(&self) -> &core::task::Waker {
        self.0.waker()
    }

    fn on_key_exchange_group(
        &mut self,
        named_group: tls::NamedGroup,
    ) -> Result<(), s2n_quic_core::transport::Error> {
        self.0.on_key_exchange_group(named_group)
    }

    fn on_tls_context(&mut self, context: Box<dyn std::any::Any + Send>) {
        self.0.on_tls_context(context)
    }
}
