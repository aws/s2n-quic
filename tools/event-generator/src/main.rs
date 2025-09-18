// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use proc_macro2::TokenStream;
use quote::quote;
use s2n_events::{GenerateConfig, Output, OutputMode, Result, parser, validation};

struct EventInfo<'a> {
    input_path: &'a str,
    output_path: &'a str,
    crate_name: &'a str,
    generate_config: GenerateConfig,
    s2n_quic_core_path: TokenStream,
    api: TokenStream,
    builder: TokenStream,
    tracing_subscriber_attr: TokenStream,
    tracing_subscriber_def: TokenStream,
    feature_alloc: TokenStream,
}

impl EventInfo<'_> {
    fn s2n_quic() -> Self {
        let tracing_subscriber_def = quote!(
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

        impl Subscriber {
            fn parent<M: crate::event::Meta>(&self, meta: &M) -> Option<tracing::Id> {
                match meta.endpoint_type() {
                    api::EndpointType::Client { .. } => self.client.id(),
                    api::EndpointType::Server { .. } => self.server.id(),
                }
            }
        }
        );

        EventInfo {
            crate_name: "s2n_quic",
            input_path: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../quic/s2n-quic-core/events/**/*.rs"
            ),
            output_path: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../quic/s2n-quic-core/src/event"
            ),
            generate_config: GenerateConfig {
                mode: OutputMode::Mut,
            },
            s2n_quic_core_path: quote!(crate),
            api: quote!(),
            builder: quote!(),
            tracing_subscriber_attr: quote! {
                #[cfg(feature = "event-tracing")]
            },
            tracing_subscriber_def,
            feature_alloc: quote!(#[cfg(feature = "alloc")]),
        }
    }

    fn s2n_quic_dc() -> Self {
        let tracing_subscriber_def = quote!(
        /// Emits events with [`tracing`](https://docs.rs/tracing)
        #[derive(Clone, Debug)]
        pub struct Subscriber {
            root: tracing::Span,
        }

        impl Default for Subscriber {
            fn default() -> Self {
                let root = tracing::span!(target: "s2n_quic_dc", tracing::Level::DEBUG, "s2n_quic_dc");

                Self {
                    root,
                }
            }
        }

        impl Subscriber {
            fn parent<M: crate::event::Meta>(&self, _meta: &M) -> Option<tracing::Id> {
                self.root.id()
            }
        }
        );

        EventInfo {
            crate_name: "s2n_quic_dc",
            input_path: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../dc/s2n-quic-dc/events/**/*.rs"
            ),
            output_path: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../dc/s2n-quic-dc/src/event"
            ),
            generate_config: GenerateConfig {
                mode: OutputMode::Ref,
            },
            s2n_quic_core_path: quote!(s2n_quic_core),
            api: quote! {
                pub use s2n_quic_core::event::api::{
                    Subject,
                    EndpointType,
                    SocketAddress,
                };
            },
            builder: quote! {
                pub use s2n_quic_core::event::builder::{
                    Subject,
                    EndpointType,
                    SocketAddress,
                };
            },
            tracing_subscriber_attr: quote!(),
            tracing_subscriber_def,
            feature_alloc: quote!(),
        }
    }
}

fn main() -> Result<()> {
    let event_paths = [EventInfo::s2n_quic(), EventInfo::s2n_quic_dc()];

    for event_info in event_paths {
        let mut files = vec![];

        let input_path = event_info.input_path;

        for path in glob::glob(input_path)? {
            let path = path?;
            eprintln!("loading {}", path.canonicalize().unwrap().display());
            let file = std::fs::read_to_string(&path)?;
            files.push(parser::parse(&file, path).unwrap());
        }

        // make sure events are in a deterministic order
        files.sort_by(|a, b| a.path.as_os_str().cmp(b.path.as_os_str()));

        // validate the events
        validation::validate(&files);

        let root = std::path::Path::new(event_info.output_path);
        let _ = std::fs::create_dir_all(root);
        let root = root.canonicalize()?;

        let mut output = Output {
            config: event_info.generate_config,
            s2n_quic_core_path: event_info.s2n_quic_core_path,
            api: event_info.api,
            builders: event_info.builder,
            tracing_subscriber_attr: event_info.tracing_subscriber_attr,
            tracing_subscriber_def: event_info.tracing_subscriber_def,
            feature_alloc: event_info.feature_alloc,
            crate_name: event_info.crate_name,
            root,
            ..Default::default()
        };

        output.generate(&files);
    }

    Ok(())
}
