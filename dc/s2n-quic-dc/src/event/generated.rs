// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use super::*;
pub mod api {
    #![doc = r" This module contains events that are emitted to the [`Subscriber`](crate::event::Subscriber)"]
    use super::*;
    pub use s2n_quic_core::event::api::{
        ConnectionInfo, ConnectionMeta, EndpointMeta, EndpointType, SocketAddress, Subject,
    };
    pub use traits::Subscriber;
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ApplicationWrite {
        #[doc = " The number of bytes that the application tried to write"]
        pub len: usize,
    }
    impl Event for ApplicationWrite {
        const NAME: &'static str = "application:write";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ApplicationRead {
        #[doc = " The number of bytes that the application tried to read"]
        pub len: usize,
    }
    impl Event for ApplicationRead {
        const NAME: &'static str = "application:write";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointInitialized<'a> {
        pub acceptor_addr: SocketAddress<'a>,
        pub handshake_addr: SocketAddress<'a>,
        pub tcp: bool,
        pub udp: bool,
    }
    impl<'a> Event for EndpointInitialized<'a> {
        const NAME: &'static str = "endpoint:initialized";
    }
}
pub mod tracing {
    #![doc = r" This module contains event integration with [`tracing`](https://docs.rs/tracing)"]
    use super::api;
    #[doc = r" Emits events with [`tracing`](https://docs.rs/tracing)"]
    #[derive(Clone, Debug)]
    pub struct Subscriber {
        client: tracing::Span,
        server: tracing::Span,
    }
    impl Default for Subscriber {
        fn default() -> Self {
            let root = tracing :: span ! (target : "s2n_quic_dc" , tracing :: Level :: DEBUG , "s2n_quic_dc");
            let client =
                tracing :: span ! (parent : root . id () , tracing :: Level :: DEBUG , "client");
            let server =
                tracing :: span ! (parent : root . id () , tracing :: Level :: DEBUG , "server");
            Self { client, server }
        }
    }
    impl super::Subscriber for Subscriber {
        type ConnectionContext = tracing::Span;
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            _info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            let parent = match meta.endpoint_type {
                api::EndpointType::Client { .. } => self.client.id(),
                api::EndpointType::Server { .. } => self.server.id(),
            };
            tracing :: span ! (target : "s2n_quic_dc" , parent : parent , tracing :: Level :: DEBUG , "conn" , id = meta . id)
        }
        #[inline]
        fn on_application_write(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            let id = context.id();
            let api::ApplicationWrite { len } = event;
            tracing :: event ! (target : "application_write" , parent : id , tracing :: Level :: DEBUG , len = tracing :: field :: debug (len));
        }
        #[inline]
        fn on_application_read(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            let id = context.id();
            let api::ApplicationRead { len } = event;
            tracing :: event ! (target : "application_read" , parent : id , tracing :: Level :: DEBUG , len = tracing :: field :: debug (len));
        }
        #[inline]
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            let parent = match meta.endpoint_type {
                api::EndpointType::Client { .. } => self.client.id(),
                api::EndpointType::Server { .. } => self.server.id(),
            };
            let api::EndpointInitialized {
                acceptor_addr,
                handshake_addr,
                tcp,
                udp,
            } = event;
            tracing :: event ! (target : "endpoint_initialized" , parent : parent , tracing :: Level :: DEBUG , acceptor_addr = tracing :: field :: debug (acceptor_addr) , handshake_addr = tracing :: field :: debug (handshake_addr) , tcp = tracing :: field :: debug (tcp) , udp = tracing :: field :: debug (udp));
        }
    }
}
pub mod builder {
    use super::*;
    pub use s2n_quic_core::event::builder::{
        ConnectionInfo, ConnectionMeta, EndpointMeta, EndpointType, SocketAddress, Subject,
    };
    #[derive(Clone, Debug)]
    pub struct ApplicationWrite {
        #[doc = " The number of bytes that the application tried to write"]
        pub len: usize,
    }
    impl IntoEvent<api::ApplicationWrite> for ApplicationWrite {
        #[inline]
        fn into_event(self) -> api::ApplicationWrite {
            let ApplicationWrite { len } = self;
            api::ApplicationWrite {
                len: len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ApplicationRead {
        #[doc = " The number of bytes that the application tried to read"]
        pub len: usize,
    }
    impl IntoEvent<api::ApplicationRead> for ApplicationRead {
        #[inline]
        fn into_event(self) -> api::ApplicationRead {
            let ApplicationRead { len } = self;
            api::ApplicationRead {
                len: len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointInitialized<'a> {
        pub acceptor_addr: SocketAddress<'a>,
        pub handshake_addr: SocketAddress<'a>,
        pub tcp: bool,
        pub udp: bool,
    }
    impl<'a> IntoEvent<api::EndpointInitialized<'a>> for EndpointInitialized<'a> {
        #[inline]
        fn into_event(self) -> api::EndpointInitialized<'a> {
            let EndpointInitialized {
                acceptor_addr,
                handshake_addr,
                tcp,
                udp,
            } = self;
            api::EndpointInitialized {
                acceptor_addr: acceptor_addr.into_event(),
                handshake_addr: handshake_addr.into_event(),
                tcp: tcp.into_event(),
                udp: udp.into_event(),
            }
        }
    }
}
pub use traits::*;
mod traits {
    use super::*;
    use core::fmt;
    use s2n_quic_core::{event::Meta, query};
    #[doc = r" Allows for events to be subscribed to"]
    pub trait Subscriber: 'static + Send {
        #[doc = r" An application provided type associated with each connection."]
        #[doc = r""]
        #[doc = r" The context provides a mechanism for applications to provide a custom type"]
        #[doc = r" and update it on each event, e.g. computing statistics. Each event"]
        #[doc = r" invocation (e.g. [`Subscriber::on_packet_sent`]) also provides mutable"]
        #[doc = r" access to the context `&mut ConnectionContext` and allows for updating the"]
        #[doc = r" context."]
        #[doc = r""]
        #[doc = r" ```no_run"]
        #[doc = r" # mod s2n_quic { pub mod provider { pub mod event {"]
        #[doc = r" #     pub use s2n_quic_core::event::{api as events, api::ConnectionInfo, api::ConnectionMeta, Subscriber};"]
        #[doc = r" # }}}"]
        #[doc = r" use s2n_quic::provider::event::{"]
        #[doc = r"     ConnectionInfo, ConnectionMeta, Subscriber, events::PacketSent"]
        #[doc = r" };"]
        #[doc = r""]
        #[doc = r" pub struct MyEventSubscriber;"]
        #[doc = r""]
        #[doc = r" pub struct MyEventContext {"]
        #[doc = r"     packet_sent: u64,"]
        #[doc = r" }"]
        #[doc = r""]
        #[doc = r" impl Subscriber for MyEventSubscriber {"]
        #[doc = r"     type ConnectionContext = MyEventContext;"]
        #[doc = r""]
        #[doc = r"     fn create_connection_context("]
        #[doc = r"         &mut self, _meta: &ConnectionMeta,"]
        #[doc = r"         _info: &ConnectionInfo,"]
        #[doc = r"     ) -> Self::ConnectionContext {"]
        #[doc = r"         MyEventContext { packet_sent: 0 }"]
        #[doc = r"     }"]
        #[doc = r""]
        #[doc = r"     fn on_packet_sent("]
        #[doc = r"         &mut self,"]
        #[doc = r"         context: &mut Self::ConnectionContext,"]
        #[doc = r"         _meta: &ConnectionMeta,"]
        #[doc = r"         _event: &PacketSent,"]
        #[doc = r"     ) {"]
        #[doc = r"         context.packet_sent += 1;"]
        #[doc = r"     }"]
        #[doc = r" }"]
        #[doc = r"  ```"]
        type ConnectionContext: 'static + Send;
        #[doc = r" Creates a context to be passed to each connection-related event"]
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext;
        #[doc = "Called when the `ApplicationWrite` event is triggered"]
        #[inline]
        fn on_application_write(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ApplicationRead` event is triggered"]
        #[inline]
        fn on_application_read(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointInitialized` event is triggered"]
        #[inline]
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Called for each event that relates to the endpoint and all connections"]
        #[inline]
        fn on_event<M: Meta, E: Event>(&self, meta: &M, event: &E) {
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Called for each event that relates to a connection"]
        #[inline]
        fn on_connection_event<E: Event>(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &E,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Used for querying the `Subscriber::ConnectionContext` on a Subscriber"]
        #[inline]
        fn query(
            context: &Self::ConnectionContext,
            query: &mut dyn query::Query,
        ) -> query::ControlFlow {
            query.execute(context)
        }
    }
    #[doc = r" Subscriber is implemented for a 2-element tuple to make it easy to compose multiple"]
    #[doc = r" subscribers."]
    impl<A, B> Subscriber for (A, B)
    where
        A: Subscriber,
        B: Subscriber,
    {
        type ConnectionContext = (A::ConnectionContext, B::ConnectionContext);
        #[inline]
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            (
                self.0.create_connection_context(meta, info),
                self.1.create_connection_context(meta, info),
            )
        }
        #[inline]
        fn on_application_write(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            (self.0).on_application_write(&context.0, meta, event);
            (self.1).on_application_write(&context.1, meta, event);
        }
        #[inline]
        fn on_application_read(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            (self.0).on_application_read(&context.0, meta, event);
            (self.1).on_application_read(&context.1, meta, event);
        }
        #[inline]
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            (self.0).on_endpoint_initialized(meta, event);
            (self.1).on_endpoint_initialized(meta, event);
        }
        #[inline]
        fn on_event<M: Meta, E: Event>(&self, meta: &M, event: &E) {
            self.0.on_event(meta, event);
            self.1.on_event(meta, event);
        }
        #[inline]
        fn on_connection_event<E: Event>(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &E,
        ) {
            self.0.on_connection_event(&context.0, meta, event);
            self.1.on_connection_event(&context.1, meta, event);
        }
        #[inline]
        fn query(
            context: &Self::ConnectionContext,
            query: &mut dyn query::Query,
        ) -> query::ControlFlow {
            query
                .execute(context)
                .and_then(|| A::query(&context.0, query))
                .and_then(|| B::query(&context.1, query))
        }
    }
    pub trait EndpointPublisher {
        #[doc = "Publishes a `EndpointInitialized` event to the publisher's subscriber"]
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized);
        #[doc = r" Returns the QUIC version, if any"]
        fn quic_version(&self) -> Option<u32>;
    }
    pub struct EndpointPublisherSubscriber<'a, Sub: Subscriber> {
        meta: api::EndpointMeta,
        quic_version: Option<u32>,
        subscriber: &'a Sub,
    }
    impl<'a, Sub: Subscriber> fmt::Debug for EndpointPublisherSubscriber<'a, Sub> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("ConnectionPublisherSubscriber")
                .field("meta", &self.meta)
                .field("quic_version", &self.quic_version)
                .finish()
        }
    }
    impl<'a, Sub: Subscriber> EndpointPublisherSubscriber<'a, Sub> {
        #[inline]
        pub fn new(
            meta: builder::EndpointMeta,
            quic_version: Option<u32>,
            subscriber: &'a Sub,
        ) -> Self {
            Self {
                meta: meta.into_event(),
                quic_version,
                subscriber,
            }
        }
    }
    impl<'a, Sub: Subscriber> EndpointPublisher for EndpointPublisherSubscriber<'a, Sub> {
        #[inline]
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized) {
            let event = event.into_event();
            self.subscriber.on_endpoint_initialized(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> Option<u32> {
            self.quic_version
        }
    }
    pub trait ConnectionPublisher {
        #[doc = "Publishes a `ApplicationWrite` event to the publisher's subscriber"]
        fn on_application_write(&self, event: builder::ApplicationWrite);
        #[doc = "Publishes a `ApplicationRead` event to the publisher's subscriber"]
        fn on_application_read(&self, event: builder::ApplicationRead);
        #[doc = r" Returns the QUIC version negotiated for the current connection, if any"]
        fn quic_version(&self) -> u32;
        #[doc = r" Returns the [`Subject`] for the current publisher"]
        fn subject(&self) -> api::Subject;
    }
    pub struct ConnectionPublisherSubscriber<'a, Sub: Subscriber> {
        meta: api::ConnectionMeta,
        quic_version: u32,
        subscriber: &'a Sub,
        context: &'a Sub::ConnectionContext,
    }
    impl<'a, Sub: Subscriber> fmt::Debug for ConnectionPublisherSubscriber<'a, Sub> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("ConnectionPublisherSubscriber")
                .field("meta", &self.meta)
                .field("quic_version", &self.quic_version)
                .finish()
        }
    }
    impl<'a, Sub: Subscriber> ConnectionPublisherSubscriber<'a, Sub> {
        #[inline]
        pub fn new(
            meta: builder::ConnectionMeta,
            quic_version: u32,
            subscriber: &'a Sub,
            context: &'a Sub::ConnectionContext,
        ) -> Self {
            Self {
                meta: meta.into_event(),
                quic_version,
                subscriber,
                context,
            }
        }
    }
    impl<'a, Sub: Subscriber> ConnectionPublisher for ConnectionPublisherSubscriber<'a, Sub> {
        #[inline]
        fn on_application_write(&self, event: builder::ApplicationWrite) {
            let event = event.into_event();
            self.subscriber
                .on_application_write(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_application_read(&self, event: builder::ApplicationRead) {
            let event = event.into_event();
            self.subscriber
                .on_application_read(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> u32 {
            self.quic_version
        }
        #[inline]
        fn subject(&self) -> api::Subject {
            self.meta.subject()
        }
    }
}
pub mod metrics {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};
    use s2n_quic_core::event::metrics::Recorder;
    #[derive(Debug)]
    pub struct Subscriber<S: super::Subscriber>
    where
        S::ConnectionContext: Recorder,
    {
        subscriber: S,
    }
    impl<S: super::Subscriber> Subscriber<S>
    where
        S::ConnectionContext: Recorder,
    {
        pub fn new(subscriber: S) -> Self {
            Self { subscriber }
        }
    }
    pub struct Context<R: Recorder> {
        recorder: R,
        application_write: AtomicU32,
        application_read: AtomicU32,
    }
    impl<S: super::Subscriber> super::Subscriber for Subscriber<S>
    where
        S::ConnectionContext: Recorder,
    {
        type ConnectionContext = Context<S::ConnectionContext>;
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            Context {
                recorder: self.subscriber.create_connection_context(meta, info),
                application_write: AtomicU32::new(0),
                application_read: AtomicU32::new(0),
            }
        }
        #[inline]
        fn on_application_write(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            context.application_write.fetch_add(1, Ordering::Relaxed);
            self.subscriber
                .on_application_write(&context.recorder, meta, event);
        }
        #[inline]
        fn on_application_read(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            context.application_read.fetch_add(1, Ordering::Relaxed);
            self.subscriber
                .on_application_read(&context.recorder, meta, event);
        }
    }
    impl<R: Recorder> Drop for Context<R> {
        fn drop(&mut self) {
            self.recorder.increment_counter(
                "application_write",
                self.application_write.load(Ordering::Relaxed) as _,
            );
            self.recorder.increment_counter(
                "application_read",
                self.application_read.load(Ordering::Relaxed) as _,
            );
        }
    }
}
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::event::snapshot::Location;
    use core::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
    pub mod endpoint {
        use super::*;
        pub struct Subscriber {
            location: Option<Location>,
            output: Mutex<Vec<String>>,
            pub endpoint_initialized: AtomicU32,
        }
        impl Drop for Subscriber {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    return;
                }
                if let Some(location) = self.location.as_ref() {
                    location.snapshot_log(&self.output.lock().unwrap());
                }
            }
        }
        impl Subscriber {
            #[doc = r" Creates a subscriber with snapshot assertions enabled"]
            #[track_caller]
            pub fn snapshot() -> Self {
                let mut sub = Self::no_snapshot();
                sub.location = Location::from_thread_name();
                sub
            }
            #[doc = r" Creates a subscriber with snapshot assertions enabled"]
            #[track_caller]
            pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
                let mut sub = Self::no_snapshot();
                sub.location = Some(Location::new(name));
                sub
            }
            #[doc = r" Creates a subscriber with snapshot assertions disabled"]
            pub fn no_snapshot() -> Self {
                Self {
                    location: None,
                    output: Default::default(),
                    endpoint_initialized: AtomicU32::new(0),
                }
            }
        }
        impl super::super::Subscriber for Subscriber {
            type ConnectionContext = ();
            fn create_connection_context(
                &self,
                _meta: &api::ConnectionMeta,
                _info: &api::ConnectionInfo,
            ) -> Self::ConnectionContext {
            }
            fn on_endpoint_initialized(
                &self,
                meta: &api::EndpointMeta,
                event: &api::EndpointInitialized,
            ) {
                self.endpoint_initialized.fetch_add(1, Ordering::Relaxed);
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
            }
        }
    }
    #[derive(Debug)]
    pub struct Subscriber {
        location: Option<Location>,
        output: Mutex<Vec<String>>,
        pub application_write: AtomicU32,
        pub application_read: AtomicU32,
        pub endpoint_initialized: AtomicU32,
    }
    impl Drop for Subscriber {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            if let Some(location) = self.location.as_ref() {
                location.snapshot_log(&self.output.lock().unwrap());
            }
        }
    }
    impl Subscriber {
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn snapshot() -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Location::from_thread_name();
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Some(Location::new(name));
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions disabled"]
        pub fn no_snapshot() -> Self {
            Self {
                location: None,
                output: Default::default(),
                application_write: AtomicU32::new(0),
                application_read: AtomicU32::new(0),
                endpoint_initialized: AtomicU32::new(0),
            }
        }
    }
    impl super::Subscriber for Subscriber {
        type ConnectionContext = ();
        fn create_connection_context(
            &self,
            _meta: &api::ConnectionMeta,
            _info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
        }
        fn on_application_write(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            self.application_write.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
            }
        }
        fn on_application_read(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            self.application_read.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
            }
        }
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            self.endpoint_initialized.fetch_add(1, Ordering::Relaxed);
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
        }
    }
    #[derive(Debug)]
    pub struct Publisher {
        location: Option<Location>,
        output: Mutex<Vec<String>>,
        pub application_write: AtomicU32,
        pub application_read: AtomicU32,
        pub endpoint_initialized: AtomicU32,
    }
    impl Publisher {
        #[doc = r" Creates a publisher with snapshot assertions enabled"]
        #[track_caller]
        pub fn snapshot() -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Location::from_thread_name();
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Some(Location::new(name));
            sub
        }
        #[doc = r" Creates a publisher with snapshot assertions disabled"]
        pub fn no_snapshot() -> Self {
            Self {
                location: None,
                output: Default::default(),
                application_write: AtomicU32::new(0),
                application_read: AtomicU32::new(0),
                endpoint_initialized: AtomicU32::new(0),
            }
        }
    }
    impl super::EndpointPublisher for Publisher {
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized) {
            self.endpoint_initialized.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn quic_version(&self) -> Option<u32> {
            Some(1)
        }
    }
    impl super::ConnectionPublisher for Publisher {
        fn on_application_write(&self, event: builder::ApplicationWrite) {
            self.application_write.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                self.output.lock().unwrap().push(format!("{event:?}"));
            }
        }
        fn on_application_read(&self, event: builder::ApplicationRead) {
            self.application_read.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                self.output.lock().unwrap().push(format!("{event:?}"));
            }
        }
        fn quic_version(&self) -> u32 {
            1
        }
        fn subject(&self) -> api::Subject {
            builder::Subject::Connection { id: 0 }.into_event()
        }
    }
    impl Drop for Publisher {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            if let Some(location) = self.location.as_ref() {
                location.snapshot_log(&self.output.lock().unwrap());
            }
        }
    }
}
