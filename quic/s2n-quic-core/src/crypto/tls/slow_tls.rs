// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use crate::{
    application,
    crypto::{tls, CryptoSuite},
    transport,
};
use alloc::{boxed::Box, vec::Vec};
use core::{any::Any, task::Poll};

const DEFER_COUNT: u8 = 3;

pub struct SlowEndpoint<E: tls::Endpoint> {
    endpoint: E,
}

impl<E: tls::Endpoint> SlowEndpoint<E> {
    pub fn new(endpoint: E) -> Self {
        SlowEndpoint { endpoint }
    }
}

impl<E: tls::Endpoint> tls::Endpoint for SlowEndpoint<E> {
    type Session = SlowSession<<E as tls::Endpoint>::Session>;

    fn new_server_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session {
        let inner_session = self.endpoint.new_server_session(transport_parameters);
        SlowSession {
            defer: DEFER_COUNT,
            inner_session,
        }
    }

    fn new_client_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        server_name: application::ServerName,
    ) -> Self::Session {
        let inner_session = self
            .endpoint
            .new_client_session(transport_parameters, server_name);
        SlowSession {
            defer: DEFER_COUNT,
            inner_session,
        }
    }

    fn max_tag_length(&self) -> usize {
        self.endpoint.max_tag_length()
    }
}

// SlowSession is a test TLS provider that is slow, namely, for each call to poll,
// it returns Poll::Pending several times before actually polling the real TLS library.
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
        self.defer = DEFER_COUNT;
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
    ) -> Result<(), transport::Error> {
        self.0
            .on_client_application_params(client_params, server_params)
    }

    fn on_handshake_keys(
        &mut self,
        key: <S as CryptoSuite>::HandshakeKey,
        header_key: <S as CryptoSuite>::HandshakeHeaderKey,
    ) -> Result<(), transport::Error> {
        self.0.on_handshake_keys(key, header_key)
    }

    fn on_zero_rtt_keys(
        &mut self,
        key: <S>::ZeroRttKey,
        header_key: <S>::ZeroRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), transport::Error> {
        self.0
            .on_zero_rtt_keys(key, header_key, application_parameters)
    }

    fn on_one_rtt_keys(
        &mut self,
        key: <S>::OneRttKey,
        header_key: <S>::OneRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), transport::Error> {
        self.0
            .on_one_rtt_keys(key, header_key, application_parameters)
    }

    fn on_server_name(
        &mut self,
        server_name: application::ServerName,
    ) -> Result<(), transport::Error> {
        self.0.on_server_name(server_name)
    }

    fn on_application_protocol(
        &mut self,
        application_protocol: tls::Bytes,
    ) -> Result<(), transport::Error> {
        self.0.on_application_protocol(application_protocol)
    }

    fn on_handshake_complete(&mut self) -> Result<(), transport::Error> {
        self.0.on_handshake_complete()
    }

    fn on_tls_exporter_ready(
        &mut self,
        session: &impl tls::TlsSession,
    ) -> Result<(), transport::Error> {
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
    ) -> Result<(), transport::Error> {
        self.0.on_key_exchange_group(named_group)
    }

    fn on_tls_context(&mut self, context: Box<dyn Any + Send>) {
        self.0.on_tls_context(context)
    }
}
