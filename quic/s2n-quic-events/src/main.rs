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
    pub builders: TokenStream,
    pub api: TokenStream,
    pub testing_fields: TokenStream,
    pub subscriber_testing: TokenStream,
    pub endpoint_publisher_testing: TokenStream,
    pub connection_publisher_testing: TokenStream,
    pub extra: TokenStream,
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
            builders,
            api,
            testing_fields,
            subscriber_testing,
            endpoint_publisher_testing,
            connection_publisher_testing,
            extra,
        } = self;

        tokens.extend(quote!(
            use super::*;

            pub mod api {
                use super::*;

                pub use traits::Subscriber;

                #api

                #extra
            }

            pub mod builder {
                use super::*;

                #builders
            }

            pub use traits::*;
            mod traits {
                use super::*;
                use api::*;
                use core::fmt;

                pub trait Subscriber: 'static + Send {
                    type ConnectionContext: 'static + Send;

                    /// Creates a context to be passed to each connection-related event
                    fn create_connection_context(&mut self) -> Self::ConnectionContext;

                    #subscriber

                    /// Called for each event that relates to the endpoint and all connections
                    #[inline]
                    fn on_event<E: Event>(&mut self, meta: &Meta, event: &E) {
                        let _ = meta;
                        let _ = event;
                    }

                    /// Called for each event that relates to a connection
                    #[inline]
                    fn on_connection_event<E: Event>(&mut self, context: &mut Self::ConnectionContext, meta: &Meta, event: &E) {
                        let _ = context;
                        let _ = meta;
                        let _ = event;
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
                    fn create_connection_context(&mut self) -> Self::ConnectionContext {
                        (self.0.create_connection_context(), self.1.create_connection_context())
                    }

                    #tuple_subscriber

                    #[inline]
                    fn on_event<E: Event>(&mut self, meta: &Meta, event: &E) {
                        self.0.on_event(meta, event);
                        self.1.on_event(meta, event);
                    }

                    #[inline]
                    fn on_connection_event<E: Event>(&mut self, context: &mut Self::ConnectionContext, meta: &Meta, event: &E) {
                        self.0.on_connection_event(&mut context.0, meta, event);
                        self.1.on_connection_event(&mut context.1, meta, event);
                    }
                }

                pub trait EndpointPublisher {
                    #endpoint_publisher

                    /// Returns the QUIC version, if any
                    fn quic_version(&self) -> Option<u32>;
                }

                pub struct EndpointPublisherSubscriber<'a, Sub: Subscriber> {
                    meta: Meta,
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
                        meta: builder::Meta,
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
                }

                pub struct ConnectionPublisherSubscriber<'a, Sub: Subscriber> {
                    meta: Meta,
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
                        meta: builder::Meta,
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
                }
            }

            #[cfg(any(test, feature = "testing"))]
            pub mod testing {
                use super::*;

                #[derive(Copy, Clone, Debug, Default)]
                pub struct Subscriber {
                    #testing_fields
                }

                impl super::Subscriber for Subscriber {
                    type ConnectionContext = ();

                    fn create_connection_context(&mut self) -> Self::ConnectionContext {}

                    #subscriber_testing
                }

                #[derive(Copy, Clone, Debug, Default)]
                pub struct Publisher {
                    #testing_fields
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
