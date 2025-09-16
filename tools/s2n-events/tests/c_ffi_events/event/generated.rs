// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-events` crate and any required
// changes should be made there.

#![allow(clippy::needless_lifetimes)]
use super::*;
pub(crate) mod metrics;
pub mod api {
    #![doc = r" This module contains events that are emitted to the [`Subscriber`](crate::event::Subscriber)"]
    use super::*;
    #[allow(unused_imports)]
    use crate::event::metrics::aggregate;
    pub use traits::Subscriber;
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionMeta {
        pub id: u64,
        pub timestamp: Timestamp,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionMeta {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionMeta");
            fmt.field("id", &self.id);
            fmt.field("timestamp", &self.timestamp);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointMeta {}
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointMeta {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointMeta");
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionInfo {}
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionInfo {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionInfo");
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum Subject {
        #[non_exhaustive]
        Endpoint {},
        #[non_exhaustive]
        Connection { id: u64 },
    }
    impl aggregate::AsVariant for Subject {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ENDPOINT\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("CONNECTION\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Endpoint { .. } => 0usize,
                Self::Connection { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ByteArrayEvent<'a> {
        pub data: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ByteArrayEvent<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ByteArrayEvent");
            fmt.field("data", &self.data);
            fmt.finish()
        }
    }
    impl<'a> Event for ByteArrayEvent<'a> {
        const NAME: &'static str = "byte_array_event";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EnumEvent {
        pub value: TestEnum,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EnumEvent {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EnumEvent");
            fmt.field("value", &self.value);
            fmt.finish()
        }
    }
    impl Event for EnumEvent {
        const NAME: &'static str = "enum_event";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum TestEnum {
        #[non_exhaustive]
        TestValue1 {},
        #[non_exhaustive]
        TestValue2 {},
    }
    impl aggregate::AsVariant for TestEnum {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("TEST_VALUE1\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("TEST_VALUE2\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::TestValue1 { .. } => 0usize,
                Self::TestValue2 { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct CountEvent {
        pub count: u32,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for CountEvent {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("CountEvent");
            fmt.field("count", &self.count);
            fmt.finish()
        }
    }
    impl Event for CountEvent {
        const NAME: &'static str = "count_event";
    }
}
pub mod tracing {
    #![doc = r" This module contains event integration with [`tracing`](https://docs.rs/tracing)"]
    use super::api;
    #[doc = r" Emits events with [`tracing`](https://docs.rs/tracing)"]
    #[derive(Clone, Debug)]
    pub struct Subscriber {
        root: tracing::Span,
    }
    impl Default for Subscriber {
        fn default() -> Self {
            let root = tracing :: span ! (target : "c_ffi_events_test" , tracing :: Level :: DEBUG , "c_ffi_events_test");
            Self { root }
        }
    }
    impl Subscriber {
        fn parent<M: crate::event::Meta>(&self, _meta: &M) -> Option<tracing::Id> {
            self.root.id()
        }
    }
    impl super::Subscriber for Subscriber {
        type ConnectionContext = tracing::Span;
        fn create_connection_context(
            &mut self,
            meta: &api::ConnectionMeta,
            _info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            let parent = self.parent(meta);
            tracing :: span ! (target : "c_ffi_events" , parent : parent , tracing :: Level :: DEBUG , "conn" , id = meta . id)
        }
        #[inline]
        fn on_byte_array_event(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ByteArrayEvent,
        ) {
            let id = context.id();
            let api::ByteArrayEvent { data } = event;
            tracing :: event ! (target : "byte_array_event" , parent : id , tracing :: Level :: DEBUG , { data = tracing :: field :: debug (data) });
        }
        #[inline]
        fn on_enum_event(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::EnumEvent,
        ) {
            let id = context.id();
            let api::EnumEvent { value } = event;
            tracing :: event ! (target : "enum_event" , parent : id , tracing :: Level :: DEBUG , { value = tracing :: field :: debug (value) });
        }
        #[inline]
        fn on_count_event(&mut self, meta: &api::EndpointMeta, event: &api::CountEvent) {
            let parent = self.parent(meta);
            let api::CountEvent { count } = event;
            tracing :: event ! (target : "count_event" , parent : parent , tracing :: Level :: DEBUG , { count = tracing :: field :: debug (count) });
        }
    }
}
pub mod builder {
    use super::*;
    pub use s2n_quic_core::event::builder::SocketAddress;
    #[derive(Clone, Debug)]
    pub struct ConnectionMeta {
        pub id: u64,
        pub timestamp: Timestamp,
    }
    impl IntoEvent<api::ConnectionMeta> for ConnectionMeta {
        #[inline]
        fn into_event(self) -> api::ConnectionMeta {
            let ConnectionMeta { id, timestamp } = self;
            api::ConnectionMeta {
                id: id.into_event(),
                timestamp: timestamp.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointMeta {}
    impl IntoEvent<api::EndpointMeta> for EndpointMeta {
        #[inline]
        fn into_event(self) -> api::EndpointMeta {
            let EndpointMeta {} = self;
            api::EndpointMeta {}
        }
    }
    #[derive(Clone, Debug)]
    pub struct ConnectionInfo {}
    impl IntoEvent<api::ConnectionInfo> for ConnectionInfo {
        #[inline]
        fn into_event(self) -> api::ConnectionInfo {
            let ConnectionInfo {} = self;
            api::ConnectionInfo {}
        }
    }
    #[derive(Clone, Debug)]
    pub enum Subject {
        Endpoint,
        Connection { id: u64 },
    }
    impl IntoEvent<api::Subject> for Subject {
        #[inline]
        fn into_event(self) -> api::Subject {
            use api::Subject::*;
            match self {
                Self::Endpoint => Endpoint {},
                Self::Connection { id } => Connection {
                    id: id.into_event(),
                },
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ByteArrayEvent<'a> {
        pub data: &'a [u8],
    }
    impl<'a> IntoEvent<api::ByteArrayEvent<'a>> for ByteArrayEvent<'a> {
        #[inline]
        fn into_event(self) -> api::ByteArrayEvent<'a> {
            let ByteArrayEvent { data } = self;
            api::ByteArrayEvent {
                data: data.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EnumEvent {
        pub value: TestEnum,
    }
    impl IntoEvent<api::EnumEvent> for EnumEvent {
        #[inline]
        fn into_event(self) -> api::EnumEvent {
            let EnumEvent { value } = self;
            api::EnumEvent {
                value: value.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum TestEnum {
        TestValue1,
        TestValue2,
    }
    impl IntoEvent<api::TestEnum> for TestEnum {
        #[inline]
        fn into_event(self) -> api::TestEnum {
            use api::TestEnum::*;
            match self {
                Self::TestValue1 => TestValue1 {},
                Self::TestValue2 => TestValue2 {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct CountEvent {
        pub count: u32,
    }
    impl IntoEvent<api::CountEvent> for CountEvent {
        #[inline]
        fn into_event(self) -> api::CountEvent {
            let CountEvent { count } = self;
            api::CountEvent {
                count: count.into_event(),
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
    use crate::event::Meta;
    use core::fmt;
    use s2n_quic_core::query;
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
            &mut self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
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
            &mut self,
            conn_context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
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
            &mut self,
            conn_context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            context: &supervisor::Context,
        ) -> supervisor::Outcome {
            supervisor::Outcome::default()
        }
        #[doc = "Called when the `ByteArrayEvent` event is triggered"]
        #[inline]
        fn on_byte_array_event(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ByteArrayEvent,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EnumEvent` event is triggered"]
        #[inline]
        fn on_enum_event(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::EnumEvent,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `CountEvent` event is triggered"]
        #[inline]
        fn on_count_event(&mut self, meta: &api::EndpointMeta, event: &api::CountEvent) {
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Called for each event that relates to the endpoint and all connections"]
        #[inline]
        fn on_event<M: Meta, E: Event>(&mut self, meta: &M, event: &E) {
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Called for each event that relates to a connection"]
        #[inline]
        fn on_connection_event<E: Event>(
            &mut self,
            context: &mut Self::ConnectionContext,
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
            &mut self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            (
                self.0.create_connection_context(meta, info),
                self.1.create_connection_context(meta, info),
            )
        }
        #[inline]
        fn supervisor_timeout(
            &mut self,
            conn_context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
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
            &mut self,
            conn_context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
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
        fn on_byte_array_event(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ByteArrayEvent,
        ) {
            (self.0).on_byte_array_event(&mut context.0, meta, event);
            (self.1).on_byte_array_event(&mut context.1, meta, event);
        }
        #[inline]
        fn on_enum_event(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::EnumEvent,
        ) {
            (self.0).on_enum_event(&mut context.0, meta, event);
            (self.1).on_enum_event(&mut context.1, meta, event);
        }
        #[inline]
        fn on_count_event(&mut self, meta: &api::EndpointMeta, event: &api::CountEvent) {
            (self.0).on_count_event(meta, event);
            (self.1).on_count_event(meta, event);
        }
        #[inline]
        fn on_event<M: Meta, E: Event>(&mut self, meta: &M, event: &E) {
            self.0.on_event(meta, event);
            self.1.on_event(meta, event);
        }
        #[inline]
        fn on_connection_event<E: Event>(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &E,
        ) {
            self.0.on_connection_event(&mut context.0, meta, event);
            self.1.on_connection_event(&mut context.1, meta, event);
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
        #[doc = "Publishes a `CountEvent` event to the publisher's subscriber"]
        fn on_count_event(&mut self, event: builder::CountEvent);
        #[doc = r" Returns the QUIC version, if any"]
        fn quic_version(&self) -> Option<u32>;
    }
    pub struct EndpointPublisherSubscriber<'a, Sub: Subscriber> {
        meta: api::EndpointMeta,
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
        fn on_count_event(&mut self, event: builder::CountEvent) {
            let event = event.into_event();
            self.subscriber.on_count_event(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> Option<u32> {
            self.quic_version
        }
    }
    pub trait ConnectionPublisher {
        #[doc = "Publishes a `ByteArrayEvent` event to the publisher's subscriber"]
        fn on_byte_array_event(&mut self, event: builder::ByteArrayEvent);
        #[doc = "Publishes a `EnumEvent` event to the publisher's subscriber"]
        fn on_enum_event(&mut self, event: builder::EnumEvent);
        #[doc = r" Returns the QUIC version negotiated for the current connection, if any"]
        fn quic_version(&self) -> u32;
        #[doc = r" Returns the [`Subject`] for the current publisher"]
        fn subject(&self) -> api::Subject;
    }
    pub struct ConnectionPublisherSubscriber<'a, Sub: Subscriber> {
        meta: api::ConnectionMeta,
        quic_version: u32,
        subscriber: &'a mut Sub,
        context: &'a mut Sub::ConnectionContext,
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
            subscriber: &'a mut Sub,
            context: &'a mut Sub::ConnectionContext,
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
        fn on_byte_array_event(&mut self, event: builder::ByteArrayEvent) {
            let event = event.into_event();
            self.subscriber
                .on_byte_array_event(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_enum_event(&mut self, event: builder::EnumEvent) {
            let event = event.into_event();
            self.subscriber
                .on_enum_event(self.context, &self.meta, &event);
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
    use crate::event::snapshot::Location;
    pub mod endpoint {
        use super::*;
        pub struct Subscriber {
            location: Option<Location>,
            output: Vec<String>,
            pub count_event: u64,
        }
        impl Drop for Subscriber {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    return;
                }
                if let Some(location) = self.location.as_ref() {
                    location.snapshot_log(&self.output);
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
                    count_event: 0,
                }
            }
        }
        impl super::super::Subscriber for Subscriber {
            type ConnectionContext = ();
            fn create_connection_context(
                &mut self,
                _meta: &api::ConnectionMeta,
                _info: &api::ConnectionInfo,
            ) -> Self::ConnectionContext {
            }
            fn on_count_event(&mut self, meta: &api::EndpointMeta, event: &api::CountEvent) {
                self.count_event += 1;
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
    }
    #[derive(Debug)]
    pub struct Subscriber {
        location: Option<Location>,
        output: Vec<String>,
        pub byte_array_event: u64,
        pub enum_event: u64,
        pub count_event: u64,
    }
    impl Drop for Subscriber {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            if let Some(location) = self.location.as_ref() {
                location.snapshot_log(&self.output);
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
                byte_array_event: 0,
                enum_event: 0,
                count_event: 0,
            }
        }
    }
    impl super::Subscriber for Subscriber {
        type ConnectionContext = ();
        fn create_connection_context(
            &mut self,
            _meta: &api::ConnectionMeta,
            _info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
        }
        fn on_byte_array_event(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ByteArrayEvent,
        ) {
            self.byte_array_event += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_enum_event(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::EnumEvent,
        ) {
            self.enum_event += 1;
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.push(out);
            }
        }
        fn on_count_event(&mut self, meta: &api::EndpointMeta, event: &api::CountEvent) {
            self.count_event += 1;
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.push(out);
        }
    }
    #[derive(Debug)]
    pub struct Publisher {
        location: Option<Location>,
        output: Vec<String>,
        pub byte_array_event: u64,
        pub enum_event: u64,
        pub count_event: u64,
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
                byte_array_event: 0,
                enum_event: 0,
                count_event: 0,
            }
        }
    }
    impl super::EndpointPublisher for Publisher {
        fn on_count_event(&mut self, event: builder::CountEvent) {
            self.count_event += 1;
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.push(out);
        }
        fn quic_version(&self) -> Option<u32> {
            Some(1)
        }
    }
    impl super::ConnectionPublisher for Publisher {
        fn on_byte_array_event(&mut self, event: builder::ByteArrayEvent) {
            self.byte_array_event += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
            }
        }
        fn on_enum_event(&mut self, event: builder::EnumEvent) {
            self.enum_event += 1;
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.push(out);
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
                location.snapshot_log(&self.output);
            }
        }
    }
}
