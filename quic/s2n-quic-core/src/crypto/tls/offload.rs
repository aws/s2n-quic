// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use crate::{
    application,
    crypto::{
        tls::{self, ConnectionInfo, NamedGroup, TlsSession},
        CryptoSuite,
    },
    sync::spsc::{channel, Receiver, SendSlice, Sender},
    transport,
};
use alloc::{boxed::Box, collections::vec_deque::VecDeque, sync::Arc, vec::Vec};
use core::{any::Any, future::Future, task::Poll};
use std::sync::Mutex;

/// Trait used for spawning async tasks corresponding to TLS operations. Each task will signify TLS work
/// that needs to be done per QUIC connection.
pub trait Executor {
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static);
}

/// Allows access to the TlsSession on handshake failure and when the exporter secret is ready.
pub trait ExporterHandler {
    fn on_tls_handshake_failed(
        &self,
        session: &impl TlsSession,
        e: &(dyn core::error::Error + Send + Sync + 'static),
    ) -> Option<Box<dyn Any + Send>>;
    fn on_tls_exporter_ready(&self, session: &impl TlsSession) -> Option<Box<dyn Any + Send>>;
}

// Most people don't need the TlsSession so we ignore these callbacks by default
impl ExporterHandler for () {
    fn on_tls_handshake_failed(
        &self,
        _session: &impl TlsSession,
        _e: &(dyn core::error::Error + Send + Sync + 'static),
    ) -> Option<Box<dyn std::any::Any + Send>> {
        None
    }

    fn on_tls_exporter_ready(
        &self,
        _session: &impl TlsSession,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        None
    }
}

pub struct OffloadEndpoint<E: tls::Endpoint, X: Executor, H: ExporterHandler> {
    inner: E,
    executor: X,
    exporter: H,
    channel_capacity: usize,
}

impl<E: tls::Endpoint, X: Executor, H: ExporterHandler> OffloadEndpoint<E, X, H> {
    pub fn new(inner: E, executor: X, exporter: H, channel_capacity: usize) -> Self {
        Self {
            inner,
            executor,
            exporter,
            channel_capacity,
        }
    }
}

impl<E, X, H> tls::Endpoint for OffloadEndpoint<E, X, H>
where
    E: tls::Endpoint,
    X: Executor + Send + 'static,
    H: ExporterHandler + Send + 'static + Sync + Clone,
{
    type Session = OffloadSession<<E as tls::Endpoint>::Session>;

    fn new_server_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        connection_info: ConnectionInfo,
    ) -> Self::Session {
        OffloadSession::new(
            self.inner
                .new_server_session(transport_parameters, connection_info),
            &self.executor,
            self.exporter.clone(),
            self.channel_capacity,
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
            self.exporter.clone(),
            self.channel_capacity,
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
    allowed_to_send: Arc<Mutex<AllowedToSend>>,
}

impl<S: tls::Session + 'static> OffloadSession<S> {
    fn new(
        mut inner: S,
        executor: &impl Executor,
        exporter: impl ExporterHandler + Sync + Send + 'static + Clone,
        channel_capacity: usize,
    ) -> Self {
        let (mut send_to_quic, recv_from_tls): (Sender<Request<S>>, Receiver<Request<S>>) =
            channel(channel_capacity);
        let (send_to_tls, mut recv_from_quic): (Sender<Response>, Receiver<Response>) =
            channel(channel_capacity);
        let allowed_to_send = Arc::new(Mutex::new(AllowedToSend::default()));
        let clone = allowed_to_send.clone();

        let future = async move {
            let mut initial_data = VecDeque::default();
            let mut handshake_data = VecDeque::default();
            let mut application_data = VecDeque::default();

            core::future::poll_fn(|ctx| {
                match send_to_quic.poll_slice(ctx) {
                    Poll::Ready(res) => match res {
                        Ok(send_slice) => {
                            let allowed_to_send = *allowed_to_send.lock().unwrap();

                            let mut context = RemoteContext {
                                send_to_quic: send_slice,
                                waker: ctx.waker().clone(),
                                initial_data: &mut initial_data,
                                handshake_data: &mut handshake_data,
                                application_data: &mut application_data,
                                exporter_handler: exporter.clone(),
                                allowed_to_send,
                                error: None,
                            };

                            while let Poll::Ready(res) = recv_from_quic.poll_slice(ctx) {
                                match res {
                                    Ok(mut recv_slice) => {
                                        while let Some(response) = recv_slice.pop() {
                                            match response {
                                                Response::Initial(data) => {
                                                    context.initial_data.push_back(data);
                                                }
                                                Response::Handshake(data) => {
                                                    context.handshake_data.push_back(data);
                                                }
                                                Response::Application(data) => {
                                                    context.application_data.push_back(data)
                                                }
                                                Response::SendStatusChanged => (),
                                            }
                                        }
                                    }
                                    Err(_) => {
                                        // For whatever reason the QUIC side decided to drop this channel. In this case
                                        // we complete the future.
                                        return Poll::Ready(());
                                    }
                                }
                            }

                            let res = inner.poll(&mut context);
                            // Either there was an error or the handshake has finished if TLS returned Poll::Ready.
                            // Notify the QUIC side accordingly.
                            if let Poll::Ready(res) = res {
                                let request = match res {
                                    Ok(_) => Request::TlsDone,
                                    Err(e) => Request::TlsError(e),
                                };
                                let _ = context.send_to_quic.push(request);
                            }

                            // We also need to notify the QUIC side of any stored errors that we have.
                            if let Some(error) = context.error {
                                let _ = context.send_to_quic.push(Request::TlsError(error));
                            }

                            // We've already sent the Result to the QUIC side so we can just map it out here.
                            res.map(|_| ())
                        }
                        Err(_) => {
                            // For whatever reason the QUIC side decided to drop this channel. In this case
                            // we complete the future.
                            Poll::Ready(())
                        }
                    },
                    Poll::Pending => Poll::Pending,
                }
            })
            .await;
        };
        executor.spawn(future);

        Self {
            recv_from_tls,
            send_to_tls,
            allowed_to_send: clone,
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

        match self.recv_from_tls.poll_slice(&mut ctx) {
            Poll::Ready(res) => match res {
                Ok(mut slice) => {
                    while let Some(request) = slice.pop() {
                        match request {
                            Request::HandshakeKeys(key, header_key) => {
                                context.on_handshake_keys(key, header_key)?;
                            }
                            Request::ServerName(server_name) => {
                                context.on_server_name(server_name)?
                            }
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
                }
                Err(_) => {
                    // For whatever reason the TLS task was cancelled. We cannot continue the handshake.
                    return Poll::Ready(Err(transport::Error::from(tls::Error::HANDSHAKE_FAILURE)));
                }
            },
            Poll::Pending => (),
        }

        let mut allowed_to_send = self.allowed_to_send.lock().unwrap();
        let mut state_change = false;
        if allowed_to_send.can_send_initial != context.can_send_initial()
            || allowed_to_send.can_send_handshake != context.can_send_handshake()
            || allowed_to_send.can_send_application != context.can_send_application()
        {
            *allowed_to_send = AllowedToSend {
                can_send_initial: context.can_send_initial(),
                can_send_handshake: context.can_send_handshake(),
                can_send_application: context.can_send_application(),
            };
            state_change = true;
        }
        // Drop the lock ASAP
        drop(allowed_to_send);

        match self.send_to_tls.poll_slice(&mut ctx) {
            Poll::Ready(res) => match res {
                Ok(mut slice) => {
                    if let Some(resp) = context.receive_initial(None) {
                        let _ = slice.push(Response::Initial(resp));
                    }

                    if let Some(resp) = context.receive_handshake(None) {
                        let _ = slice.push(Response::Handshake(resp));
                    }

                    if let Some(resp) = context.receive_application(None) {
                        let _ = slice.push(Response::Application(resp));
                    }

                    if state_change {
                        let _ = slice.push(Response::SendStatusChanged);
                    }
                }
                Err(_) => {
                    // For whatever reason the TLS task was cancelled. We cannot continue the handshake.
                    return Poll::Ready(Err(transport::Error::from(tls::Error::HANDSHAKE_FAILURE)));
                }
            },
            Poll::Pending => (),
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

#[derive(Debug, Default, Copy, Clone)]
struct AllowedToSend {
    can_send_initial: bool,
    can_send_handshake: bool,
    can_send_application: bool,
}

const SLICE_ERROR: crate::transport::Error =
    crate::transport::Error::INTERNAL_ERROR.with_reason("Slice is full");

#[derive(Debug)]
struct RemoteContext<'a, Request, H> {
    send_to_quic: SendSlice<'a, Request>,
    initial_data: &'a mut VecDeque<bytes::Bytes>,
    handshake_data: &'a mut VecDeque<bytes::Bytes>,
    application_data: &'a mut VecDeque<bytes::Bytes>,
    waker: core::task::Waker,
    allowed_to_send: AllowedToSend,
    exporter_handler: H,
    error: Option<crate::transport::Error>,
}

impl<S: CryptoSuite, H: ExporterHandler> tls::Context<S> for RemoteContext<'_, Request<S>, H> {
    fn on_client_application_params(
        &mut self,
        client_params: tls::ApplicationParameters,
        server_params: &mut alloc::vec::Vec<u8>,
    ) -> Result<(), crate::transport::Error> {
        match self.send_to_quic.push(Request::ClientParams(
            client_params.transport_parameters.to_vec(),
            server_params.to_vec(),
        )) {
            Ok(_) => return Ok(()),
            Err(_) => self.error = Some(SLICE_ERROR),
        }
        Ok(())
    }

    fn on_handshake_keys(
        &mut self,
        key: <S as CryptoSuite>::HandshakeKey,
        header_key: <S as CryptoSuite>::HandshakeHeaderKey,
    ) -> Result<(), crate::transport::Error> {
        match self
            .send_to_quic
            .push(Request::HandshakeKeys(key, header_key))
        {
            Ok(_) => return Ok(()),
            Err(_) => self.error = Some(SLICE_ERROR),
        }
        Ok(())
    }

    fn on_zero_rtt_keys(
        &mut self,
        key: <S as CryptoSuite>::ZeroRttKey,
        header_key: <S as CryptoSuite>::ZeroRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), crate::transport::Error> {
        match self.send_to_quic.push(Request::ZeroRtt(
            key,
            header_key,
            application_parameters.transport_parameters.to_vec(),
        )) {
            Ok(_) => (),
            Err(_) => self.error = Some(SLICE_ERROR),
        }
        Ok(())
    }

    fn on_one_rtt_keys(
        &mut self,
        key: <S as CryptoSuite>::OneRttKey,
        header_key: <S as CryptoSuite>::OneRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), crate::transport::Error> {
        match self.send_to_quic.push(Request::OneRttKeys(
            key,
            header_key,
            application_parameters.transport_parameters.to_vec(),
        )) {
            Ok(_) => (),
            Err(_) => self.error = Some(SLICE_ERROR),
        }
        Ok(())
    }

    fn on_server_name(
        &mut self,
        server_name: crate::application::ServerName,
    ) -> Result<(), crate::transport::Error> {
        match self.send_to_quic.push(Request::ServerName(server_name)) {
            Ok(_) => (),
            Err(_) => self.error = Some(SLICE_ERROR),
        }
        Ok(())
    }

    fn on_application_protocol(
        &mut self,
        application_protocol: bytes::Bytes,
    ) -> Result<(), crate::transport::Error> {
        match self
            .send_to_quic
            .push(Request::ApplicationProtocol(application_protocol))
        {
            Ok(_) => (),
            Err(_) => self.error = Some(SLICE_ERROR),
        }
        Ok(())
    }

    fn on_key_exchange_group(
        &mut self,
        named_group: tls::NamedGroup,
    ) -> Result<(), crate::transport::Error> {
        match self
            .send_to_quic
            .push(Request::KeyExchangeGroup(named_group))
        {
            Ok(_) => (),
            Err(_) => self.error = Some(SLICE_ERROR),
        }
        Ok(())
    }

    fn on_handshake_complete(&mut self) -> Result<(), crate::transport::Error> {
        match self.send_to_quic.push(Request::HandshakeComplete) {
            Ok(_) => (),
            Err(_) => self.error = Some(SLICE_ERROR),
        }

        Ok(())
    }

    fn on_tls_context(&mut self, _context: Box<dyn Any + Send>) {
        unimplemented!("TLS Context is not supported in Offload implementation");
    }

    fn on_tls_exporter_ready(
        &mut self,
        session: &impl TlsSession,
    ) -> Result<(), crate::transport::Error> {
        if let Some(context) = self.exporter_handler.on_tls_exporter_ready(session) {
            match self.send_to_quic.push(Request::TlsContext(context)) {
                Ok(_) => (),
                Err(_) => self.error = Some(SLICE_ERROR),
            }
        }

        Ok(())
    }

    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        gimme_bytes(max_len, self.initial_data)
    }

    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        gimme_bytes(max_len, self.handshake_data)
    }

    fn receive_application(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        gimme_bytes(max_len, self.application_data)
    }

    fn can_send_initial(&self) -> bool {
        self.allowed_to_send.can_send_initial
    }

    fn send_initial(&mut self, transmission: bytes::Bytes) {
        if self
            .send_to_quic
            .push(Request::SendInitial(transmission))
            .is_err()
        {
            self.error = Some(SLICE_ERROR);
        }
    }

    fn can_send_handshake(&self) -> bool {
        self.allowed_to_send.can_send_handshake
    }

    fn send_handshake(&mut self, transmission: bytes::Bytes) {
        if self
            .send_to_quic
            .push(Request::SendHandshake(transmission))
            .is_err()
        {
            self.error = Some(SLICE_ERROR);
        }
    }

    fn can_send_application(&self) -> bool {
        self.allowed_to_send.can_send_application
    }

    fn send_application(&mut self, transmission: bytes::Bytes) {
        if self
            .send_to_quic
            .push(Request::SendApplication(transmission))
            .is_err()
        {
            self.error = Some(SLICE_ERROR);
        }
    }

    fn waker(&self) -> &core::task::Waker {
        &self.waker
    }

    fn on_tls_handshake_failed(
        &mut self,
        session: &impl tls::TlsSession,
        e: &(dyn core::error::Error + Send + Sync + 'static),
    ) -> Result<(), crate::transport::Error> {
        if let Some(context) = self.exporter_handler.on_tls_handshake_failed(session, e) {
            match self.send_to_quic.push(Request::TlsContext(context)) {
                Ok(_) => (),
                Err(_) => self.error = Some(SLICE_ERROR),
            }
        }
        Ok(())
    }
}

fn gimme_bytes(max_len: Option<usize>, vec: &mut VecDeque<bytes::Bytes>) -> Option<bytes::Bytes> {
    let bytes = vec.pop_front();
    if let Some(mut bytes) = bytes {
        if let Some(max_len) = max_len {
            if bytes.len() > max_len {
                let remainder = bytes.split_off(max_len);
                vec.push_front(remainder);
            }
        }
        return Some(bytes);
    }
    None
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
    SendStatusChanged,
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
            Response::SendStatusChanged => write!(f, "SendStatusChanged"),
        }
    }
}
