// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T, E = Error> = core::result::Result<T, E>;

mod output;
mod output_mode;
mod parser;

use output::Output;
use output_mode::OutputMode;

struct EventInfo<'a> {
    input_path: &'a str,
    output_path: &'a str,
    output_mode: OutputMode,
    s2n_quic_core_path: TokenStream,
    api: TokenStream,
    builder: TokenStream,
    tracing_subscriber_attr: TokenStream,
    tracing_subscriber_def: TokenStream,
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
            input_path: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../s2n-quic-core/events/**/*.rs"
            ),
            output_path: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../s2n-quic-core/src/event/generated.rs"
            ),
            output_mode: OutputMode::Mut,
            s2n_quic_core_path: quote!(crate),
            api: quote!(),
            builder: quote!(),
            tracing_subscriber_attr: quote! {
                #[cfg(feature = "event-tracing")]
            },
            tracing_subscriber_def,
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
            input_path: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../dc/s2n-quic-dc/events/**/*.rs"
            ),
            output_path: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../dc/s2n-quic-dc/src/event/generated.rs"
            ),
            output_mode: OutputMode::Ref,
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
            let file = std::fs::read_to_string(path)?;
            files.push(parser::parse(&file).unwrap());
        }

        let mut output = Output {
            mode: event_info.output_mode,
            s2n_quic_core_path: event_info.s2n_quic_core_path,
            api: event_info.api,
            builders: event_info.builder,
            tracing_subscriber_attr: event_info.tracing_subscriber_attr,
            tracing_subscriber_def: event_info.tracing_subscriber_def,
            ..Default::default()
        };

        for file in &files {
            file.to_tokens(&mut output);
        }

        let generated = std::path::Path::new(event_info.output_path)
            .canonicalize()
            .unwrap();

        let mut o = std::fs::File::create(&generated)?;

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
            .arg(&generated)
            .spawn()?
            .wait()?;

        assert!(status.success());

        eprintln!("  wrote {}", generated.display());
    }

    Ok(())
}
