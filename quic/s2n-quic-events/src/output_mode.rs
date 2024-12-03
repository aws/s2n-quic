// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

#[derive(Debug, Default)]
pub enum OutputMode {
    Ref,
    #[default]
    Mut,
}

impl OutputMode {
    pub fn is_ref(&self) -> bool {
        matches!(self, Self::Ref)
    }

    #[allow(unused)]
    pub fn is_mut(&self) -> bool {
        matches!(self, Self::Mut)
    }

    pub fn receiver(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(),
            OutputMode::Mut => quote!(mut),
        }
    }

    pub fn counter_type(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(AtomicU64),
            OutputMode::Mut => quote!(u64),
        }
    }

    pub fn counter_init(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(AtomicU64::new(0)),
            OutputMode::Mut => quote!(0),
        }
    }

    pub fn counter_increment(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(.fetch_add(1, Ordering::Relaxed)),
            OutputMode::Mut => quote!(+= 1),
        }
    }

    pub fn counter_increment_by(&self, value: TokenStream) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(.fetch_add(#value, Ordering::Relaxed)),
            OutputMode::Mut => quote!(+= #value),
        }
    }

    pub fn counter_load(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(.load(Ordering::Relaxed)),
            OutputMode::Mut => quote!(),
        }
    }

    pub fn lock(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(.lock().unwrap()),
            OutputMode::Mut => quote!(),
        }
    }

    pub fn imports(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(
                use core::sync::atomic::{AtomicU64, Ordering};
            ),
            OutputMode::Mut => quote!(),
        }
    }

    pub fn mutex(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(
                use std::sync::Mutex;
            ),
            OutputMode::Mut => quote!(),
        }
    }

    pub fn testing_output_type(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(Mutex<Vec<String>>),
            OutputMode::Mut => quote!(Vec<String>),
        }
    }

    pub fn trait_constraints(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!('static + Send + Sync),
            OutputMode::Mut => quote!('static + Send),
        }
    }

    pub fn query_mut(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(),
            OutputMode::Mut => quote!(
                /// Used for querying and mutating the `Subscriber::ConnectionContext` on a Subscriber
                #[inline]
                fn query_mut(
                    context: &mut Self::ConnectionContext,
                    query: &mut dyn query::QueryMut,
                ) -> query::ControlFlow {
                    query.execute_mut(context)
                }
            ),
        }
    }

    pub fn query_mut_tuple(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(),
            OutputMode::Mut => quote!(
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
            ),
        }
    }

    pub fn supervisor(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(),
            OutputMode::Mut => quote!(
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
                        Close { error_code: application::Error },

                        /// Close the connection without notifying the peer
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
            ),
        }
    }

    pub fn supervisor_timeout(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(),
            OutputMode::Mut => quote!(
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
                fn supervisor_timeout(
                    &mut self,
                    conn_context: &mut Self::ConnectionContext,
                    meta: &api::ConnectionMeta,
                    context: &supervisor::Context,
                ) -> Option<Duration> {
                    None
                }

                /// Called for each `supervisor_timeout` to determine any action to take on the connection based on the `supervisor::Outcome`
                ///
                /// If multiple `event::Subscriber`s are composed together, the minimum `supervisor_timeout`
                /// across all `event::Subscriber`s will be used, and thus `on_supervisor_timeout` may be called
                /// earlier than the `supervisor_timeout` for a given `event::Subscriber` implementation.
                #[allow(unused_variables)]
                fn on_supervisor_timeout(
                    &mut self,
                    conn_context: &mut Self::ConnectionContext,
                    meta: &api::ConnectionMeta,
                    context: &supervisor::Context,
                ) -> supervisor::Outcome {
                    supervisor::Outcome::default()
                }
            ),
        }
    }

    pub fn supervisor_timeout_tuple(&self) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(),
            OutputMode::Mut => quote!(
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
                    let outcome_a =
                        self.0
                            .on_supervisor_timeout(&mut conn_context.0, meta, context);
                    let outcome_b =
                        self.1
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
            ),
        }
    }

    pub fn ref_subscriber(&self, inner: TokenStream) -> TokenStream {
        match self {
            OutputMode::Ref => quote!(
                impl<T: Subscriber> Subscriber for std::sync::Arc<T> {
                    #inner
                }
            ),
            OutputMode::Mut => quote!(),
        }
    }
}

impl ToTokens for OutputMode {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(self.receiver());
    }
}
