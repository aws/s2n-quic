// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{Output, Result};
use heck::{ToShoutySnakeCase, ToSnakeCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};
use std::path::PathBuf;
use syn::{
    parse::{Parse, ParseStream},
    Meta, Token,
};

pub fn parse(contents: &str, path: PathBuf) -> Result<File> {
    let file = syn::parse_str(contents)?;
    let common = File::parse(file, path);
    Ok(common)
}

#[derive(Debug, Default)]
pub struct File {
    pub structs: Vec<Struct>,
    pub enums: Vec<Enum>,
    pub extra: TokenStream,
    pub path: PathBuf,
}

impl File {
    fn parse(file: syn::File, path: PathBuf) -> Self {
        assert!(file.attrs.is_empty());
        assert!(file.shebang.is_none());

        let mut out = File {
            path,
            ..Default::default()
        };
        for item in file.items {
            match item {
                syn::Item::Enum(v) => out.enums.push(Enum::parse(v)),
                syn::Item::Struct(v) => out.structs.push(Struct::parse(v)),
                item => item.to_tokens(&mut out.extra),
            }
        }

        out
    }

    pub(crate) fn to_tokens(&self, output: &mut Output) {
        self.extra.to_tokens(&mut output.extra);
        for v in &self.structs {
            v.to_tokens(output);
        }
        for v in &self.enums {
            v.to_tokens(output);
        }
    }
}

#[derive(Debug)]
pub struct Struct {
    pub attrs: ContainerAttrs,
    pub ident: syn::Ident,
    pub generics: syn::Generics,
    pub fields: Vec<Field>,
}

impl Struct {
    fn parse(item: syn::ItemStruct) -> Self {
        let attrs = ContainerAttrs::parse(item.attrs);

        Self {
            attrs,
            ident: item.ident,
            generics: item.generics,
            fields: item.fields.into_iter().map(Field::parse).collect(),
        }
    }

    pub fn ident_str(&self) -> String {
        self.ident.to_string()
    }

    pub fn ident_snake(&self) -> String {
        self.ident_str().to_snake_case()
    }

    pub fn function_name(&self) -> String {
        format!("on_{}", self.ident_snake())
    }

    pub fn function(&self) -> Ident {
        Ident::new(&self.function_name(), Span::call_site())
    }

    pub fn counter(&self) -> Ident {
        Ident::new(&self.ident_snake(), Span::call_site())
    }

    fn to_tokens(&self, output: &mut Output) {
        let Self {
            attrs,
            ident,
            generics,
            fields,
        } = self;
        let ident_str = ident.to_string();

        let derive_attrs = &attrs.derive_attrs;
        let builder_derive_attrs = &attrs.builder_derive_attrs;
        let extra_attrs = &attrs.extra;
        let deprecated = &attrs.deprecated;
        let allow_deprecated = &attrs.allow_deprecated;

        let destructure_fields: Vec<_> = fields.iter().map(Field::destructure).collect();
        let builder_fields = fields.iter().map(Field::builder);
        let builder_field_impls = fields.iter().map(Field::builder_impl);
        let api_fields = fields.iter().map(Field::api);
        let snapshot_fields = fields.iter().map(Field::snapshot);

        if attrs.builder_derive {
            output.builders.extend(quote!(
                #[#builder_derive_attrs]
            ));
        }

        output.builders.extend(quote!(
            #[derive(Clone, Debug)]
            #extra_attrs
            pub struct #ident #generics {
                #(#builder_fields)*
            }

            #allow_deprecated
            impl #generics IntoEvent<api::#ident #generics> for #ident #generics {
                #[inline]
                fn into_event(self) -> api::#ident #generics {
                    let #ident {
                        #(#destructure_fields),*
                    } = self;

                    api::#ident {
                        #(#builder_field_impls)*
                    }
                }
            }
        ));

        if attrs.derive {
            output.api.extend(quote!(#[derive(Clone, Debug)]));
        }

        if !attrs.exhaustive {
            output.api.extend(quote!(#[non_exhaustive]));
        }

        output.api.extend(quote!(
            #derive_attrs
            #extra_attrs
            #deprecated
            #allow_deprecated
            pub struct #ident #generics {
                #(#api_fields)*
            }

            #[cfg(any(test, feature = "testing"))]
            #allow_deprecated
            impl #generics crate::event::snapshot::Fmt for #ident #generics {
                fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
                    let mut fmt = fmt.debug_struct(#ident_str);
                    #(#snapshot_fields)*
                    fmt.finish()
                }
            }
        ));

        if let Some(event_name) = attrs.event_name.as_ref() {
            output.api.extend(quote!(
                #allow_deprecated
                impl #generics Event for #ident #generics {
                    const NAME: &'static str = #event_name;
                }
            ));

            let ident_str = self.ident_str();
            let snake = self.ident_snake();
            let counter = self.counter();
            let function = self.function();

            let subscriber_doc = format!("Called when the `{ident_str}` event is triggered");
            let publisher_doc =
                format!("Publishes a `{ident_str}` event to the publisher's subscriber");

            let counter_type = output.mode.counter_type();
            let counter_init = output.mode.counter_init();

            // add a counter for testing structs
            output.testing_fields.extend(quote!(
                pub #counter: #counter_type,
            ));
            output.testing_fields_init.extend(quote!(
                #counter: #counter_init,
            ));

            let receiver = output.mode.receiver();
            let counter_increment = output.mode.counter_increment();
            let lock = output.mode.lock();

            match attrs.subject {
                Subject::Endpoint => {
                    output.subscriber.extend(quote!(
                        #[doc = #subscriber_doc]
                        #[inline]
                        #deprecated
                        #allow_deprecated
                        fn #function(&#receiver self, meta: &api::EndpointMeta, event: &api::#ident) {
                            let _ = meta;
                            let _ = event;
                        }
                    ));

                    output.tuple_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&#receiver self, meta: &api::EndpointMeta, event: &api::#ident) {
                            (self.0).#function(meta, event);
                            (self.1).#function(meta, event);
                        }
                    ));

                    if output.mode.is_ref() {
                        output.ref_subscriber.extend(quote!(
                            #[inline]
                            #allow_deprecated
                            fn #function(&#receiver self, meta: &api::EndpointMeta, event: &api::#ident) {
                                self.as_ref().#function(meta, event);
                            }
                        ));
                    }

                    output.tracing_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&#receiver self, meta: &api::EndpointMeta, event: &api::#ident) {
                            let parent = self.parent(meta);
                            let api::#ident { #(#destructure_fields),* } = event;
                            tracing::event!(target: #snake, parent: parent, tracing::Level::DEBUG, { #(#destructure_fields = tracing::field::debug(#destructure_fields)),* });
                        }
                    ));

                    output.endpoint_publisher.extend(quote!(
                        #[doc = #publisher_doc]
                        fn #function(&#receiver self, event: builder::#ident);
                    ));

                    output.endpoint_publisher_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&#receiver self, event: builder::#ident) {
                            let event = event.into_event();
                            self.subscriber.#function(&self.meta, &event);
                            self.subscriber.on_event(&self.meta, &event);
                        }
                    ));

                    for subscriber in [
                        &mut output.endpoint_subscriber_testing,
                        &mut output.subscriber_testing,
                    ] {
                        subscriber.extend(quote!(
                            #allow_deprecated
                            fn #function(&#receiver self, meta: &api::EndpointMeta, event: &api::#ident) {
                                self.#counter #counter_increment;
                                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                                let out = format!("{meta:?} {event:?}");
                                self.output #lock.push(out);
                            }
                        ));
                    }

                    // add a counter for testing structs
                    output.endpoint_testing_fields.extend(quote!(
                        pub #counter: #counter_type,
                    ));
                    output.endpoint_testing_fields_init.extend(quote!(
                        #counter: #counter_init,
                    ));

                    output.endpoint_publisher_testing.extend(quote!(
                        #allow_deprecated
                        fn #function(&#receiver self, event: builder::#ident) {
                            self.#counter #counter_increment;
                            let event = event.into_event();
                            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                            let out = format!("{event:?}");
                            self.output #lock.push(out);
                        }
                    ));
                }
                Subject::Connection => {
                    output.subscriber.extend(quote!(
                        #[doc = #subscriber_doc]
                        #[inline]
                        #deprecated
                        #allow_deprecated
                        fn #function(
                            &#receiver self,
                            context: &#receiver Self::ConnectionContext,
                            meta: &api::ConnectionMeta,
                            event: &api::#ident
                        ) {
                            let _ = context;
                            let _ = meta;
                            let _ = event;
                        }
                    ));

                    output.tuple_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(
                            &#receiver self,
                            context: &#receiver Self::ConnectionContext,
                            meta: &api::ConnectionMeta,
                            event: &api::#ident
                        ) {
                            (self.0).#function(&#receiver context.0, meta, event);
                            (self.1).#function(&#receiver context.1, meta, event);
                        }
                    ));

                    if output.mode.is_ref() {
                        output.ref_subscriber.extend(quote!(
                            #[inline]
                            #allow_deprecated
                            fn #function(
                                &#receiver self,
                                context: &#receiver Self::ConnectionContext,
                                meta: &api::ConnectionMeta,
                                event: &api::#ident
                            ) {
                                self.as_ref().#function(context, meta, event);
                            }
                        ));
                    }

                    output.tracing_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(
                            &#receiver self,
                            context: &#receiver Self::ConnectionContext,
                            _meta: &api::ConnectionMeta,
                            event: &api::#ident
                        ) {
                            let id = context.id();
                            let api::#ident { #(#destructure_fields),* } = event;
                            tracing::event!(target: #snake, parent: id, tracing::Level::DEBUG, { #(#destructure_fields = tracing::field::debug(#destructure_fields)),* });
                        }
                    ));

                    output.connection_publisher.extend(quote!(
                        #[doc = #publisher_doc]
                        fn #function(&#receiver self, event: builder::#ident);
                    ));

                    output.connection_publisher_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&#receiver self, event: builder::#ident) {
                            let event = event.into_event();
                            self.subscriber.#function(self.context, &self.meta, &event);
                            self.subscriber.on_connection_event(self.context, &self.meta, &event);
                            self.subscriber.on_event(&self.meta, &event);
                        }
                    ));

                    output.subscriber_testing.extend(quote!(
                        #allow_deprecated
                        fn #function(
                            &#receiver self,
                            _context: &#receiver Self::ConnectionContext,
                            meta: &api::ConnectionMeta,
                            event: &api::#ident
                        ) {
                            self.#counter #counter_increment;
                            if self.location.is_some() {
                                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                                let out = format!("{meta:?} {event:?}");
                                self.output #lock.push(out);
                            }
                        }
                    ));

                    output.connection_publisher_testing.extend(quote!(
                        #allow_deprecated
                        fn #function(&#receiver self, event: builder::#ident) {
                            self.#counter #counter_increment;
                            let event = event.into_event();
                            if self.location.is_some() {
                                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                                let out = format!("{event:?}");
                                self.output #lock.push(out);
                            }
                        }
                    ));
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct Enum {
    pub attrs: ContainerAttrs,
    pub ident: syn::Ident,
    pub generics: syn::Generics,
    pub variants: Vec<Variant>,
}

impl Enum {
    fn parse(item: syn::ItemEnum) -> Self {
        let attrs = ContainerAttrs::parse(item.attrs);

        Self {
            attrs,
            ident: item.ident,
            generics: item.generics,
            variants: item.variants.into_iter().map(Variant::parse).collect(),
        }
    }

    fn to_tokens(&self, output: &mut Output) {
        let Self {
            attrs,
            ident,
            generics,
            variants,
        } = self;

        assert!(
            attrs.event_name.is_none(),
            "enum events are not currently supported"
        );

        let derive_attrs = &attrs.derive_attrs;
        let builder_derive_attrs = &attrs.builder_derive_attrs;
        let extra_attrs = &attrs.extra;
        let deprecated = &attrs.deprecated;
        let allow_deprecated = &attrs.allow_deprecated;

        let builder_fields = variants.iter().map(Variant::builder);
        let builder_field_impls = variants.iter().map(Variant::builder_impl);
        let api_fields = variants.iter().map(Variant::api);

        if attrs.builder_derive {
            output.builders.extend(quote!(
                #[#builder_derive_attrs]
            ));
        }

        output.builders.extend(quote!(
            #[derive(Clone, Debug)]
            #extra_attrs
            pub enum #ident #generics {
                #(#builder_fields)*
            }

            #allow_deprecated
            impl #generics IntoEvent<api::#ident #generics> for #ident #generics {
                #[inline]
                fn into_event(self) -> api::#ident #generics {
                    use api::#ident::*;
                    match self {
                        #(#builder_field_impls)*
                    }
                }
            }
        ));

        if attrs.derive {
            output.api.extend(quote!(#[derive(Clone, Debug)]));
        }

        if !attrs.exhaustive {
            output.api.extend(quote!(#[non_exhaustive]));
        }

        let mut variant_defs = quote!();
        let mut variant_matches = quote!();

        for (idx, variant) in variants.iter().enumerate() {
            let ident = &variant.ident;
            let name = ident.to_string();
            let mut name = name.to_shouty_snake_case();
            name.push('\0');

            variant_defs.extend(quote!(aggregate::info::variant::Builder {
                name: aggregate::info::Str::new(#name),
                id: #idx,
            }.build(),));

            variant_matches.extend(quote!(
                Self::#ident { .. } => #idx,
            ));
        }

        output.api.extend(quote!(
            #derive_attrs
            #extra_attrs
            #deprecated
            pub enum #ident #generics {
                #(#api_fields)*
            }

            #allow_deprecated
            impl #generics aggregate::AsVariant for #ident #generics {
                const VARIANTS: &'static [aggregate::info::Variant] = &[#variant_defs];

                #[inline]
                fn variant_idx(&self) -> usize {
                    match self {
                        #variant_matches
                    }
                }
            }
        ));
    }
}

#[derive(Debug)]
pub struct ContainerAttrs {
    pub event_name: Option<syn::LitStr>,
    pub deprecated: TokenStream,
    pub allow_deprecated: TokenStream,
    pub subject: Subject,
    pub exhaustive: bool,
    pub derive: bool,
    pub derive_attrs: TokenStream,
    pub builder_derive: bool,
    pub builder_derive_attrs: TokenStream,
    pub checkpoint: Vec<Checkpoint>,
    pub measure_counter: Vec<Metric>,
    pub extra: TokenStream,
}

impl ContainerAttrs {
    fn parse(attrs: Vec<syn::Attribute>) -> Self {
        let mut v = Self {
            // events must include a name to be considered an event
            event_name: None,
            deprecated: TokenStream::default(),
            allow_deprecated: TokenStream::default(),
            // most event subjects relate to actual connections so make that the default
            subject: Subject::Connection,
            // default to #[non_exhaustive]
            exhaustive: false,
            derive: true,
            derive_attrs: quote!(),
            builder_derive: false,
            builder_derive_attrs: quote!(),
            checkpoint: vec![],
            measure_counter: vec![],
            extra: quote!(),
        };

        for attr in attrs {
            let path = attr.path();
            if path.is_ident("event") {
                v.event_name = Some(attr.parse_args().unwrap());
            } else if path.is_ident("deprecated") {
                attr.to_tokens(&mut v.deprecated);

                if v.allow_deprecated.is_empty() {
                    v.allow_deprecated = quote!(#[allow(deprecated)]);
                }
            } else if path.is_ident("subject") {
                v.subject = attr.parse_args().unwrap();
            } else if path.is_ident("exhaustive") {
                v.exhaustive = true;
            } else if path.is_ident("derive") {
                v.derive = false;
                attr.to_tokens(&mut v.derive_attrs);
            } else if path.is_ident("builder_derive") {
                v.builder_derive = true;
                if let Meta::List(list) = attr.parse_args().unwrap() {
                    list.to_tokens(&mut v.builder_derive_attrs);
                }
            } else if path.is_ident("checkpoint") {
                v.checkpoint.push(attr.parse_args().unwrap());
            } else if path.is_ident("measure_counter") {
                v.measure_counter.push(attr.parse_args().unwrap());
            } else {
                attr.to_tokens(&mut v.extra)
            }
        }

        if !(v.checkpoint.is_empty() && v.measure_counter.is_empty()) {
            assert_eq!(v.subject, Subject::Connection);
        }

        v
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Subject {
    Connection,
    Endpoint,
}

impl Subject {
    #[allow(dead_code)]
    pub fn is_connection(&self) -> bool {
        matches!(self, Self::Connection)
    }

    #[allow(dead_code)]
    pub fn is_endpoint(&self) -> bool {
        matches!(self, Self::Endpoint)
    }
}

impl Parse for Subject {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        let name: Ident = input.parse()?;
        match name.to_string().as_str() {
            "connection" => Ok(Self::Connection),
            "endpoint" => Ok(Self::Endpoint),
            name => Err(syn::parse::Error::new(
                input.span(),
                format!("invalid event subject: {name}, expected connection or endpoint"),
            )),
        }
    }
}

#[derive(Debug)]
pub struct Field {
    pub attrs: FieldAttrs,
    pub ident: Option<syn::Ident>,
    pub ty: syn::Type,
}

impl Field {
    fn parse(item: syn::Field) -> Self {
        Self {
            attrs: FieldAttrs::parse(item.attrs),
            ident: item.ident,
            ty: item.ty,
        }
    }

    fn api(&self) -> TokenStream {
        let Self { attrs, ident, ty } = self;
        let attrs = &attrs.extra;
        if let Some(name) = ident {
            quote!(
                #attrs
                pub #name: #ty,
            )
        } else {
            quote!(#attrs pub #ty,)
        }
    }

    fn enum_api(&self) -> TokenStream {
        let Self {
            attrs, ident, ty, ..
        } = self;
        let attrs = &attrs.extra;
        if let Some(name) = ident {
            quote!(
                #attrs
                #name: #ty,
            )
        } else {
            quote!(#attrs #ty,)
        }
    }

    fn destructure(&self) -> TokenStream {
        let Self { ident, .. } = self;
        quote!(#ident)
    }

    fn builder(&self) -> TokenStream {
        let Self { attrs, ident, .. } = self;
        let attrs = &attrs.extra;
        let ty = self.builder_type();
        if let Some(name) = ident {
            quote!(#attrs pub #name: #ty,)
        } else {
            quote!(#attrs pub #ty,)
        }
    }

    fn enum_builder(&self) -> TokenStream {
        let Self { attrs, ident, .. } = self;
        let attrs = &attrs.extra;
        let ty = self.builder_type();
        if let Some(name) = ident {
            quote!(#attrs #name: #ty,)
        } else {
            quote!(#attrs #ty,)
        }
    }

    fn snapshot(&self) -> TokenStream {
        let Self { attrs, ident, .. } = self;
        let ident = ident.as_ref().expect("all events should have field names");
        let ident_str = ident.to_string();
        if let Some(expr) = attrs.snapshot.as_ref() {
            quote!(fmt.field(#ident_str, &#expr);)
        } else {
            quote!(fmt.field(#ident_str, &self.#ident);)
        }
    }

    fn builder_impl(&self) -> TokenStream {
        let Self { ident, .. } = self;
        quote!(#ident: #ident.into_event(),)
    }

    fn builder_type(&self) -> &syn::Type {
        if let Some(ty) = &self.attrs.builder {
            ty
        } else {
            &self.ty
        }
    }
}

#[derive(Debug, Default)]
pub struct FieldAttrs {
    pub builder: Option<syn::Type>,
    pub snapshot: Option<syn::Expr>,
    pub counter: Vec<Metric>,
    pub measure_counter: Vec<Metric>,
    pub bool_counter: Vec<MetricNoUnit>,
    pub nominal_counter: Vec<Metric>,
    pub nominal_checkpoint: Vec<MetricNoUnit>,
    pub measure: Vec<Metric>,
    pub gauge: Vec<Metric>,
    pub timer: Vec<MetricNoUnit>,
    pub extra: TokenStream,
}

impl FieldAttrs {
    fn parse(attrs: Vec<syn::Attribute>) -> Self {
        let mut v = Self::default();

        for attr in attrs {
            macro_rules! field {
                ($name:ident) => {
                    if attr.path().is_ident(stringify!($name)) {
                        v.$name = Some(attr.parse_args().unwrap_or_else(|err| {
                            panic!("{err} in {:?}", attr.into_token_stream().to_string())
                        }));
                        continue;
                    }
                };
                ($name:ident[]) => {
                    if attr.path().is_ident(stringify!($name)) {
                        v.$name.push(attr.parse_args().unwrap_or_else(|err| {
                            panic!("{err} in {:?}", attr.into_token_stream().to_string())
                        }));
                        continue;
                    }
                };
            }

            field!(builder);
            field!(snapshot);
            field!(counter[]);
            field!(measure_counter[]);
            field!(bool_counter[]);
            field!(nominal_counter[]);
            field!(nominal_checkpoint[]);
            field!(measure[]);
            field!(gauge[]);
            field!(timer[]);

            attr.to_tokens(&mut v.extra);
        }

        v
    }
}

#[derive(Debug)]
pub struct Variant {
    pub ident: syn::Ident,
    pub attrs: Vec<syn::Attribute>,
    pub fields: Vec<Field>,
}

impl Variant {
    fn parse(item: syn::Variant) -> Self {
        Self {
            ident: item.ident,
            attrs: item.attrs,
            fields: item.fields.into_iter().map(Field::parse).collect(),
        }
    }

    fn api(&self) -> TokenStream {
        let Self {
            ident,
            attrs,
            fields,
        } = self;
        let fields = fields.iter().map(Field::enum_api);
        quote!(
            #[non_exhaustive]
            #(#attrs)*
            #ident { #(#fields)* },
        )
    }

    fn builder(&self) -> TokenStream {
        let Self {
            ident,
            fields,
            attrs,
        } = self;
        if fields.is_empty() {
            quote!(#(#attrs)* #ident,)
        } else {
            let fields = fields.iter().map(Field::enum_builder);
            quote!(#(#attrs)* #ident { #(#fields)* },)
        }
    }

    fn builder_impl(&self) -> TokenStream {
        let Self { ident, fields, .. } = self;
        if fields.is_empty() {
            quote!(Self::#ident => #ident { },)
        } else {
            let destructure = fields.iter().map(Field::destructure);
            let fields = fields.iter().map(Field::builder_impl);

            quote!(Self::#ident { #(#destructure),* } => #ident { #(#fields)* },)
        }
    }
}

#[derive(Debug)]
pub struct Metric {
    pub name: syn::LitStr,
    pub unit: Option<syn::Ident>,
}

impl syn::parse::Parse for Metric {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        let unit = if input.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
            let unit = input.parse()?;
            Some(unit)
        } else {
            None
        };
        let _: syn::parse::Nothing = input.parse()?;
        Ok(Self { name, unit })
    }
}

#[derive(Debug)]
pub struct MetricNoUnit {
    pub name: syn::LitStr,
}

impl syn::parse::Parse for MetricNoUnit {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        let _: syn::parse::Nothing = input.parse()?;
        Ok(Self { name })
    }
}

#[derive(Debug)]
pub struct Checkpoint {
    pub name: syn::LitStr,
    pub condition: Option<syn::ExprClosure>,
}

impl syn::parse::Parse for Checkpoint {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        let condition = if input.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
            let v = input.parse()?;
            Some(v)
        } else {
            None
        };
        let _: syn::parse::Nothing = input.parse()?;
        Ok(Self { name, condition })
    }
}
