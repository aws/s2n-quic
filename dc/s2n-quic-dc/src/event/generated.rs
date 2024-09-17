// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use super::*;
pub mod api {
    #![doc = r" This module contains events that are emitted to the [`Subscriber`](crate::event::Subscriber)"]
    use super::*;
    pub use traits::Subscriber;
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Frame was sent"]
    pub struct FrameSent {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub frame: Frame,
    }
    impl Event for FrameSent {
        const NAME: &'static str = "transport:frame_sent";
    }
}
#[cfg(feature = "event-tracing")]
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
            let root =
                tracing :: span ! (target : "s2n_quic" , tracing :: Level :: DEBUG , "s2n_quic");
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
                api::EndpointType::Client {} => self.client.id(),
                api::EndpointType::Server {} => self.server.id(),
            };
            tracing :: span ! (target : "s2n_quic" , parent : parent , tracing :: Level :: DEBUG , "conn" , id = meta . id)
        }
        #[inline]
        fn on_frame_sent(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::FrameSent,
        ) {
            let id = context.id();
            let api::FrameSent {
                packet_header,
                path_id,
                frame,
            } = event;
            tracing :: event ! (target : "frame_sent" , parent : id , tracing :: Level :: DEBUG , packet_header = tracing :: field :: debug (packet_header) , path_id = tracing :: field :: debug (path_id) , frame = tracing :: field :: debug (frame));
        }
    }
}
pub mod builder {
    use super::*;
    #[derive(Clone, Debug)]
    #[doc = " Frame was sent"]
    pub struct FrameSent {
        pub packet_header: PacketHeader,
        pub path_id: u64,
        pub frame: Frame,
    }
    impl IntoEvent<api::FrameSent> for FrameSent {
        #[inline]
        fn into_event(self) -> api::FrameSent {
            let FrameSent {
                packet_header,
                path_id,
                frame,
            } = self;
            api::FrameSent {
                packet_header: packet_header.into_event(),
                path_id: path_id.into_event(),
                frame: frame.into_event(),
            }
        }
    }
}
pub mod supervisor {
    #![doc = r" This module contains the `supervisor::Outcome` and `supervisor::Context` for use"]
    #![doc = r" when implementing [`Subscriber::supervisor_timeout`](crate::event::Subscriber::supervisor_timeout) and"]
    #![doc = r" [`Subscriber::on_supervisor_timeout`](crate::event::Subscriber::on_supervisor_timeout)"]
    #![doc = r" on a Subscriber."]
    use crate::{
        application,
        event::{builder::SocketAddress, IntoEvent},
    };
    #[non_exhaustive]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum Outcome {
        #[doc = r" Allow the connection to remain open"]
        Continue,
        #[doc = r" Close the connection and notify the peer"]
        Close { error_code: application::Error },
        #[doc = r" Close the connection without notifying the peer"]
        ImmediateClose { reason: &'static str },
    }
    impl Default for Outcome {
        fn default() -> Self {
            Self::Continue
        }
    }
    #[non_exhaustive]
    #[derive(Debug)]
    pub struct Context<'a> {
        #[doc = r" Number of handshakes that have begun but not completed"]
        pub inflight_handshakes: usize,
        #[doc = r" Number of open connections"]
        pub connection_count: usize,
        #[doc = r" The address of the peer"]
        pub remote_address: SocketAddress<'a>,
        #[doc = r" True if the connection is in the handshake state, false otherwise"]
        pub is_handshaking: bool,
    }
    impl<'a> Context<'a> {
        pub fn new(
            inflight_handshakes: usize,
            connection_count: usize,
            remote_address: &'a crate::inet::SocketAddress,
            is_handshaking: bool,
        ) -> Self {
            Self {
                inflight_handshakes,
                connection_count,
                remote_address: remote_address.into_event(),
                is_handshaking,
            }
        }
    }
}
pub use traits::*;
mod traits {
    use super::*;
    use crate::query;
    use api::*;
    use core::fmt;
    #[doc = r" Provides metadata related to an event"]
    pub trait Meta: fmt::Debug {
        #[doc = r" Returns whether the local endpoint is a Client or Server"]
        fn endpoint_type(&self) -> &EndpointType;
        #[doc = r" A context from which the event is being emitted"]
        #[doc = r""]
        #[doc = r" An event can occur in the context of an Endpoint or Connection"]
        fn subject(&self) -> Subject;
        #[doc = r" The time the event occurred"]
        fn timestamp(&self) -> &crate::event::Timestamp;
    }
    impl Meta for ConnectionMeta {
        fn endpoint_type(&self) -> &EndpointType {
            &self.endpoint_type
        }
        fn subject(&self) -> Subject {
            Subject::Connection { id: self.id }
        }
        fn timestamp(&self) -> &crate::event::Timestamp {
            &self.timestamp
        }
    }
    impl Meta for EndpointMeta {
        fn endpoint_type(&self) -> &EndpointType {
            &self.endpoint_type
        }
        fn subject(&self) -> Subject {
            Subject::Endpoint {}
        }
        fn timestamp(&self) -> &crate::event::Timestamp {
            &self.timestamp
        }
    }
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
            meta: &ConnectionMeta,
            info: &ConnectionInfo,
        ) -> Self::ConnectionContext;
        #[doc = r" The period at which `on_supervisor_timeout` is called"]
        #[doc = r""]
        #[doc = r" If multiple `event::Subscriber`s are composed together, the minimum `supervisor_timeout`"]
        #[doc = r" across all `event::Subscriber`s will be used."]
        #[doc = r""]
        #[doc = r" If the `supervisor_timeout()` is `None` across all `event::Subscriber`s, connection supervision"]
        #[doc = r" will cease for the remaining lifetime of the connection and `on_supervisor_timeout` will no longer"]
        #[doc = r" be called."]
        #[doc = r""]
        #[doc = r" It is recommended to avoid setting this value less than ~100ms, as short durations"]
        #[doc = r" may lead to higher CPU utilization."]
        #[allow(unused_variables)]
        fn supervisor_timeout(
            &self,
            conn_context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            context: &supervisor::Context,
        ) -> Option<Duration> {
            None
        }
        #[doc = r" Called for each `supervisor_timeout` to determine any action to take on the connection based on the `supervisor::Outcome`"]
        #[doc = r""]
        #[doc = r" If multiple `event::Subscriber`s are composed together, the minimum `supervisor_timeout`"]
        #[doc = r" across all `event::Subscriber`s will be used, and thus `on_supervisor_timeout` may be called"]
        #[doc = r" earlier than the `supervisor_timeout` for a given `event::Subscriber` implementation."]
        #[allow(unused_variables)]
        fn on_supervisor_timeout(
            &self,
            conn_context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            context: &supervisor::Context,
        ) -> supervisor::Outcome {
            supervisor::Outcome::default()
        }
        #[doc = "Called when the `FrameSent` event is triggered"]
        #[inline]
        fn on_frame_sent(
            &self,
            context: &Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &FrameSent,
        ) {
            let _ = context;
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
            meta: &ConnectionMeta,
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
        #[doc = r" Used for querying and mutating the `Subscriber::ConnectionContext` on a Subscriber"]
        #[inline]
        fn query_mut(
            context: &mut Self::ConnectionContext,
            query: &mut dyn query::QueryMut,
        ) -> query::ControlFlow {
            query.execute_mut(context)
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
            meta: &ConnectionMeta,
            info: &ConnectionInfo,
        ) -> Self::ConnectionContext {
            (
                self.0.create_connection_context(meta, info),
                self.1.create_connection_context(meta, info),
            )
        }
        #[inline]
        fn supervisor_timeout(
            &self,
            conn_context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            context: &supervisor::Context,
        ) -> Option<Duration> {
            let timeout_a = self
                .0
                .supervisor_timeout(&mut conn_context.0, meta, context);
            let timeout_b = self
                .1
                .supervisor_timeout(&mut conn_context.1, meta, context);
            match (timeout_a, timeout_b) {
                (None, None) => None,
                (None, Some(timeout)) | (Some(timeout), None) => Some(timeout),
                (Some(a), Some(b)) => Some(a.min(b)),
            }
        }
        #[inline]
        fn on_supervisor_timeout(
            &self,
            conn_context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            context: &supervisor::Context,
        ) -> supervisor::Outcome {
            let outcome_a = self
                .0
                .on_supervisor_timeout(&mut conn_context.0, meta, context);
            let outcome_b = self
                .1
                .on_supervisor_timeout(&mut conn_context.1, meta, context);
            match (outcome_a, outcome_b) {
                (supervisor::Outcome::ImmediateClose { reason }, _)
                | (_, supervisor::Outcome::ImmediateClose { reason }) => {
                    supervisor::Outcome::ImmediateClose { reason }
                }
                (supervisor::Outcome::Close { error_code }, _)
                | (_, supervisor::Outcome::Close { error_code }) => {
                    supervisor::Outcome::Close { error_code }
                }
                _ => supervisor::Outcome::Continue,
            }
        }
        #[inline]
        fn on_frame_sent(
            &self,
            context: &Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &FrameSent,
        ) {
            (self.0).on_frame_sent(&context.0, meta, event);
            (self.1).on_frame_sent(&context.1, meta, event);
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
            meta: &ConnectionMeta,
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
        #[inline]
        fn query_mut(
            context: &mut Self::ConnectionContext,
            query: &mut dyn query::QueryMut,
        ) -> query::ControlFlow {
            query
                .execute_mut(context)
                .and_then(|| A::query_mut(&mut context.0, query))
                .and_then(|| B::query_mut(&mut context.1, query))
        }
    }
    pub trait EndpointPublisher {
        #[doc = r" Returns the QUIC version, if any"]
        fn quic_version(&self) -> Option<u32>;
    }
    pub struct EndpointPublisherSubscriber<'a, Sub: Subscriber> {
        meta: EndpointMeta,
        quic_version: Option<u32>,
        subscriber: &'a mut Sub,
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
            subscriber: &'a mut Sub,
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
        fn quic_version(&self) -> Option<u32> {
            self.quic_version
        }
    }
    pub trait ConnectionPublisher {
        #[doc = "Publishes a `FrameSent` event to the publisher's subscriber"]
        fn on_frame_sent(&self, event: builder::FrameSent);
        #[doc = r" Returns the QUIC version negotiated for the current connection, if any"]
        fn quic_version(&self) -> u32;
        #[doc = r" Returns the [`Subject`] for the current publisher"]
        fn subject(&self) -> Subject;
    }
    pub struct ConnectionPublisherSubscriber<'a, Sub: Subscriber> {
        meta: ConnectionMeta,
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
        fn on_frame_sent(&self, event: builder::FrameSent) {
            let event = event.into_event();
            self.subscriber
                .on_frame_sent(self.context, &self.meta, &event);
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
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Debug)]
    pub struct Subscriber {
        location: Option<Location>,
        output: Arc<Mutex<Vec<String>>>,
        pub frame_sent: Arc<AtomicU32>,
    }
    impl Drop for Subscriber {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            if let Some(location) = self.location.as_ref() {
                location.snapshot(&self.output.lock().unwrap());
            }
        }
    }
    impl Subscriber {
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn snapshot() -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Location::try_new();
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions disabled"]
        pub fn no_snapshot() -> Self {
            Self {
                location: None,
                output: Default::default(),
                frame_sent: Arc::new(AtomicU32::new(0)),
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
        fn on_frame_sent(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::FrameSent,
        ) {
            self.frame_sent.fetch_add(1, Ordering::SeqCst);
            if self.location.is_some() {
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Publisher {
        location: Option<Location>,
        output: Arc<Mutex<Vec<String>>>,
        pub frame_sent: Arc<AtomicU32>,
    }
    impl Publisher {
        #[doc = r" Creates a publisher with snapshot assertions enabled"]
        #[track_caller]
        pub fn snapshot() -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Location::try_new();
            sub
        }
        #[doc = r" Creates a publisher with snapshot assertions disabled"]
        pub fn no_snapshot() -> Self {
            Self {
                location: None,
                output: Default::default(),
                frame_sent: Arc::new(AtomicU32::new(0)),
            }
        }
    }
    impl super::EndpointPublisher for Publisher {
        fn quic_version(&self) -> Option<u32> {
            Some(1)
        }
    }
    impl super::ConnectionPublisher for Publisher {
        fn on_frame_sent(&self, event: builder::FrameSent) {
            self.frame_sent.fetch_add(1, Ordering::SeqCst);
            let event = event.into_event();
            if self.location.is_some() {
                self.output.lock().unwrap().push(format!("{event:?}"));
            }
        }
        fn quic_version(&self) -> u32 {
            1
        }
        fn subject(&self) -> api::Subject {
            api::Subject::Connection { id: 0 }
        }
    }
    impl Drop for Publisher {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            if let Some(location) = self.location.as_ref() {
                location.snapshot(&self.output.lock().unwrap());
            }
        }
    }
    #[derive(Clone, Debug)]
    struct Location(&'static core::panic::Location<'static>);
    impl Location {
        #[track_caller]
        fn try_new() -> Option<Self> {
            let thread = std::thread::current();
            if thread.name().map_or(false, |name| name != "main") {
                Some(Self(core::panic::Location::caller()))
            } else {
                None
            }
        }
        fn snapshot(&self, output: &[String]) {
            if cfg!(miri) {
                return;
            }
            use std::path::{Component, Path};
            let value = output.join("\n");
            let thread = std::thread::current();
            let function_name = thread.name().unwrap();
            let test_path = Path::new(self.0.file().trim_end_matches(".rs"));
            let module_path = test_path
                .components()
                .filter_map(|comp| match comp {
                    Component::Normal(comp) => comp.to_str(),
                    _ => Some("_"),
                })
                .chain(Some("events"))
                .collect::<Vec<_>>()
                .join("::");
            let current_dir = std::env::current_dir().unwrap();
            insta::_macro_support::assert_snapshot(
                insta::_macro_support::AutoName.into(),
                &value,
                current_dir.to_str().unwrap(),
                function_name,
                &module_path,
                self.0.file(),
                self.0.line(),
                "",
            )
            .unwrap()
        }
    }
}
