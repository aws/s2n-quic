// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use crate::{
    application,
    crypto::{
        tls::{self, NamedGroup},
        CryptoSuite,
    },
    sync::spsc::{channel, Receiver, Sender},
    transport,
};
use alloc::{boxed::Box, vec::Vec};
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
pub struct OffloadSession<S: tls::Session> {
    recv_from_tls: Receiver<Msg<S>>,
    send_to_tls: Sender<Msg<S>>,
}

impl<S: tls::Session + 'static> OffloadSession<S> {
    fn new(mut inner: S, executor: &impl Executor) -> Self {
        let (mut send_to_quic, recv_from_tls): (Sender<Msg<S>>, Receiver<Msg<S>>) = channel(10);
        let (send_to_tls, mut recv_from_quic): (Sender<Msg<S>>, Receiver<Msg<S>>) = channel(10);

        let future = async move {
            loop {
                if (recv_from_quic.acquire().await).is_err() {
                    break;
                }

                let res = core::future::poll_fn(|ctx| {
                    let mut slice = recv_from_quic.slice();
                    let mut context = RemoteContext {
                        send_to_quic: &mut send_to_quic,
                        waker: ctx.waker().clone(),
                        initial_data: Vec::default(),
                        handshake_data: Vec::default(),
                        application_data: Vec::default(),
                        can_send_initial: false,
                        can_send_handshake: false,
                        can_send_application: false,
                    };
                    while let Some(msg) = slice.pop() {
                        match msg {
                            Msg::ResponseInitial(data) => {
                                context.initial_data.push(data);
                            }
                            Msg::ResponseHandshake(data) => context.handshake_data.push(data),
                            Msg::ResponseApplication(data) => context.application_data.push(data),
                            Msg::CanSendHandshake(bool) => context.can_send_handshake = bool,
                            Msg::CanSendInitial(bool) => context.can_send_initial = bool,
                            Msg::CanSendApplication(bool) => context.can_send_application = bool,
                            _ => (),
                        }
                    }
                    let res = inner.poll(&mut context);

                    // If the TLS implementation is complete, either there was an error or the handshake has finished
                    if let Poll::Ready(res) = res {
                        if let Poll::Ready(Ok(mut slice)) = send_to_quic.poll_slice(ctx) {
                            match res {
                                Ok(()) => {
                                    let _ = slice.push(Msg::TlsDone);
                                }

                                Err(e) => {
                                    let _ = slice.push(Msg::TlsError(e));
                                }
                            }
                        }
                    }
                    Poll::Ready(res)
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
            while let Some(msg) = slice.pop() {
                match msg {
                    Msg::HandshakeKeys(key, header_key) => {
                        context.on_handshake_keys(key, header_key)?;
                    }
                    Msg::ServerName(server_name) => context.on_server_name(server_name)?,
                    Msg::SendInitial(bytes) => context.send_initial(bytes),
                    Msg::ClientParams(client_params, mut server_params) => context
                        .on_client_application_params(
                            tls::ApplicationParameters {
                                transport_parameters: &client_params,
                            },
                            &mut server_params,
                        )?,
                    Msg::ApplicationProtocol(bytes) => {
                        context.on_application_protocol(bytes)?;
                    }
                    Msg::KeyExchangeGroup(named_group) => {
                        context.on_key_exchange_group(named_group)?;
                    }
                    Msg::OneRttKeys(key, header_key, transport_parameters) => context
                        .on_one_rtt_keys(
                            key,
                            header_key,
                            tls::ApplicationParameters {
                                transport_parameters: &transport_parameters,
                            },
                        )?,
                    Msg::SendHandshake(bytes) => {
                        context.send_handshake(bytes);
                    }
                    Msg::HandshakeComplete => {
                        context.on_handshake_complete()?;
                    }
                    Msg::TlsDone => {
                        return Poll::Ready(Ok(()));
                    }
                    Msg::ZeroRtt(key, header_key, transport_parameters) => {
                        context.on_zero_rtt_keys(
                            key,
                            header_key,
                            tls::ApplicationParameters {
                                transport_parameters: &transport_parameters,
                            },
                        )?;
                    }
                    Msg::TlsContext(ctx) => {
                        context.on_tls_context(ctx);
                    }
                    Msg::SendApplication(transmission) => {
                        context.send_application(transmission);
                    }
                    Msg::TlsError(e) => return Poll::Ready(Err(e)),
                    // No other messages are sent from the TLS side
                    _ => (),
                }
            }
        };

        // Send any TLS data to the TLS side. Note that we have to schedule a wakeup on the quic endpoint each time
        // we pass data to the TLS side. This is because TLS may generate some data that needs to be sent
        // in response to the data quic provides (for example, reading the client hello causes the TLS side
        // to produce a server hello, which will then need to be sent).
        if let Poll::Ready(Ok(mut slice)) = self.send_to_tls.poll_slice(&mut ctx) {
            if let Some(resp) = context.receive_initial(None) {
                context.waker().wake_by_ref();
                let _ = slice.push(Msg::ResponseInitial(resp));
            }

            if let Some(resp) = context.receive_handshake(None) {
                context.waker().wake_by_ref();
                let _ = slice.push(Msg::ResponseHandshake(resp));
            }

            if let Some(resp) = context.receive_application(None) {
                context.waker().wake_by_ref();
                let _ = slice.push(Msg::ResponseApplication(resp));
            }

            let _ = slice.push(Msg::CanSendInitial(context.can_send_initial()));
            let _ = slice.push(Msg::CanSendHandshake(context.can_send_handshake()));
            let _ = slice.push(Msg::CanSendApplication(context.can_send_application()));
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
struct RemoteContext<'a, Msg> {
    send_to_quic: &'a mut Sender<Msg>,
    waker: core::task::Waker,
    initial_data: Vec<bytes::Bytes>,
    handshake_data: Vec<bytes::Bytes>,
    application_data: Vec<bytes::Bytes>,
    can_send_initial: bool,
    can_send_handshake: bool,
    can_send_application: bool,
}

impl<'a, S: CryptoSuite> tls::Context<S> for RemoteContext<'a, Msg<S>> {
    fn on_client_application_params(
        &mut self,
        client_params: tls::ApplicationParameters,
        server_params: &mut alloc::vec::Vec<u8>,
    ) -> Result<(), crate::transport::Error> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::ClientParams(
                client_params.transport_parameters.to_vec(),
                server_params.to_vec(),
            ));
        }
        Ok(())
    }

    fn on_handshake_keys(
        &mut self,
        key: <S as CryptoSuite>::HandshakeKey,
        header_key: <S as CryptoSuite>::HandshakeHeaderKey,
    ) -> Result<(), crate::transport::Error> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::HandshakeKeys(key, header_key));
        }
        Ok(())
    }

    fn on_zero_rtt_keys(
        &mut self,
        key: <S as CryptoSuite>::ZeroRttKey,
        header_key: <S as CryptoSuite>::ZeroRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), crate::transport::Error> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::ZeroRtt(
                key,
                header_key,
                application_parameters.transport_parameters.to_vec(),
            ));
        }
        Ok(())
    }

    fn on_one_rtt_keys(
        &mut self,
        key: <S as CryptoSuite>::OneRttKey,
        header_key: <S as CryptoSuite>::OneRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), crate::transport::Error> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::OneRttKeys(
                key,
                header_key,
                application_parameters.transport_parameters.to_vec(),
            ));
        }
        Ok(())
    }

    fn on_server_name(
        &mut self,
        server_name: crate::application::ServerName,
    ) -> Result<(), crate::transport::Error> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::ServerName(server_name));
        }
        Ok(())
    }

    fn on_application_protocol(
        &mut self,
        application_protocol: bytes::Bytes,
    ) -> Result<(), crate::transport::Error> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::ApplicationProtocol(application_protocol));
        }
        Ok(())
    }

    fn on_key_exchange_group(
        &mut self,
        named_group: tls::NamedGroup,
    ) -> Result<(), crate::transport::Error> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::KeyExchangeGroup(named_group));
        }
        Ok(())
    }

    fn on_handshake_complete(&mut self) -> Result<(), crate::transport::Error> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::HandshakeComplete);
        }

        Ok(())
    }

    fn on_tls_context(&mut self, context: Box<dyn Any + Send>) {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::TlsContext(context));
        }
    }

    fn on_tls_exporter_ready(
        &mut self,
        _session: &impl tls::TlsSession,
    ) -> Result<(), crate::transport::Error> {
        // Not sure what we can do here
        Ok(())
    }

    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        if let Some(max_len) = max_len {
            if !self.initial_data.is_empty() {
                let mut bytes = self.initial_data.remove(0);
                if bytes.len() > max_len {
                    let remainder = bytes.split_off(max_len);
                    self.initial_data.insert(0, remainder);
                }

                return Some(bytes);
            }
        }

        None
    }

    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        if let Some(max_len) = max_len {
            if !self.handshake_data.is_empty() {
                let mut bytes = self.handshake_data.remove(0);
                if bytes.len() > max_len {
                    let remainder = bytes.split_off(max_len);
                    self.handshake_data.insert(0, remainder);
                }

                return Some(bytes);
            }
        }
        None
    }

    fn receive_application(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        if let Some(max_len) = max_len {
            if !self.application_data.is_empty() {
                let mut bytes = self.application_data.remove(0);
                if bytes.len() > max_len {
                    let remainder = bytes.split_off(max_len);
                    self.application_data.insert(0, remainder);
                }

                return Some(bytes);
            }
        }

        None
    }

    fn can_send_initial(&self) -> bool {
        self.can_send_initial
    }

    fn send_initial(&mut self, transmission: bytes::Bytes) {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::SendInitial(transmission));
        }
    }

    fn can_send_handshake(&self) -> bool {
        self.can_send_handshake
    }

    fn send_handshake(&mut self, transmission: bytes::Bytes) {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::SendHandshake(transmission));
        }
    }

    fn can_send_application(&self) -> bool {
        self.can_send_application
    }

    fn send_application(&mut self, transmission: bytes::Bytes) {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(Ok(mut slice)) = self.send_to_quic.poll_slice(&mut cx) {
            let _ = slice.push(Msg::SendApplication(transmission));
        }
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

enum Msg<S: CryptoSuite> {
    ZeroRtt(
        <S as CryptoSuite>::ZeroRttKey,
        <S as CryptoSuite>::ZeroRttHeaderKey,
        Vec<u8>,
    ),
    ServerName(crate::application::ServerName),
    SendInitial(bytes::Bytes),
    ResponseInitial(bytes::Bytes),
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
    ResponseHandshake(bytes::Bytes),
    HandshakeComplete,
    TlsDone,
    TlsContext(Box<dyn Any + Send>),
    ResponseApplication(bytes::Bytes),
    SendApplication(bytes::Bytes),
    TlsError(transport::Error),
    CanSendInitial(bool),
    CanSendHandshake(bool),
    CanSendApplication(bool),
}

impl<S: CryptoSuite> alloc::fmt::Debug for Msg<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Msg::ServerName(_) => write!(f, "ServerName"),
            Msg::SendInitial(_) => write!(f, "SendInitial"),
            Msg::ResponseInitial(_) => write!(f, "ResponseInitial"),
            Msg::ClientParams(_, _) => write!(f, "ClientParams"),
            Msg::HandshakeKeys(_, _) => write!(f, "HandshakeKeys"),
            Msg::SendHandshake(_) => write!(f, "SendHandshake"),
            Msg::ApplicationProtocol(_) => write!(f, "ApplicationProtocol"),
            Msg::KeyExchangeGroup(_) => write!(f, "KeyExchangeGroup"),
            Msg::OneRttKeys(_, _, _) => write!(f, "OneRttKeys"),
            Msg::ResponseHandshake(_) => write!(f, "ResponseHandshake"),
            Msg::HandshakeComplete => write!(f, "HandshakeComplete"),
            Msg::TlsDone => write!(f, "TlsDone"),
            Msg::ZeroRtt(_, _, _) => write!(f, "ZeroRtt"),
            Msg::TlsContext(_) => write!(f, "TlsContext"),
            Msg::ResponseApplication(_) => write!(f, "ResponseApplication"),
            Msg::SendApplication(_) => write!(f, "SendApplication"),
            Msg::TlsError(_) => write!(f, "TlsError"),
            Msg::CanSendInitial(_) => write!(f, "CanSendInitial"),
            Msg::CanSendHandshake(_) => write!(f, "CanSendHandshake"),
            Msg::CanSendApplication(_) => write!(f, "CanSendApplication"),
        }
    }
}
