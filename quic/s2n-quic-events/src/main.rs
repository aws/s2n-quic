// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

type Error = Box<dyn std::error::Error>;
type Result<T, E = Error> = core::result::Result<T, E>;

mod parser;

#[derive(Debug, Default)]
struct Output {
    pub subscriber: TokenStream,
    pub endpoint_publisher: TokenStream,
    pub endpoint_publisher_subscriber: TokenStream,
    pub connection_publisher: TokenStream,
    pub connection_publisher_subscriber: TokenStream,
    pub tuple_subscriber: TokenStream,
    pub tracing_subscriber: TokenStream,
    pub bpf_subscriber: TokenStream,
    pub builders: TokenStream,
    pub api: TokenStream,
    pub testing_fields: TokenStream,
    pub testing_fields_init: TokenStream,
    pub subscriber_testing: TokenStream,
    pub endpoint_publisher_testing: TokenStream,
    pub connection_publisher_testing: TokenStream,
    pub extra: TokenStream,

    // eBPF Rust repr_c structs
    pub mode_rust_reprc: bool,
    pub rust_bpf_reprc: TokenStream,
    // eBPF C header file
    pub mode_c_reprc: bool,
    pub c_bpf_reprc: TokenStream,
}

impl Output {
    fn to_tokens_c_reprc(&self, tokens: &mut TokenStream) {
        let Output {
            subscriber: _,
            endpoint_publisher: _,
            endpoint_publisher_subscriber: _,
            connection_publisher: _,
            connection_publisher_subscriber: _,
            tuple_subscriber: _,
            tracing_subscriber: _,
            bpf_subscriber: _,
            builders: _,
            api: _,
            testing_fields: _,
            testing_fields_init: _,
            subscriber_testing: _,
            endpoint_publisher_testing: _,
            connection_publisher_testing: _,
            extra: _,
            mode_rust_reprc: _,
            rust_bpf_reprc: _,
            mode_c_reprc: _,
            c_bpf_reprc,
        } = self;

        tokens.extend(quote!(
            #c_bpf_reprc
        ));
    }

    fn to_tokens_rust_reprc(&self, tokens: &mut TokenStream) {
        let Output {
            subscriber: _,
            endpoint_publisher: _,
            endpoint_publisher_subscriber: _,
            connection_publisher: _,
            connection_publisher_subscriber: _,
            tuple_subscriber: _,
            tracing_subscriber: _,
            bpf_subscriber: _,
            builders: _,
            api: _,
            testing_fields: _,
            testing_fields_init: _,
            subscriber_testing: _,
            endpoint_publisher_testing: _,
            connection_publisher_testing: _,
            extra: _,
            mode_rust_reprc: _,
            rust_bpf_reprc,
            mode_c_reprc: _,
            c_bpf_reprc: _,
        } = self;

        tokens.extend(quote!(
            use super::{api, bpf::IntoBpf};

            #rust_bpf_reprc
        ));
    }
}

impl ToTokens for Output {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if self.mode_c_reprc {
            self.to_tokens_c_reprc(tokens);
            return;
        }

        if self.mode_rust_reprc {
            self.to_tokens_rust_reprc(tokens);
            return;
        }

        let Output {
            subscriber,
            endpoint_publisher,
            endpoint_publisher_subscriber,
            connection_publisher,
            connection_publisher_subscriber,
            tuple_subscriber,
            tracing_subscriber,
            bpf_subscriber,
            builders,
            api,
            testing_fields,
            testing_fields_init,
            subscriber_testing,
            endpoint_publisher_testing,
            connection_publisher_testing,
            extra,
            mode_rust_reprc: _,
            rust_bpf_reprc: _,
            mode_c_reprc: _,
            c_bpf_reprc: _,
        } = self;

        tokens.extend(quote!(
            use super::*;

            pub mod api {
                //! This module contains events that are emitted to the [`Subscriber`](crate::event::Subscriber)
                use super::*;

                pub use traits::Subscriber;

                #api

                #extra
            }

            #[cfg(feature = "event-tracing")]
            pub mod tracing {
                //! This module contains event integration with [`tracing`](https://docs.rs/tracing)
                use super::api;

                /// Emits events with [`tracing`](https://docs.rs/tracing)
                #[derive(Clone, Debug)]
                pub struct Subscriber {
                    client: tracing::Span,
                    server: tracing::Span,
                }

                impl Default for Subscriber {
                    fn default() -> Self {
                        let root = tracing::span!(target: "s2n_quic", tracing::Level::DEBUG, "s2n_quic");
                        let client = tracing::span!(parent: root.id(), tracing::Level::DEBUG, "client");
                        let server = tracing::span!(parent: root.id(), tracing::Level::DEBUG, "server");

                        Self {
                            client,
                            server,
                        }
                    }
                }

                impl super::Subscriber for Subscriber {
                    type ConnectionContext = tracing::Span;

                    fn create_connection_context(&mut self, meta: &api::ConnectionMeta, _info: &api::ConnectionInfo) -> Self::ConnectionContext {
                        let parent = match meta.endpoint_type {
                            api::EndpointType::Client {} => {
                                self.client.id()
                            }
                            api::EndpointType::Server {} => {
                                self.server.id()
                            }
                        };
                        tracing::span!(target: "s2n_quic", parent: parent, tracing::Level::DEBUG, "conn", id = meta.id)
                    }

                    #tracing_subscriber
                }
            }

            #[cfg(all(s2n_quic_unstable, feature = "event-bpf"))]
            pub mod bpf {
                //! This module contains event integration with [`tracing`](https://docs.rs/tracing)
                use super::api;
                use probe::probe;
                use crate::event::bpf::{ IntoBpf};
                use crate::event::generated_bpf;

                /// Emits events with [`tracing`](https://docs.rs/tracing)
                #[derive(Clone, Debug, Default)]
                pub struct Subscriber;

                impl super::Subscriber for Subscriber {
                    type ConnectionContext = ();

                    fn create_connection_context(&mut self, meta: &api::ConnectionMeta, _info: &api::ConnectionInfo) -> Self::ConnectionContext {
                        probe!(s2n_quic, create_connection_context, meta.id);
                    }

                    #bpf_subscriber
                }
            }

            pub mod builder {
                use super::*;

                #builders
            }

            pub mod supervisor {
                //! This module contains the `supervisor::Outcome` and `supervisor::Context` for use
                //! when implementing [`Subscriber::supervisor_timeout`](crate::event::Subscriber::supervisor_timeout) and
                //! [`Subscriber::on_supervisor_timeout`](crate::event::Subscriber::on_supervisor_timeout)
                //! on a Subscriber.

                use crate::{
                    application,
                    event::{builder::SocketAddress, IntoEvent},
                };

                #[non_exhaustive]
                #[derive(Clone, Debug, Eq, PartialEq)]
                pub enum Outcome {
                    /// Allow the connection to remain open
                    Continue,

                    /// Close the connection and notify the peer
                    Close {error_code: application::Error},

                    /// Close the connection without notifying the peer
                    ImmediateClose {reason: &'static str},
                }

                impl Default for Outcome {
                    fn default() -> Self {
                        Self::Continue
                    }
                }

                #[non_exhaustive]
                #[derive(Debug)]
                pub struct Context<'a> {
                    /// Number of handshakes that have begun but not completed
                    pub inflight_handshakes: usize,

                    /// Number of open connections
                    pub connection_count: usize,

                    /// The address of the peer
                    pub remote_address: SocketAddress<'a>,

                    /// True if the connection is in the handshake state, false otherwise
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
                use api::*;
                use core::fmt;

                /// Provides metadata related to an event
                pub trait Meta: fmt::Debug {
                    /// Returns whether the local endpoint is a Client or Server
                    fn endpoint_type(&self) -> &EndpointType;

                    /// A context from which the event is being emitted
                    ///
                    /// An event can occur in the context of an Endpoint or Connection
                    fn subject(&self) -> Subject;

                    /// The time the event occurred
                    fn timestamp(&self) -> &crate::event::Timestamp;
                }

                impl Meta for ConnectionMeta {
                    fn endpoint_type(&self) -> &EndpointType {
                        &self.endpoint_type
                    }

                    fn subject(&self) -> Subject {
                        Subject::Connection { id : self.id }
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

                /// Allows for events to be subscribed to
                pub trait Subscriber: 'static + Send {

                    /// An application provided type associated with each connection.
                    ///
                    /// The context provides a mechanism for applications to provide a custom type
                    /// and update it on each event, e.g. computing statistics. Each event
                    /// invocation (e.g. [`Subscriber::on_packet_sent`]) also provides mutable
                    /// access to the context `&mut ConnectionContext` and allows for updating the
                    /// context.
                    ///
                    /// ```no_run
                    /// # mod s2n_quic { pub mod provider { pub mod event {
                    /// #     pub use s2n_quic_core::event::{api as events, api::ConnectionInfo, api::ConnectionMeta, Subscriber};
                    /// # }}}
                    /// use s2n_quic::provider::event::{
                    ///     ConnectionInfo, ConnectionMeta, Subscriber, events::PacketSent
                    /// };
                    ///
                    /// pub struct MyEventSubscriber;
                    ///
                    /// pub struct MyEventContext {
                    ///     packet_sent: u64,
                    /// }
                    ///
                    /// impl Subscriber for MyEventSubscriber {
                    ///     type ConnectionContext = MyEventContext;
                    ///
                    ///     fn create_connection_context(
                    ///         &mut self, _meta: &ConnectionMeta,
                    ///         _info: &ConnectionInfo,
                    ///     ) -> Self::ConnectionContext {
                    ///         MyEventContext { packet_sent: 0 }
                    ///     }
                    ///
                    ///     fn on_packet_sent(
                    ///         &mut self,
                    ///         context: &mut Self::ConnectionContext,
                    ///         _meta: &ConnectionMeta,
                    ///         _event: &PacketSent,
                    ///     ) {
                    ///         context.packet_sent += 1;
                    ///     }
                    /// }
                    ///  ```
                    type ConnectionContext: 'static + Send;

                    /// Creates a context to be passed to each connection-related event
                    fn create_connection_context(&mut self, meta: &ConnectionMeta, info: &ConnectionInfo) -> Self::ConnectionContext;

                    /// The period at which `on_supervisor_timeout` is called
                    ///
                    /// If multiple `event::Subscriber`s are composed together, the minimum `supervisor_timeout`
                    /// across all `event::Subscriber`s will be used.
                    ///
                    /// If the `supervisor_timeout()` is `None` across all `event::Subscriber`s, connection supervision
                    /// will cease for the remaining lifetime of the connection and `on_supervisor_timeout` will no longer
                    /// be called.
                    ///
                    /// It is recommended to avoid setting this value less than ~100ms, as short durations
                    /// may lead to higher CPU utilization.
                    #[allow(unused_variables)]
                    fn supervisor_timeout(&mut self, conn_context: &mut Self::ConnectionContext, meta: &ConnectionMeta, context: &supervisor::Context) -> Option<Duration> {
                        None
                    }

                    /// Called for each `supervisor_timeout` to determine any action to take on the connection based on the `supervisor::Outcome`
                    ///
                    /// If multiple `event::Subscriber`s are composed together, the minimum `supervisor_timeout`
                    /// across all `event::Subscriber`s will be used, and thus `on_supervisor_timeout` may be called
                    /// earlier than the `supervisor_timeout` for a given `event::Subscriber` implementation.
                    #[allow(unused_variables)]
                    fn on_supervisor_timeout(&mut self, conn_context: &mut Self::ConnectionContext, meta: &ConnectionMeta, context: &supervisor::Context) -> supervisor::Outcome {
                        supervisor::Outcome::default()
                    }

                    #subscriber

                    /// Called for each event that relates to the endpoint and all connections
                    #[inline]
                    fn on_event<M: Meta, E: Event>(&mut self, meta: &M, event: &E) {
                        let _ = meta;
                        let _ = event;
                    }

                    /// Called for each event that relates to a connection
                    #[inline]
                    fn on_connection_event<E: Event>(&mut self, context: &mut Self::ConnectionContext, meta: &ConnectionMeta, event: &E) {
                        let _ = context;
                        let _ = meta;
                        let _ = event;
                    }

                    /// Used for querying the `Subscriber::ConnectionContext` on a Subscriber
                    #[inline]
                    fn query(context: &Self::ConnectionContext, query: &mut dyn query::Query) -> query::ControlFlow {
                        query.execute(context)
                    }

                    /// Used for querying and mutating the `Subscriber::ConnectionContext` on a Subscriber
                    #[inline]
                    fn query_mut(context: &mut Self::ConnectionContext, query: &mut dyn query::QueryMut) -> query::ControlFlow {
                        query.execute_mut(context)
                    }
                }

                /// Subscriber is implemented for a 2-element tuple to make it easy to compose multiple
                /// subscribers.
                impl<A, B> Subscriber for (A, B)
                    where
                        A: Subscriber,
                        B: Subscriber,
                {
                    type ConnectionContext = (A::ConnectionContext, B::ConnectionContext);

                    #[inline]
                    fn create_connection_context(&mut self, meta: &ConnectionMeta, info: &ConnectionInfo) -> Self::ConnectionContext {
                        (self.0.create_connection_context(meta, info), self.1.create_connection_context(meta, info))
                    }

                    #[inline]
                    fn supervisor_timeout(&mut self, conn_context: &mut Self::ConnectionContext, meta: &ConnectionMeta, context: &supervisor::Context) -> Option<Duration> {
                        let timeout_a = self.0.supervisor_timeout(&mut conn_context.0, meta, context);
                        let timeout_b = self.1.supervisor_timeout(&mut conn_context.1, meta, context);
                        match (timeout_a, timeout_b) {
                            (None, None) => None,
                            (None, Some(timeout)) | (Some(timeout), None) => Some(timeout),
                            (Some(a), Some(b)) => Some(a.min(b)),
                        }
                    }

                    #[inline]
                    fn on_supervisor_timeout(&mut self, conn_context: &mut Self::ConnectionContext, meta: &ConnectionMeta, context: &supervisor::Context) -> supervisor::Outcome {
                        let outcome_a = self.0.on_supervisor_timeout(&mut conn_context.0, meta, context);
                        let outcome_b = self.1.on_supervisor_timeout(&mut conn_context.1, meta, context);
                        match (outcome_a, outcome_b) {
                            (supervisor::Outcome::ImmediateClose { reason }, _) | (_, supervisor::Outcome::ImmediateClose { reason }) => supervisor::Outcome::ImmediateClose { reason },
                            (supervisor::Outcome::Close { error_code }, _) | (_, supervisor::Outcome::Close { error_code }) => supervisor::Outcome::Close { error_code },
                            _ => supervisor::Outcome::Continue,
                        }
                    }

                    #tuple_subscriber

                    #[inline]
                    fn on_event<M: Meta, E: Event>(&mut self, meta: &M, event: &E) {
                        self.0.on_event(meta, event);
                        self.1.on_event(meta, event);
                    }

                    #[inline]
                    fn on_connection_event<E: Event>(&mut self, context: &mut Self::ConnectionContext, meta: &ConnectionMeta, event: &E) {
                        self.0.on_connection_event(&mut context.0, meta, event);
                        self.1.on_connection_event(&mut context.1, meta, event);
                    }

                    #[inline]
                    fn query(context: &Self::ConnectionContext, query: &mut dyn query::Query) -> query::ControlFlow {
                        query.execute(context)
                            .and_then(|| A::query(&context.0, query))
                            .and_then(|| B::query(&context.1, query))
                    }

                    #[inline]
                    fn query_mut(context: &mut Self::ConnectionContext, query: &mut dyn query::QueryMut) -> query::ControlFlow {
                        query.execute_mut(context)
                            .and_then(|| A::query_mut(&mut context.0, query))
                            .and_then(|| B::query_mut(&mut context.1, query))
                    }
                }

                pub trait EndpointPublisher {
                    #endpoint_publisher

                    /// Returns the QUIC version, if any
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
                    #endpoint_publisher_subscriber

                    #[inline]
                    fn quic_version(&self) -> Option<u32> {
                        self.quic_version
                    }
                }

                pub trait ConnectionPublisher {
                    #connection_publisher

                    /// Returns the QUIC version negotiated for the current connection, if any
                    fn quic_version(&self) -> u32;

                    /// Returns the [`Subject`] for the current publisher
                    fn subject(&self) -> Subject;
                }

                pub struct ConnectionPublisherSubscriber<'a, Sub: Subscriber> {
                    meta: ConnectionMeta,
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
                        context: &'a mut Sub::ConnectionContext
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
                    #connection_publisher_subscriber

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

                #[derive(Clone, Debug)]
                pub struct Subscriber {
                    location: Option<Location>,
                    output: Vec<String>,
                    #testing_fields
                }

                impl Drop for Subscriber {
                    fn drop(&mut self) {
                        // don't make any assertions if we're already failing the test
                        if std::thread::panicking() {
                            return;
                        }

                        if let Some(location) = self.location.as_ref() {
                            location.snapshot(&self.output);
                        }
                    }
                }

                impl Subscriber {
                    /// Creates a subscriber with snapshot assertions enabled
                    #[track_caller]
                    pub fn snapshot() -> Self {
                        let mut sub = Self::no_snapshot();
                        sub.location = Location::try_new();
                        sub
                    }

                    /// Creates a subscriber with snapshot assertions disabled
                    pub fn no_snapshot() -> Self {
                        Self {
                            location: None,
                            output: Default::default(),
                            #testing_fields_init
                        }
                    }
                }

                impl super::Subscriber for Subscriber {
                    type ConnectionContext = ();

                    fn create_connection_context(&mut self, _meta: &api::ConnectionMeta, _info: &api::ConnectionInfo) -> Self::ConnectionContext {}

                    #subscriber_testing
                }

                #[derive(Clone, Debug)]
                pub struct Publisher {
                    location: Option<Location>,
                    output: Vec<String>,
                    #testing_fields
                }

                impl Publisher {
                    /// Creates a publisher with snapshot assertions enabled
                    #[track_caller]
                    pub fn snapshot() -> Self {
                        let mut sub = Self::no_snapshot();
                        sub.location = Location::try_new();
                        sub
                    }

                    /// Creates a publisher with snapshot assertions disabled
                    pub fn no_snapshot() -> Self {
                        Self {
                            location: None,
                            output: Default::default(),
                            #testing_fields_init
                        }
                    }
                }

                impl super::EndpointPublisher for Publisher {
                    #endpoint_publisher_testing

                    fn quic_version(&self) -> Option<u32> {
                        Some(1)
                    }
                }

                impl super::ConnectionPublisher for Publisher {
                    #connection_publisher_testing

                    fn quic_version(&self) -> u32 {
                        1
                    }

                    fn subject(&self) -> api::Subject {
                        api::Subject::Connection { id: 0 }
                    }
                }

                impl Drop for Publisher {
                    fn drop(&mut self) {
                        // don't make any assertions if we're already failing the test
                        if std::thread::panicking() {
                            return;
                        }

                        if let Some(location) = self.location.as_ref() {
                            location.snapshot(&self.output);
                        }
                    }
                }

                #[derive(Clone, Debug)]
                struct Location(&'static core::panic::Location<'static>);

                impl Location {
                    #[track_caller]
                    fn try_new() -> Option<Self> {
                        let thread = std::thread::current();

                        // only create a location if insta can figure out the test name from the
                        // thread
                        if thread.name().map_or(false, |name| name != "main") {
                            Some(Self(core::panic::Location::caller()))
                        } else {
                            None
                        }
                    }

                    fn snapshot(&self, output: &[String]) {
                        use std::path::{Path, Component};
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
        ));
    }
}

fn main() -> Result<()> {
    let mut files = vec![];

    for path in glob::glob(concat!(env!("CARGO_MANIFEST_DIR"), "/events/**/*.rs"))? {
        let path = path?;
        let file = std::fs::read_to_string(path)?;
        files.push(parser::parse(&file).unwrap());
    }

    let mut output = Output::default();

    for file in &files {
        file.to_tokens(&mut output);
    }

    generate_events(&output)?;
    generate_c_bpf(&mut output)?;
    generate_rust_bpf(&mut output)?;

    Ok(())
}

fn generate_events(output: &Output) -> Result<()> {
    let generated = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../s2n-quic-core/src/event/generated.rs"
    );

    let mut o = std::fs::File::create(generated)?;

    macro_rules! put {
        ($($arg:tt)*) => {{
            use std::io::Write;
            writeln!(o, $($arg)*)?;
        }}
    }

    put!("// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.");
    put!("// SPDX-License-Identifier: Apache-2.0");
    put!();
    put!("// DO NOT MODIFY THIS FILE");
    put!("// This file was generated with the `s2n-quic-events` crate and any required");
    put!("// changes should be made there.");
    put!();
    put!("{}", output.to_token_stream());

    let status = std::process::Command::new("rustfmt")
        .arg(generated)
        .spawn()?
        .wait()?;

    assert!(status.success());
    Ok(())
}

fn generate_rust_bpf(output: &mut Output) -> Result<()> {
    output.mode_rust_reprc = true;
    let generated = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../s2n-quic-core/src/event/generated_bpf.rs"
    );

    let mut o = std::fs::File::create(generated)?;

    macro_rules! put {
        ($($arg:tt)*) => {{
            use std::io::Write;
            writeln!(o, $($arg)*)?;
        }}
    }

    put!("// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.");
    put!("// SPDX-License-Identifier: Apache-2.0");
    put!();
    put!("// DO NOT MODIFY THIS FILE");
    put!("// This file was generated with the `s2n-quic-events` crate and any required");
    put!("// changes should be made there.");
    put!();
    put!("{}", output.to_token_stream());

    let status = std::process::Command::new("rustfmt")
        .arg(generated)
        .spawn()?
        .wait()?;

    assert!(status.success());
    output.mode_rust_reprc = false;
    Ok(())
}

fn generate_c_bpf(output: &mut Output) -> Result<()> {
    output.mode_c_reprc = true;
    let generated = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../s2n-quic-core/src/event/generated_s2n_quic_bpf_events.h"
    );

    let mut o = std::fs::File::create(generated)?;

    macro_rules! put {
        ($($arg:tt)*) => {{
            use std::io::Write;
            writeln!(o, $($arg)*)?;
        }}
    }

    put!("/*");
    put!(" * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.");
    put!(" * SPDX-License-Identifier: Apache-2.0");
    put!(" */");
    put!();
    put!("/* DO NOT MODIFY THIS FILE");
    put!(" * This file was generated with the `s2n-quic-events` crate and any required");
    put!(" * changes should be made there.");
    put!(" */");
    put!();
    put!("#include <linux/path.h>");
    put!();
    put!("{}", output.to_token_stream());

    let status = std::process::Command::new("clang-format")
        .arg("-i")
        .arg(generated)
        .spawn()?
        .wait()?;

    assert!(status.success());

    output.mode_c_reprc = false;
    Ok(())
}
