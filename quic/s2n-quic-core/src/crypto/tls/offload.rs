// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use crate::{
    application,
    crypto::{
        tls::{self, NamedGroup, TlsSession},
        CryptoSuite,
    },
    sync::spsc::{channel, Receiver, SendSlice, Sender},
    transport,
};
use alloc::{boxed::Box, collections::vec_deque::VecDeque};
use core::{
    any::Any,
    future::Future,
    task::{Context, Poll},
};

pub trait Executor {
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static);
}

pub struct OffloadEndpoint<E: tls::Endpoint, X: Executor> {
    inner: E,
    executor: X,
}

impl<E: tls::Endpoint, X: Executor> OffloadEndpoint<E, X> {
    pub fn new(inner: E, executor: X) -> Self {
        Self { inner, executor }
    }
}

impl<E: tls::Endpoint, X: Executor + Send + 'static> tls::Endpoint for OffloadEndpoint<E, X> {
    type Session = OffloadSession<<E as tls::Endpoint>::Session>;

    fn new_server_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session {
        OffloadSession::new(
            self.inner.new_server_session(transport_parameters),
            &self.executor,
        )
    }

    fn new_client_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        server_name: application::ServerName,
    ) -> Self::Session {
        OffloadSession::new(
            self.inner
                .new_client_session(transport_parameters, server_name),
            &self.executor,
        )
    }

    fn max_tag_length(&self) -> usize {
        self.inner.max_tag_length()
    }
}

#[derive(Debug)]
pub struct OffloadSession<S: CryptoSuite> {
    recv_from_tls: Receiver<Request<S>>,
    send_to_tls: Sender<Response>,
}

impl<S: tls::Session + 'static> OffloadSession<S> {
    fn new(mut inner: S, executor: &impl Executor) -> Self {
        // A channel of size 10 is somewhat arbitrary. I haven't seen this limit be exceeded, but we
        // could raise this in the future if necessary.
        let (mut send_to_quic, recv_from_tls): (Sender<Request<S>>, Receiver<Request<S>>) =
            channel(10);
        let (send_to_tls, mut recv_from_quic): (Sender<Response>, Receiver<Response>) = channel(10);

        let future = async move {
            let mut initial_data = VecDeque::default();
            let mut handshake_data = VecDeque::default();
            let mut application_data = VecDeque::default();
            loop {
                if recv_from_quic.acquire().await.is_err() {
                    break;
                }

                let res = core::future::poll_fn(|mut ctx| {
                    if let Poll::Ready(Ok(send_slice)) = send_to_quic.poll_slice(&mut ctx) {
                        let mut context = RemoteContext {
                            send_to_quic: send_slice,
                            waker: ctx.waker().clone(),
                            initial_data: &mut initial_data,
                            handshake_data: &mut handshake_data,
                            application_data: &mut application_data,
                            can_send_initial: false,
                            can_send_handshake: false,
                            can_send_application: false,
                        };

                        let mut recv_slice = recv_from_quic.slice();
                        while let Some(response) = recv_slice.pop() {
                            match response {
                                Response::Initial(data) => {
                                    context.initial_data.push_back(data);
                                }
                                Response::Handshake(data) => context.handshake_data.push_back(data),
                                Response::Application(data) => {
                                    context.application_data.push_back(data)
                                }
                                Response::CanSendHandshake(bool) => {
                                    context.can_send_handshake = bool
                                }
                                Response::CanSendInitial(bool) => context.can_send_initial = bool,
                                Response::CanSendApplication(bool) => {
                                    context.can_send_application = bool
                                }
                            }
                        }
                        let res = inner.poll(&mut context);
                        // Either there was an error or the handshake has finished if TLS returned Poll::Ready.
                        // Notify the QUIC side accordingly.
                        if let Poll::Ready(res) = res {
                            match res {
                                Ok(()) => {
                                    let _ = context.send_to_quic.push(Request::TlsDone);
                                }

                                Err(e) => {
                                    let _ = context.send_to_quic.push(Request::TlsError(e));
                                }
                            }
                        }
                        return Poll::Ready(res);
                    }

                    Poll::Pending
                })
                .await;

                match res {
                    Poll::Ready(_) => {
                        return;
                    }
                    Poll::Pending => (),
                }
            }
        };
        executor.spawn(future);

        Self {
            recv_from_tls,
            send_to_tls,
        }
    }
}

impl<S: tls::Session> tls::Session for OffloadSession<S> {
    #[inline]
    fn poll<W>(&mut self, context: &mut W) -> Poll<Result<(), transport::Error>>
    where
        W: tls::Context<Self>,
    {
        let cloned_waker = &context.waker().clone();
        let mut ctx = core::task::Context::from_waker(cloned_waker);

        if let Poll::Ready(Ok(mut slice)) = self.recv_from_tls.poll_slice(&mut ctx) {
            while let Some(request) = slice.pop() {
                match request {
                    Request::HandshakeKeys(key, header_key) => {
                        context.on_handshake_keys(key, header_key)?;
                    }
                    Request::ServerName(server_name) => context.on_server_name(server_name)?,
                    Request::SendInitial(bytes) => context.send_initial(bytes),
                    Request::ClientParams(client_params, mut server_params) => context
                        .on_client_application_params(
                            tls::ApplicationParameters {
                                transport_parameters: &client_params,
                            },
                            &mut server_params,
                        )?,
                    Request::ApplicationProtocol(bytes) => {
                        context.on_application_protocol(bytes)?;
                    }
                    Request::KeyExchangeGroup(named_group) => {
                        context.on_key_exchange_group(named_group)?;
                    }
                    Request::OneRttKeys(key, header_key, transport_parameters) => context
                        .on_one_rtt_keys(
                            key,
                            header_key,
                            tls::ApplicationParameters {
                                transport_parameters: &transport_parameters,
                            },
                        )?,
                    Request::SendHandshake(bytes) => {
                        context.send_handshake(bytes);
                    }
                    Request::HandshakeComplete => {
                        context.on_handshake_complete()?;
                    }
                    Request::TlsDone => {
                        return Poll::Ready(Ok(()));
                    }
                    Request::ZeroRtt(key, header_key, transport_parameters) => {
                        context.on_zero_rtt_keys(
                            key,
                            header_key,
                            tls::ApplicationParameters {
                                transport_parameters: &transport_parameters,
                            },
                        )?;
                    }
                    Request::TlsContext(ctx) => {
                        context.on_tls_context(ctx);
                    }
                    Request::SendApplication(transmission) => {
                        context.send_application(transmission);
                    }
                    Request::TlsError(e) => return Poll::Ready(Err(e)),
                }
            }
        };

        // Send any TLS data that we have through the async TLS channel. Note that we schedule a wakeup
        // for the quic endpoint if we have data to give to TLS. This is because when TLS reads
        // a message, usually its next task is to send a message in response. So we wakeup the quic endpoint
        // so it is ready to send that response as quickly as possible.
        if let Poll::Ready(Ok(mut slice)) = self.send_to_tls.poll_slice(&mut ctx) {
            if let Some(resp) = context.receive_initial(None) {
                context.waker().wake_by_ref();
                let _ = slice.push(Response::Initial(resp));
            }

            if let Some(resp) = context.receive_handshake(None) {
                context.waker().wake_by_ref();
                let _ = slice.push(Response::Handshake(resp));
            }

            if let Some(resp) = context.receive_application(None) {
                context.waker().wake_by_ref();
                let _ = slice.push(Response::Application(resp));
            }

            let _ = slice.push(Response::CanSendInitial(context.can_send_initial()));
            let _ = slice.push(Response::CanSendHandshake(context.can_send_handshake()));
            let _ = slice.push(Response::CanSendApplication(context.can_send_application()));
        }

        Poll::Pending
    }
}

impl<S: tls::Session> CryptoSuite for OffloadSession<S> {
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

#[derive(Debug)]
struct RemoteContext<'a, Request> {
    send_to_quic: SendSlice<'a, Request>,
    initial_data: &'a mut VecDeque<bytes::Bytes>,
    handshake_data: &'a mut VecDeque<bytes::Bytes>,
    application_data: &'a mut VecDeque<bytes::Bytes>,
    can_send_initial: bool,
    can_send_handshake: bool,
    can_send_application: bool,
    waker: core::task::Waker,
}

impl<'a, S: CryptoSuite> tls::Context<S> for RemoteContext<'a, Request<S>> {
    fn on_client_application_params(
        &mut self,
        client_params: tls::ApplicationParameters,
        server_params: &mut alloc::vec::Vec<u8>,
    ) -> Result<(), crate::transport::Error> {
        let _ = self.send_to_quic.push(Request::ClientParams(
            client_params.transport_parameters.to_vec(),
            server_params.to_vec(),
        ));

        Ok(())
    }

    fn on_handshake_keys(
        &mut self,
        key: <S as CryptoSuite>::HandshakeKey,
        header_key: <S as CryptoSuite>::HandshakeHeaderKey,
    ) -> Result<(), crate::transport::Error> {
        let _ = self
            .send_to_quic
            .push(Request::HandshakeKeys(key, header_key));

        Ok(())
    }

    fn on_zero_rtt_keys(
        &mut self,
        key: <S as CryptoSuite>::ZeroRttKey,
        header_key: <S as CryptoSuite>::ZeroRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), crate::transport::Error> {
        let _ = self.send_to_quic.push(Request::ZeroRtt(
            key,
            header_key,
            application_parameters.transport_parameters.to_vec(),
        ));

        Ok(())
    }

    fn on_one_rtt_keys(
        &mut self,
        key: <S as CryptoSuite>::OneRttKey,
        header_key: <S as CryptoSuite>::OneRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), crate::transport::Error> {
        let _ = self.send_to_quic.push(Request::OneRttKeys(
            key,
            header_key,
            application_parameters.transport_parameters.to_vec(),
        ));

        Ok(())
    }

    fn on_server_name(
        &mut self,
        server_name: crate::application::ServerName,
    ) -> Result<(), crate::transport::Error> {
        let _ = self.send_to_quic.push(Request::ServerName(server_name));

        Ok(())
    }

    fn on_application_protocol(
        &mut self,
        application_protocol: bytes::Bytes,
    ) -> Result<(), crate::transport::Error> {
        let _ = self
            .send_to_quic
            .push(Request::ApplicationProtocol(application_protocol));

        Ok(())
    }

    fn on_key_exchange_group(
        &mut self,
        named_group: tls::NamedGroup,
    ) -> Result<(), crate::transport::Error> {
        let _ = self
            .send_to_quic
            .push(Request::KeyExchangeGroup(named_group));

        Ok(())
    }

    fn on_handshake_complete(&mut self) -> Result<(), crate::transport::Error> {
        let _ = self.send_to_quic.push(Request::HandshakeComplete);

        Ok(())
    }

    fn on_tls_context(&mut self, context: Box<dyn Any + Send>) {
        let _ = self.send_to_quic.push(Request::TlsContext(context));
    }

    fn on_tls_exporter_ready(
        &mut self,
        _session: &impl TlsSession,
    ) -> Result<(), crate::transport::Error> {
        // TODO

        Ok(())
    }

    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        let bytes = self.initial_data.pop_front();
        if let Some(mut bytes) = bytes {
            if let Some(max_len) = max_len {
                if bytes.len() > max_len {
                    let remainder = bytes.split_off(max_len);
                    self.initial_data.push_front(remainder);
                }
            }
            return Some(bytes);
        }

        None
    }

    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        let bytes = self.handshake_data.pop_front();
        if let Some(mut bytes) = bytes {
            if let Some(max_len) = max_len {
                if bytes.len() > max_len {
                    let remainder = bytes.split_off(max_len);
                    self.handshake_data.push_front(remainder);
                }
            }
            return Some(bytes);
        }
        None
    }

    fn receive_application(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        let bytes = self.application_data.pop_front();
        if let Some(mut bytes) = bytes {
            if let Some(max_len) = max_len {
                if bytes.len() > max_len {
                    let remainder = bytes.split_off(max_len);
                    self.application_data.push_front(remainder);
                }
            }
            return Some(bytes);
        }
        None
    }

    fn can_send_initial(&self) -> bool {
        self.can_send_initial
    }

    fn send_initial(&mut self, transmission: bytes::Bytes) {
        let _ = self.send_to_quic.push(Request::SendInitial(transmission));
    }

    fn can_send_handshake(&self) -> bool {
        self.can_send_handshake
    }

    fn send_handshake(&mut self, transmission: bytes::Bytes) {
        let _ = self.send_to_quic.push(Request::SendHandshake(transmission));
    }

    fn can_send_application(&self) -> bool {
        self.can_send_application
    }

    fn send_application(&mut self, transmission: bytes::Bytes) {
        let _ = self
            .send_to_quic
            .push(Request::SendApplication(transmission));
    }

    fn waker(&self) -> &core::task::Waker {
        &self.waker
    }

    fn on_tls_handshake_failed(
        &mut self,
        _session: &impl tls::TlsSession,
    ) -> Result<(), crate::transport::Error> {
        // Not sure what we can do here
        Ok(())
    }
}

enum Request<S: CryptoSuite> {
    ZeroRtt(
        <S as CryptoSuite>::ZeroRttKey,
        <S as CryptoSuite>::ZeroRttHeaderKey,
        Vec<u8>,
    ),
    ServerName(crate::application::ServerName),
    SendInitial(bytes::Bytes),
    ClientParams(Vec<u8>, Vec<u8>),
    HandshakeKeys(
        <S as CryptoSuite>::HandshakeKey,
        <S as CryptoSuite>::HandshakeHeaderKey,
    ),
    SendHandshake(bytes::Bytes),
    ApplicationProtocol(bytes::Bytes),
    KeyExchangeGroup(NamedGroup),
    OneRttKeys(
        <S as CryptoSuite>::OneRttKey,
        <S as CryptoSuite>::OneRttHeaderKey,
        Vec<u8>,
    ),
    HandshakeComplete,
    TlsDone,
    TlsContext(Box<dyn Any + Send>),
    SendApplication(bytes::Bytes),
    TlsError(transport::Error),
}

enum Response {
    Initial(bytes::Bytes),
    Handshake(bytes::Bytes),
    Application(bytes::Bytes),
    CanSendInitial(bool),
    CanSendHandshake(bool),
    CanSendApplication(bool),
}

impl<S: CryptoSuite> alloc::fmt::Debug for Request<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Request::ServerName(_) => write!(f, "ServerName"),
            Request::SendInitial(_) => write!(f, "SendInitial"),
            Request::ClientParams(_, _) => write!(f, "ClientParams"),
            Request::HandshakeKeys(_, _) => write!(f, "HandshakeKeys"),
            Request::SendHandshake(_) => write!(f, "SendHandshake"),
            Request::ApplicationProtocol(_) => write!(f, "ApplicationProtocol"),
            Request::KeyExchangeGroup(_) => write!(f, "KeyExchangeGroup"),
            Request::OneRttKeys(_, _, _) => write!(f, "OneRttKeys"),
            Request::HandshakeComplete => write!(f, "HandshakeComplete"),
            Request::TlsDone => write!(f, "TlsDone"),
            Request::ZeroRtt(_, _, _) => write!(f, "ZeroRtt"),
            Request::TlsContext(_) => write!(f, "TlsContext"),
            Request::SendApplication(_) => write!(f, "SendApplication"),
            Request::TlsError(_) => write!(f, "TlsError"),
        }
    }
}

impl alloc::fmt::Debug for Response {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Response::Initial(_) => write!(f, "ResponseInitial"),
            Response::Handshake(_) => write!(f, "ResponseHandshake"),
            Response::Application(_) => write!(f, "ResponseApplication"),
            Response::CanSendInitial(_) => write!(f, "CanSendInitial"),
            Response::CanSendHandshake(_) => write!(f, "CanSendHandshake"),
            Response::CanSendApplication(_) => write!(f, "CanSendApplication"),
        }
    }
}
