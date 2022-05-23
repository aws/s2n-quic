// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{Output, Result};
use heck::ToSnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseStream};

#[derive(Debug, Default)]
pub struct File {
    pub structs: Vec<Struct>,
    pub enums: Vec<Enum>,
    pub extra: TokenStream,
}

impl File {
    fn parse(file: syn::File) -> Self {
        assert!(file.attrs.is_empty());
        assert!(file.shebang.is_none());

        let mut out = File::default();
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
    attrs: ContainerAttrs,
    ident: syn::Ident,
    generics: syn::Generics,
    fields: Vec<Field>,
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

    fn to_tokens(&self, output: &mut Output) {
        let Self {
            attrs,
            ident,
            generics,
            fields,
        } = self;

        let derive_attrs = &attrs.derive_attrs;
        let extra_attrs = &attrs.extra;
        let deprecated = &attrs.deprecated;
        let allow_deprecated = &attrs.allow_deprecated;

        let destructure_fields: Vec<_> = fields.iter().map(Field::destructure).collect();
        let builder_fields = fields.iter().map(Field::builder);
        let builder_field_impls = fields.iter().map(Field::builder_impl);
        let api_fields = fields.iter().map(Field::api);

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
            pub struct #ident #generics {
                #(#api_fields)*
            }
        ));

        if let Some(event_name) = attrs.event_name.as_ref() {
            output.api.extend(quote!(
                #allow_deprecated
                impl #generics Event for #ident #generics {
                    const NAME: &'static str = #event_name;
                }
            ));

            let ident_str = ident.to_string();
            let snake = ident_str.to_snake_case();
            let function = format!("on_{}", snake);
            let counter = Ident::new(&snake, Span::call_site());
            let function = Ident::new(&function, Span::call_site());

            let subscriber_doc = format!("Called when the `{}` event is triggered", ident_str);
            let publisher_doc = format!(
                "Publishes a `{}` event to the publisher's subscriber",
                ident_str
            );

            // add a counter for testing structs
            output.testing_fields.extend(quote!(
                pub #counter: u32,
            ));
            output.testing_fields_init.extend(quote!(
                #counter: 0,
            ));

            match attrs.subject {
                Subject::Endpoint => {
                    output.subscriber.extend(quote!(
                        #[doc = #subscriber_doc]
                        #[inline]
                        #deprecated
                        #allow_deprecated
                        fn #function(&mut self, meta: &EndpointMeta, event: &#ident) {
                            let _ = meta;
                            let _ = event;
                        }
                    ));

                    output.tuple_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&mut self, meta: &EndpointMeta, event: &#ident) {
                            (self.0).#function(meta, event);
                            (self.1).#function(meta, event);
                        }
                    ));

                    output.tracing_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&mut self, meta: &api::EndpointMeta, event: &api::#ident) {
                            let parent = match meta.endpoint_type {
                                api::EndpointType::Client {} => {
                                    self.client.id()
                                }
                                api::EndpointType::Server {} => {
                                    self.server.id()
                                }
                            };
                            let api::#ident { #(#destructure_fields),* } = event;
                            tracing::event!(target: #snake, parent: parent, tracing::Level::DEBUG, #(#destructure_fields = tracing::field::debug(#destructure_fields)),*);
                        }
                    ));

                    // endpoint level events
                    if attrs.bpf {
                        // eBPF Rust repr_c structs
                        output.rust_bpf_reprc.extend(quote!(
                            #[repr(C)]
                            #[derive(Debug)]
                            pub(super) struct #ident {
                                #(pub #destructure_fields: u64,) *
                            }

                            impl #generics IntoBpf<#ident> for api::#ident #generics {
                                #[inline]
                                fn as_bpf(&self) -> #ident {
                                    #ident {
                                        #(#destructure_fields: self.#destructure_fields.as_bpf(),) *
                                    }
                                }
                            }
                        ));
                        // eBPF C header file
                        output.c_bpf_reprc.extend(quote!(
                            struct #ident {
                                #(uint64_t #destructure_fields;) *
                            };

                        ));

                        output.bpf_subscriber.extend(quote!(
                            #[inline]
                            #allow_deprecated
                            fn #function(&mut self, _meta: &api::EndpointMeta, event: &api::#ident) {
                                let m = event.as_bpf();
                                let ptr = &m as *const generated_bpf::#ident;
                                probe!(s2n_quic, #function, ptr);
                            }
                        ));
                    } else {
                        output.bpf_subscriber.extend(quote!(
                            #[inline]
                            #allow_deprecated
                            fn #function(&mut self, _meta: &api::EndpointMeta, _event: &api::#ident) {
                                probe!(s2n_quic, #function);
                            }
                        ));
                    }

                    output.endpoint_publisher.extend(quote!(
                        #[doc = #publisher_doc]
                        fn #function(&mut self, event: builder::#ident);
                    ));

                    output.endpoint_publisher_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&mut self, event: builder::#ident) {
                            let event = event.into_event();
                            self.subscriber.#function(&self.meta, &event);
                            self.subscriber.on_event(&self.meta, &event);
                        }
                    ));

                    output.subscriber_testing.extend(quote!(
                        #allow_deprecated
                        fn #function(&mut self, meta: &api::EndpointMeta, event: &api::#ident) {
                            self.#counter += 1;
                            self.output.push(format!("{:?} {:?}", meta, event));
                        }
                    ));

                    output.endpoint_publisher_testing.extend(quote!(
                        #allow_deprecated
                        fn #function(&mut self, event: builder::#ident) {
                            self.#counter += 1;
                            let event = event.into_event();
                            self.output.push(format!("{:?}", event));
                        }
                    ));
                }
                Subject::Connection => {
                    output.subscriber.extend(quote!(
                        #[doc = #subscriber_doc]
                        #[inline]
                        #deprecated
                        #allow_deprecated
                        fn #function(&mut self, context: &mut Self::ConnectionContext, meta: &ConnectionMeta, event: &#ident) {
                            let _ = context;
                            let _ = meta;
                            let _ = event;
                        }
                    ));

                    output.tuple_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&mut self, context: &mut Self::ConnectionContext, meta: &ConnectionMeta, event: &#ident) {
                            (self.0).#function(&mut context.0, meta, event);
                            (self.1).#function(&mut context.1, meta, event);
                        }
                    ));

                    output.tracing_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&mut self, context: &mut Self::ConnectionContext, _meta: &api::ConnectionMeta, event: &api::#ident) {
                            let id = context.id();
                            let api::#ident { #(#destructure_fields),* } = event;
                            tracing::event!(target: #snake, parent: id, tracing::Level::DEBUG, #(#destructure_fields = tracing::field::debug(#destructure_fields)),*);
                        }
                    ));

                    // connection level events
                    if attrs.bpf {
                        // eBPF Rust repr_c structs
                        output.rust_bpf_reprc.extend(quote!(
                            #[repr(C)]
                            #[derive(Debug)]
                            pub(super) struct #ident {
                                #(pub #destructure_fields: u64,) *
                            }

                            impl #generics IntoBpf<#ident> for api::#ident #generics {
                                #[inline]
                                fn as_bpf(&self) -> #ident {
                                    #ident {
                                        #(#destructure_fields: self.#destructure_fields.as_bpf(),) *
                                    }
                                }
                            }
                        ));
                        // eBPF C header file
                        output.c_bpf_reprc.extend(quote!(
                            struct #ident {
                                #(uint64_t #destructure_fields;) *
                            };

                        ));

                        output.bpf_subscriber.extend(quote!(
                            #[inline]
                            #allow_deprecated
                            fn #function(&mut self, _context: &mut Self::ConnectionContext, meta: &api::ConnectionMeta, event: &api::#ident) {
                                let id = meta.id;
                                let m = event.as_bpf();
                                let ptr = &m as *const generated_bpf::#ident;
                                probe!(s2n_quic, #function, id, ptr);
                            }
                        ));
                    } else {
                        output.bpf_subscriber.extend(quote!(
                            #[inline]
                            #allow_deprecated
                            fn #function(&mut self, _context: &mut Self::ConnectionContext, meta: &api::ConnectionMeta, _event: &api::#ident) {
                                let id = meta.id;
                                probe!(s2n_quic, #function, id);
                            }
                        ));
                    }

                    output.connection_publisher.extend(quote!(
                        #[doc = #publisher_doc]
                        fn #function(&mut self, event: builder::#ident);
                    ));

                    output.connection_publisher_subscriber.extend(quote!(
                        #[inline]
                        #allow_deprecated
                        fn #function(&mut self, event: builder::#ident) {
                            let event = event.into_event();
                            self.subscriber.#function(self.context, &self.meta, &event);
                            self.subscriber.on_connection_event(self.context, &self.meta, &event);
                            self.subscriber.on_event(&self.meta, &event);
                        }
                    ));

                    output.subscriber_testing.extend(quote!(
                        #allow_deprecated
                        fn #function(&mut self, _context: &mut Self::ConnectionContext, meta: &api::ConnectionMeta, event: &api::#ident) {
                            self.#counter += 1;
                            if self.location.is_some() {
                                self.output.push(format!("{:?} {:?}", meta, event));
                            }
                        }
                    ));

                    output.connection_publisher_testing.extend(quote!(
                        #allow_deprecated
                        fn #function(&mut self, event: builder::#ident) {
                            self.#counter += 1;
                            let event = event.into_event();
                            if self.location.is_some() {
                                self.output.push(format!("{:?}", event));
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
    attrs: ContainerAttrs,
    ident: syn::Ident,
    generics: syn::Generics,
    variants: Vec<Variant>,
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
        let extra_attrs = &attrs.extra;
        let deprecated = &attrs.deprecated;
        let allow_deprecated = &attrs.allow_deprecated;

        let builder_fields = variants.iter().map(Variant::builder);
        let builder_field_impls = variants.iter().map(Variant::builder_impl);
        let api_fields = variants.iter().map(Variant::api);

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

        output.api.extend(quote!(
            #derive_attrs
            #extra_attrs
            #deprecated
            pub enum #ident #generics {
                #(#api_fields)*
            }
        ));
    }
}

#[derive(Debug)]
struct ContainerAttrs {
    event_name: Option<syn::LitStr>,
    deprecated: TokenStream,
    allow_deprecated: TokenStream,
    subject: Subject,
    exhaustive: bool,
    derive: bool,
    bpf: bool,
    derive_attrs: TokenStream,
    extra: TokenStream,
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
            bpf: false,
            derive_attrs: quote!(),
            extra: quote!(),
        };

        for attr in attrs {
            if attr.path.is_ident("event") {
                v.event_name = Some(attr.parse_args().unwrap());
            } else if attr.path.is_ident("deprecated") {
                attr.to_tokens(&mut v.deprecated);

                if v.allow_deprecated.is_empty() {
                    v.allow_deprecated = quote!(#[allow(deprecated)]);
                }
            } else if attr.path.is_ident("subject") {
                v.subject = attr.parse_args().unwrap();
            } else if attr.path.is_ident("exhaustive") {
                v.exhaustive = true;
            } else if attr.path.is_ident("derive") {
                v.derive = false;
                attr.to_tokens(&mut v.derive_attrs);
            } else if attr.path.is_ident("bpf") {
                v.bpf = true;
            } else {
                attr.to_tokens(&mut v.extra)
            }
        }

        v
    }
}

#[derive(Debug)]
enum Subject {
    Connection,
    Endpoint,
}

impl Parse for Subject {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        let name: Ident = input.parse()?;
        match name.to_string().as_str() {
            "connection" => Ok(Self::Connection),
            "endpoint" => Ok(Self::Endpoint),
            name => Err(syn::parse::Error::new(
                input.span(),
                format!(
                    "invalid event subject: {}, expected connection or endpoint",
                    name
                ),
            )),
        }
    }
}

#[derive(Debug)]
struct Field {
    attrs: FieldAttrs,
    ident: Option<syn::Ident>,
    ty: syn::Type,
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

#[derive(Debug)]
struct FieldAttrs {
    builder: Option<syn::Type>,
    extra: TokenStream,
}

impl FieldAttrs {
    fn parse(attrs: Vec<syn::Attribute>) -> Self {
        let mut v = Self {
            // The event can override the builder with a specific type
            builder: None,
            extra: quote!(),
        };

        for attr in attrs {
            if attr.path.is_ident("builder") {
                v.builder = Some(attr.parse_args().unwrap());
            } else {
                attr.to_tokens(&mut v.extra)
            }
        }

        v
    }
}

#[derive(Debug)]
struct Variant {
    ident: syn::Ident,
    attrs: Vec<syn::Attribute>,
    fields: Vec<Field>,
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

pub fn parse(contents: &str) -> Result<File> {
    let file = syn::parse_str(contents)?;
    let common = File::parse(file);
    Ok(common)
}
