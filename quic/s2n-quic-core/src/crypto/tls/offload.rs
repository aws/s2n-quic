// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use crate::{
    application,
    crypto::{
        tls::{self, NamedGroup, Session},
        CryptoSuite,
    },
    transport,
};
use alloc::{sync::Arc, task::Wake, vec, vec::Vec};
use core::{
    any::Any,
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use futures::{prelude::Stream, task};
use futures_channel::{
    mpsc::{UnboundedReceiver, UnboundedSender},
    oneshot::{Receiver, Sender},
};
use std::thread;

type SessionProducer<E> = (
    <E as tls::Endpoint>::Session,
    UnboundedSender<Request<<E as tls::Endpoint>::Session>>,
);
pub struct OffloadEndpoint<E: tls::Endpoint> {
    new_session: UnboundedSender<SessionProducer<E>>,
    _thread: thread::JoinHandle<()>,
    inner: E,
    remote_thread_waker: Waker,
}

impl<E: tls::Endpoint> OffloadEndpoint<E> {
    pub fn new(inner: E) -> Self {
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<SessionProducer<E>>();

        let handle = thread::spawn(move || {
            let mut sessions = vec![];
            let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));

            loop {
                let mut cx = Context::from_waker(&waker);

                // Add incoming sessions to queue
                while let Poll::Ready(Some((new_session, tx))) =
                    Pin::new(&mut rx).poll_next(&mut cx)
                {
                    sessions.push((
                        new_session,
                        RemoteContext {
                            tx,
                            waker: waker.clone(),
                            receive_initial: AsyncRequest::empty(),
                            receive_handshake: AsyncRequest::empty(),
                            receive_application: AsyncRequest::empty(),

                            can_send_initial: AsyncRequest::empty(),
                            can_send_handshake: AsyncRequest::empty(),
                            can_send_application: AsyncRequest::empty(),
                        },
                    ))
                }

                let mut next_sessions = vec![];

                // Make progress on all stored sessions, prioritizing existing sessions over incoming ones
                for (mut session, mut ctx) in sessions {
                    match session.poll(&mut ctx) {
                        Poll::Ready(res) => {
                            let _ = ctx.tx.unbounded_send(Request::Done(session, res));
                        }
                        Poll::Pending => {
                            next_sessions.push((session, ctx));
                        }
                    }
                }
                sessions = next_sessions;

                thread::park();
            }
        });

        Self {
            inner,
            remote_thread_waker: task::Waker::from(Arc::new(ThreadWaker(handle.thread().clone()))),
            _thread: handle,
            new_session: tx,
        }
    }
}

struct ThreadWaker(thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

impl<E: tls::Endpoint> tls::Endpoint for OffloadEndpoint<E> {
    type Session = OffloadSession<<E as tls::Endpoint>::Session>;

    fn new_server_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session {
        OffloadSession::new(
            self.inner.new_server_session(transport_parameters),
            &mut self.new_session,
            self.remote_thread_waker.clone(),
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
            &mut self.new_session,
            self.remote_thread_waker.clone(),
        )
    }

    fn max_tag_length(&self) -> usize {
        self.inner.max_tag_length()
    }
}

#[derive(Debug)]
pub struct OffloadSession<S: tls::Session> {
    // Inner is none while remote thread has the session
    inner: Option<S>,
    is_poll_done: Option<Result<(), crate::transport::Error>>,
    pending_requests: UnboundedReceiver<Request<S>>,
    waker: Waker,
}

impl<S: tls::Session> OffloadSession<S> {
    fn new(
        inner: S,
        new_session: &mut UnboundedSender<(S, UnboundedSender<Request<S>>)>,
        remote_thread: Waker,
    ) -> Self {
        // Channel to pass requests from remote TLS thread to main thread
        let (tx, rx) = futures_channel::mpsc::unbounded::<Request<S>>();

        // Send the session to the TLS thread. It will pass it back when the handshake has finished.
        let _ = new_session.unbounded_send((inner, tx));

        Self {
            pending_requests: rx,
            waker: remote_thread,
            is_poll_done: None,
            inner: None,
        }
    }
}

impl<S: tls::Session> tls::Session for OffloadSession<S> {
    #[inline]
    fn poll<W>(&mut self, context: &mut W) -> Poll<Result<(), transport::Error>>
    where
        W: tls::Context<Self>,
    {
        if let Some(finished) = self.is_poll_done {
            return Poll::Ready(finished);
        }
        // This will wake up the TLS remote thread
        self.waker.wake_by_ref();

        loop {
            let mut cx = Context::from_waker(context.waker());

            let req = match Pin::new(&mut self.pending_requests).poll_next(&mut cx) {
                Poll::Ready(Some(request)) => request,
                Poll::Ready(None) => {
                    return Poll::Ready(Err(crate::transport::Error::INTERNAL_ERROR
                        .with_reason("offloaded crypto session finished without sending Done")))
                }
                Poll::Pending => break,
            };

            match req {
                Request::HandshakeKeys(key, header_key) => {
                    context.on_handshake_keys(key, header_key)?;
                }
                Request::ZeroRttKeys(key, header_key, transport_parameters) => {
                    context.on_zero_rtt_keys(
                        key,
                        header_key,
                        tls::ApplicationParameters {
                            transport_parameters: &transport_parameters,
                        },
                    )?;
                }
                Request::ClientParams(client_params, mut server_params) => context
                    .on_client_application_params(
                        tls::ApplicationParameters {
                            transport_parameters: &client_params,
                        },
                        &mut server_params,
                    )?,
                Request::OneRttKeys(key, header_key, transport_parameters) => {
                    context.on_one_rtt_keys(
                        key,
                        header_key,
                        tls::ApplicationParameters {
                            transport_parameters: &transport_parameters,
                        },
                    )?;
                }
                Request::Done(session, res) => {
                    self.inner = Some(session);
                    self.is_poll_done = Some(res);

                    return Poll::Ready(res);
                }
                Request::ServerName(server_name) => {
                    context.on_server_name(server_name)?;
                }
                Request::ApplicationProtocol(application_protocol) => {
                    context.on_application_protocol(application_protocol)?;
                }
                Request::HandshakeComplete => {
                    context.on_handshake_complete()?;
                }
                Request::CanSendInitial(sender) => {
                    let _ = sender.send(context.can_send_initial());
                }
                Request::ReceiveInitial(max_len, sender) => {
                    let resp = context.receive_initial(max_len);
                    let _ = sender.send(resp);
                }
                Request::ReceiveApplication(max_len, sender) => {
                    let resp = context.receive_application(max_len);
                    let _ = sender.send(resp);
                }
                Request::ReceiveHandshake(max_len, sender) => {
                    let resp = context.receive_handshake(max_len);
                    if let Some(_) = resp {
                        // We need to wake up the s2n-quic endpoint after providing
                        // handshake packets to the TLS provider as there may now be
                        // handshake data that needs to be sent in response.
                        context.waker().wake_by_ref();
                    }
                    let _ = sender.send(resp);
                }
                Request::CanSendHandshake(sender) => {
                    let _ = sender.send(context.can_send_handshake());
                }
                Request::CanSendApplication(sender) => {
                    let _ = sender.send(context.can_send_application());
                }
                Request::SendApplication(bytes) => {
                    context.send_application(bytes);
                }
                Request::SendHandshake(bytes) => {
                    context.send_handshake(bytes);
                }
                Request::SendInitial(bytes) => context.send_initial(bytes),
                Request::KeyExchangeGroup(named_group) => {
                    context.on_key_exchange_group(named_group)?;
                }
                Request::TlsContext(ctx) => context.on_tls_context(ctx),
            }
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

struct AsyncRequest<T> {
    rx: Option<Receiver<T>>,
}

impl<T> AsyncRequest<T> {
    fn empty() -> Self {
        AsyncRequest { rx: None }
    }

    fn poll_request(
        &mut self,
        cx: &mut core::task::Context<'_>,
        issue: impl FnOnce(Sender<T>),
    ) -> Poll<T> {
        loop {
            if let Some(mut receiver) = self.rx.as_mut() {
                match Pin::new(&mut receiver).poll(cx) {
                    Poll::Ready(Ok(value)) => {
                        receiver.close();
                        self.rx = None;
                        return Poll::Ready(value);
                    }
                    Poll::Ready(Err(_)) => {
                        // treat cancellation as reason to ask again.
                        // FIXME: this probably means that the parent thread is no longer interested
                        // in this connection and we should instead tear it down.
                        receiver.close();
                        self.rx = None;
                        // loop around to next loop iteration
                    }
                    Poll::Pending => return Poll::Pending,
                }
            } else {
                let (tx, rx) = futures_channel::oneshot::channel();
                self.rx = Some(rx);
                issue(tx);
                return Poll::Pending;
            }
        }
    }
}

/// Context used on the remote thread. This must delegate all methods via a channel to the calling
/// thread, using `Request` to send parameters (and optionally receive results).
struct RemoteContext<S: CryptoSuite> {
    tx: UnboundedSender<Request<S>>,
    waker: Waker,

    receive_initial: AsyncRequest<Option<bytes::Bytes>>,
    receive_handshake: AsyncRequest<Option<bytes::Bytes>>,
    receive_application: AsyncRequest<Option<bytes::Bytes>>,

    can_send_initial: AsyncRequest<bool>,
    can_send_handshake: AsyncRequest<bool>,
    can_send_application: AsyncRequest<bool>,
}

impl<S: CryptoSuite> tls::Context<S> for RemoteContext<S> {
    fn on_client_application_params(
        &mut self,
        client_params: tls::ApplicationParameters,
        server_params: &mut alloc::vec::Vec<u8>,
    ) -> Result<(), crate::transport::Error> {
        let _ = self.tx.unbounded_send(Request::ClientParams(
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
            .tx
            .unbounded_send(Request::HandshakeKeys(key, header_key));
        Ok(())
    }

    fn on_zero_rtt_keys(
        &mut self,
        key: <S as CryptoSuite>::ZeroRttKey,
        header_key: <S as CryptoSuite>::ZeroRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), crate::transport::Error> {
        let _ = self.tx.unbounded_send(Request::ZeroRttKeys(
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
        let _ = self.tx.unbounded_send(Request::OneRttKeys(
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
        let _ = self.tx.unbounded_send(Request::ServerName(server_name));
        Ok(())
    }

    fn on_application_protocol(
        &mut self,
        application_protocol: bytes::Bytes,
    ) -> Result<(), crate::transport::Error> {
        let _ = self
            .tx
            .unbounded_send(Request::ApplicationProtocol(application_protocol));
        Ok(())
    }

    fn on_key_exchange_group(
        &mut self,
        named_group: tls::NamedGroup,
    ) -> Result<(), crate::transport::Error> {
        let _ = self
            .tx
            .unbounded_send(Request::KeyExchangeGroup(named_group));
        Ok(())
    }

    fn on_handshake_complete(&mut self) -> Result<(), crate::transport::Error> {
        let _ = self.tx.unbounded_send(Request::HandshakeComplete);
        Ok(())
    }

    fn on_tls_context(&mut self, context: alloc::boxed::Box<dyn Any + Send>) {
        let _ = self.tx.unbounded_send(Request::TlsContext(context));
    }

    fn on_tls_exporter_ready(
        &mut self,
        _session: &impl tls::TlsSession,
    ) -> Result<(), crate::transport::Error> {
        // FIXME: needs some form of async callback, or maybe never gets called during remote phase?
        Ok(())
    }

    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(resp) = self.receive_initial.poll_request(&mut cx, |tx| {
            let _ = self.tx.unbounded_send(Request::ReceiveInitial(max_len, tx));
        }) {
            resp
        } else {
            None
        }
    }

    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(resp) = self.receive_handshake.poll_request(&mut cx, |tx| {
            let _ = self
                .tx
                .unbounded_send(Request::ReceiveHandshake(max_len, tx));
        }) {
            resp
        } else {
            None
        }
    }

    fn receive_application(&mut self, max_len: Option<usize>) -> Option<bytes::Bytes> {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(resp) = self.receive_application.poll_request(&mut cx, |tx| {
            let _ = self
                .tx
                .unbounded_send(Request::ReceiveApplication(max_len, tx));
        }) {
            resp
        } else {
            None
        }
    }

    fn can_send_initial(&mut self) -> bool {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(resp) = self.can_send_initial.poll_request(&mut cx, |tx| {
            let _ = self.tx.unbounded_send(Request::CanSendInitial(tx));
        }) {
            resp
        } else {
            // FIXME: either async-ify, remove, or figure out what the Pending value should be.
            false
        }
    }

    fn send_initial(&mut self, transmission: bytes::Bytes) {
        let _ = self.tx.unbounded_send(Request::SendInitial(transmission));
    }

    fn can_send_handshake(&mut self) -> bool {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(resp) = self.can_send_handshake.poll_request(&mut cx, |tx| {
            let _ = self.tx.unbounded_send(Request::CanSendHandshake(tx));
        }) {
            resp
        } else {
            // FIXME: either async-ify, remove, or figure out what the Pending value should be.
            false
        }
    }

    fn send_handshake(&mut self, transmission: bytes::Bytes) {
        let _ = self.tx.unbounded_send(Request::SendHandshake(transmission));
    }

    fn can_send_application(&mut self) -> bool {
        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(resp) = self.can_send_application.poll_request(&mut cx, |tx| {
            let _ = self.tx.unbounded_send(Request::CanSendApplication(tx));
        }) {
            resp
        } else {
            // FIXME: either async-ify, remove, or figure out what the Pending value should be.
            false
        }
    }

    fn send_application(&mut self, transmission: bytes::Bytes) {
        let _ = self
            .tx
            .unbounded_send(Request::SendApplication(transmission));
    }

    fn waker(&self) -> &core::task::Waker {
        &self.waker
    }
}

enum Request<S: CryptoSuite> {
    ClientParams(Vec<u8>, Vec<u8>),
    HandshakeKeys(
        <S as CryptoSuite>::HandshakeKey,
        <S as CryptoSuite>::HandshakeHeaderKey,
    ),
    ZeroRttKeys(
        <S as CryptoSuite>::ZeroRttKey,
        <S as CryptoSuite>::ZeroRttHeaderKey,
        Vec<u8>,
    ),
    OneRttKeys(
        <S as CryptoSuite>::OneRttKey,
        <S as CryptoSuite>::OneRttHeaderKey,
        Vec<u8>,
    ),
    TlsContext(alloc::boxed::Box<dyn Any + Send>),
    ServerName(crate::application::ServerName),
    ApplicationProtocol(bytes::Bytes),
    KeyExchangeGroup(NamedGroup),
    HandshakeComplete,

    ReceiveInitial(Option<usize>, Sender<Option<bytes::Bytes>>),
    ReceiveApplication(Option<usize>, Sender<Option<bytes::Bytes>>),
    ReceiveHandshake(Option<usize>, Sender<Option<bytes::Bytes>>),
    CanSendInitial(Sender<bool>),
    CanSendHandshake(Sender<bool>),
    CanSendApplication(Sender<bool>),
    SendApplication(bytes::Bytes),
    SendHandshake(bytes::Bytes),
    SendInitial(bytes::Bytes),
    Done(S, Result<(), crate::transport::Error>),
}
