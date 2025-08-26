// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{parser::File, OutputMode};
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use std::path::{Path, PathBuf};

pub mod metrics;

#[derive(Debug, Default)]
pub struct Output {
    pub subscriber: TokenStream,
    pub endpoint_publisher: TokenStream,
    pub endpoint_publisher_subscriber: TokenStream,
    pub connection_publisher: TokenStream,
    pub connection_publisher_subscriber: TokenStream,
    pub tuple_subscriber: TokenStream,
    pub ref_subscriber: TokenStream,
    pub tracing_subscriber: TokenStream,
    pub tracing_subscriber_attr: TokenStream,
    pub tracing_subscriber_def: TokenStream,
    pub builders: TokenStream,
    pub api: TokenStream,
    pub testing_fields: TokenStream,
    pub testing_fields_init: TokenStream,
    pub subscriber_testing: TokenStream,
    pub endpoint_subscriber_testing: TokenStream,
    pub endpoint_testing_fields: TokenStream,
    pub endpoint_testing_fields_init: TokenStream,
    pub endpoint_publisher_testing: TokenStream,
    pub connection_publisher_testing: TokenStream,
    pub extra: TokenStream,
    pub mode: OutputMode,
    pub crate_name: &'static str,
    pub s2n_quic_core_path: TokenStream,
    pub top_level: TokenStream,
    pub feature_alloc: TokenStream,
    pub root: PathBuf,
}

impl Output {
    pub fn generate(&mut self, files: &[File]) {
        for file in files {
            file.to_tokens(self);
        }

        self.top_level.extend(metrics::emit(self, files));

        self.emit("generated.rs", &self);
    }

    pub fn emit<P: AsRef<Path>, T: ToTokens>(&self, path: P, output: T) {
        let path = self.root.join(path);

        let _ = std::fs::create_dir_all(path.parent().unwrap());

        let mut o = std::fs::File::create(&path).unwrap();

        macro_rules! put {
            ($($arg:tt)*) => {{
                use std::io::Write;
                writeln!(o, $($arg)*).unwrap();
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
            .arg(&path)
            .spawn()
            .unwrap()
            .wait()
            .unwrap();

        assert!(status.success());

        eprintln!("  wrote {}", path.display());
    }
}

impl ToTokens for Output {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Output {
            subscriber,
            endpoint_publisher,
            endpoint_publisher_subscriber,
            connection_publisher,
            connection_publisher_subscriber,
            tuple_subscriber,
            ref_subscriber,
            tracing_subscriber,
            tracing_subscriber_attr,
            tracing_subscriber_def,
            builders,
            api,
            testing_fields,
            testing_fields_init,
            subscriber_testing,
            endpoint_subscriber_testing,
            endpoint_testing_fields,
            endpoint_testing_fields_init,
            endpoint_publisher_testing,
            connection_publisher_testing,
            extra,
            mode,
            s2n_quic_core_path,
            top_level,
            feature_alloc: _,
            crate_name,
            root: _,
        } = self;

        let imports = self.mode.imports();
        let mutex = self.mode.mutex();
        let testing_output_type = self.mode.testing_output_type();
        let lock = self.mode.lock();
        let supervisor = self.mode.supervisor();
        let supervisor_timeout = self.mode.supervisor_timeout();
        let supervisor_timeout_tuple = self.mode.supervisor_timeout_tuple();
        let query_mut = self.mode.query_mut();
        let query_mut_tuple = self.mode.query_mut_tuple();
        let trait_constraints = self.mode.trait_constraints();

        let ref_subscriber = self.mode.ref_subscriber(quote!(
            type ConnectionContext = T::ConnectionContext;

            #[inline]
            fn create_connection_context(
                &#mode self,
                meta: &api::ConnectionMeta,
                info: &api::ConnectionInfo
            ) -> Self::ConnectionContext {
                self.as_ref().create_connection_context(meta, info)
            }

            #ref_subscriber

            #[inline]
            fn on_event<M: Meta, E: Event>(&#mode self, meta: &M, event: &E) {
                self.as_ref().on_event(meta, event);
            }

            #[inline]
            fn on_connection_event<E: Event>(
                &#mode self,
                context: &#mode Self::ConnectionContext,
                meta: &api::ConnectionMeta,
                event: &E
            ) {
                self.as_ref().on_connection_event(context, meta, event);
            }
        ));

        tokens.extend(quote!(
            #![allow(clippy::needless_lifetimes)]

            use super::*;

            #top_level

            pub mod api {
                //! This module contains events that are emitted to the [`Subscriber`](crate::event::Subscriber)
                use super::*;

                // we may or may not need to derive traits aggregate metrics
                #[allow(unused_imports)]
                use crate::event::metrics::aggregate;

                pub use traits::Subscriber;

                #api

                #extra
            }

            #tracing_subscriber_attr
            pub mod tracing {
                //! This module contains event integration with [`tracing`](https://docs.rs/tracing)
                use super::api;

                #tracing_subscriber_def

                impl super::Subscriber for Subscriber {
                    type ConnectionContext = tracing::Span;

                    fn create_connection_context(
                        &#mode self,
                        meta: &api::ConnectionMeta,
                        _info: &api::ConnectionInfo
                    ) -> Self::ConnectionContext {
                        let parent = self.parent(meta);
                        tracing::span!(target: #crate_name, parent: parent, tracing::Level::DEBUG, "conn", id = meta.id)
                    }

                    #tracing_subscriber
                }
            }

            pub mod builder {
                use super::*;

                #builders
            }

            #supervisor

            pub use traits::*;
            mod traits {
                use super::*;
                use core::fmt;
                use #s2n_quic_core_path::query;
                use crate::event::Meta;

                /// Allows for events to be subscribed to
                pub trait Subscriber: #trait_constraints {

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
                    type ConnectionContext: #trait_constraints;

                    /// Creates a context to be passed to each connection-related event
                    fn create_connection_context(
                        &#mode self,
                        meta: &api::ConnectionMeta,
                        info: &api::ConnectionInfo
                    ) -> Self::ConnectionContext;

                    #supervisor_timeout

                    #subscriber

                    /// Called for each event that relates to the endpoint and all connections
                    #[inline]
                    fn on_event<M: Meta, E: Event>(&#mode self, meta: &M, event: &E) {
                        let _ = meta;
                        let _ = event;
                    }

                    /// Called for each event that relates to a connection
                    #[inline]
                    fn on_connection_event<E: Event>(
                        &#mode self,
                        context: &#mode Self::ConnectionContext,
                        meta: &api::ConnectionMeta,
                        event: &E
                    ) {
                        let _ = context;
                        let _ = meta;
                        let _ = event;
                    }

                    /// Used for querying the `Subscriber::ConnectionContext` on a Subscriber
                    #[inline]
                    fn query(context: &Self::ConnectionContext, query: &mut dyn query::Query) -> query::ControlFlow {
                        query.execute(context)
                    }

                    #query_mut
                }

                #ref_subscriber

                /// Subscriber is implemented for a 2-element tuple to make it easy to compose multiple
                /// subscribers.
                impl<A, B> Subscriber for (A, B)
                    where
                        A: Subscriber,
                        B: Subscriber,
                {
                    type ConnectionContext = (A::ConnectionContext, B::ConnectionContext);

                    #[inline]
                    fn create_connection_context(
                        &#mode self,
                        meta: &api::ConnectionMeta,
                        info: &api::ConnectionInfo
                    ) -> Self::ConnectionContext {
                        (self.0.create_connection_context(meta, info), self.1.create_connection_context(meta, info))
                    }

                    #supervisor_timeout_tuple

                    #tuple_subscriber

                    #[inline]
                    fn on_event<M: Meta, E: Event>(&#mode self, meta: &M, event: &E) {
                        self.0.on_event(meta, event);
                        self.1.on_event(meta, event);
                    }

                    #[inline]
                    fn on_connection_event<E: Event>(
                        &#mode self,
                        context: &#mode Self::ConnectionContext,
                        meta: &api::ConnectionMeta,
                        event: &E
                    ) {
                        self.0.on_connection_event(&#mode context.0, meta, event);
                        self.1.on_connection_event(&#mode context.1, meta, event);
                    }

                    #[inline]
                    fn query(context: &Self::ConnectionContext, query: &mut dyn query::Query) -> query::ControlFlow {
                        query.execute(context)
                            .and_then(|| A::query(&context.0, query))
                            .and_then(|| B::query(&context.1, query))
                    }

                    #query_mut_tuple
                }

                pub trait EndpointPublisher {
                    #endpoint_publisher

                    /// Returns the QUIC version, if any
                    fn quic_version(&self) -> Option<u32>;
                }

                pub struct EndpointPublisherSubscriber<'a, Sub: Subscriber> {
                    meta: api::EndpointMeta,
                    quic_version: Option<u32>,
                    subscriber: &'a #mode Sub,
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
                        subscriber: &'a #mode Sub,
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
                    fn subject(&self) -> api::Subject;
                }

                pub struct ConnectionPublisherSubscriber<'a, Sub: Subscriber> {
                    meta: api::ConnectionMeta,
                    quic_version: u32,
                    subscriber: &'a #mode Sub,
                    context: &'a #mode Sub::ConnectionContext,
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
                        subscriber: &'a #mode Sub,
                        context: &'a #mode Sub::ConnectionContext
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
                use crate::event::snapshot::Location;
                #imports
                #mutex

                pub mod endpoint {
                    use super::*;

                    pub struct Subscriber {
                        location: Option<Location>,
                        output: #testing_output_type,
                        #endpoint_testing_fields
                    }

                    impl Drop for Subscriber {
                        fn drop(&mut self) {
                            // don't make any assertions if we're already failing the test
                            if std::thread::panicking() {
                                return;
                            }

                            if let Some(location) = self.location.as_ref() {
                                location.snapshot_log(&self.output #lock);
                            }
                        }
                    }

                    impl Subscriber {
                        /// Creates a subscriber with snapshot assertions enabled
                        #[track_caller]
                        pub fn snapshot() -> Self {
                            let mut sub = Self::no_snapshot();
                            sub.location = Location::from_thread_name();
                            sub
                        }

                        /// Creates a subscriber with snapshot assertions enabled
                        #[track_caller]
                        pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
                            let mut sub = Self::no_snapshot();
                            sub.location = Some(Location::new(name));
                            sub
                        }

                        /// Creates a subscriber with snapshot assertions disabled
                        pub fn no_snapshot() -> Self {
                            Self {
                                location: None,
                                output: Default::default(),
                                #endpoint_testing_fields_init
                            }
                        }
                    }

                    impl super::super::Subscriber for Subscriber {
                        type ConnectionContext = ();

                        fn create_connection_context(
                            &#mode self,
                            _meta: &api::ConnectionMeta,
                            _info: &api::ConnectionInfo
                        ) -> Self::ConnectionContext {}

                        #endpoint_subscriber_testing
                    }
                }

                #[derive(Debug)]
                pub struct Subscriber {
                    location: Option<Location>,
                    output: #testing_output_type,
                    #testing_fields
                }

                impl Drop for Subscriber {
                    fn drop(&mut self) {
                        // don't make any assertions if we're already failing the test
                        if std::thread::panicking() {
                            return;
                        }

                        if let Some(location) = self.location.as_ref() {
                            location.snapshot_log(&self.output #lock);
                        }
                    }
                }

                impl Subscriber {
                    /// Creates a subscriber with snapshot assertions enabled
                    #[track_caller]
                    pub fn snapshot() -> Self {
                        let mut sub = Self::no_snapshot();
                        sub.location = Location::from_thread_name();
                        sub
                    }

                    /// Creates a subscriber with snapshot assertions enabled
                    #[track_caller]
                    pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
                        let mut sub = Self::no_snapshot();
                        sub.location = Some(Location::new(name));
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

                    fn create_connection_context(
                        &#mode self,
                        _meta: &api::ConnectionMeta,
                        _info: &api::ConnectionInfo
                    ) -> Self::ConnectionContext {}

                    #subscriber_testing
                }

                #[derive(Debug)]
                pub struct Publisher {
                    location: Option<Location>,
                    output: #testing_output_type,
                    #testing_fields
                }

                impl Publisher {
                    /// Creates a publisher with snapshot assertions enabled
                    #[track_caller]
                    pub fn snapshot() -> Self {
                        let mut sub = Self::no_snapshot();
                        sub.location = Location::from_thread_name();
                        sub
                    }

                    /// Creates a subscriber with snapshot assertions enabled
                    #[track_caller]
                    pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
                        let mut sub = Self::no_snapshot();
                        sub.location = Some(Location::new(name));
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
                        builder::Subject::Connection { id: 0 }.into_event()
                    }
                }

                impl Drop for Publisher {
                    fn drop(&mut self) {
                        // don't make any assertions if we're already failing the test
                        if std::thread::panicking() {
                            return;
                        }

                        if let Some(location) = self.location.as_ref() {
                            location.snapshot_log(&self.output #lock);
                        }
                    }
                }
            }
        ));
    }
}
