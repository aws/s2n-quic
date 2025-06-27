// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{parser::File, Output};
use proc_macro2::TokenStream;
use quote::quote;

mod aggregate;

pub fn emit(output: &Output, files: &[File]) -> TokenStream {
    let events = files
        .iter()
        .flat_map(|file| file.structs.iter())
        .filter(|s| s.attrs.event_name.is_some())
        .filter(|s| {
            // Metrics is only connection-level events
            s.attrs.subject.is_connection()
        });

    let mode = &output.mode;

    let imports = output.mode.imports();
    let receiver = output.mode.receiver();
    let counter_increment = output.mode.counter_increment();
    let counter_type = output.mode.counter_type();
    let counter_init = output.mode.counter_init();
    let counter_load = output.mode.counter_load();

    let mut fields = quote!();
    let mut init = quote!();
    let mut record = quote!();
    let mut subscriber = quote!();

    for event in events {
        let ident = &event.ident;
        let counter = event.counter();
        let snake = event.ident_snake();
        let function = event.function();
        let allow_deprecated = &event.attrs.allow_deprecated;

        fields.extend(quote!(
            #counter: #counter_type,
        ));
        init.extend(quote!(
            #counter: #counter_init,
        ));

        record.extend(quote!(
            self.recorder.increment_counter(#snake, self.#counter #counter_load as _);
        ));

        subscriber.extend(quote!(
            #[inline]
            #allow_deprecated
            fn #function(
                &#receiver self,
                context: &#receiver Self::ConnectionContext,
                meta: &api::ConnectionMeta,
                event: &api::#ident
            ) {
                context.#counter #counter_increment;
                self.subscriber.#function(&#receiver context.recorder, meta, event);
            }
        ));
    }

    let aggregate = aggregate::emit(output, files);

    let tokens = quote!(
        #imports
        use crate::event::{metrics::Recorder, api, self};

        #aggregate

        #[derive(Debug)]
        pub struct Subscriber<S: event::Subscriber>
        where
            S::ConnectionContext: Recorder
        {
            subscriber: S,
        }

        impl<S: event::Subscriber> Subscriber<S>
        where
            S::ConnectionContext: Recorder
        {
            pub fn new(subscriber: S) -> Self {
                Self { subscriber }
            }
        }

        pub struct Context<R: Recorder> {
            recorder: R,
            #fields
        }

        impl<R: Recorder> Context<R> {
            pub fn inner(&self) -> &R {
                &self.recorder
            }

            pub fn inner_mut(&mut self) -> &mut R {
                &mut self.recorder
            }
        }

        impl<S: event::Subscriber> event::Subscriber for Subscriber<S>
            where S::ConnectionContext: Recorder {
            type ConnectionContext = Context<S::ConnectionContext>;

            fn create_connection_context(
                &#mode self,
                meta: &api::ConnectionMeta,
                info: &api::ConnectionInfo
            ) -> Self::ConnectionContext {
                Context {
                    recorder: self.subscriber.create_connection_context(meta, info),
                    #init
                }
            }

            #subscriber
        }

        impl<R: Recorder> Drop for Context<R> {
            fn drop(&mut self) {
                #record
            }
        }
    );

    let path = "generated/metrics.rs";

    output.emit(path, tokens);

    quote!(
        pub(crate) mod metrics;
    )
}
