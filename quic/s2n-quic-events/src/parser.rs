// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{Output, Result};
use heck::SnakeCase;
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

        let extra_attrs = &attrs.extra;

        let destructure_fields = fields.iter().map(Field::destructure);
        let builder_fields = fields.iter().map(Field::builder);
        let builder_field_impls = fields.iter().map(Field::builder_impl);
        let api_fields = fields.iter().map(Field::api);

        output.builders.extend(quote!(
            #[derive(Clone, Debug)]
            #extra_attrs
            pub struct #ident #generics {
                #(#builder_fields)*
            }

            impl #generics IntoEvent<api::#ident #generics> for #ident #generics {
                #[inline]
                fn into_event(self) -> api::#ident #generics {
                    let #ident {
                        #(#destructure_fields)*
                    } = self;

                    api::#ident {
                        #(#builder_field_impls)*
                    }
                }
            }
        ));

        output.api.extend(quote!(
            #[derive(Clone, Debug)]
            #[non_exhaustive]
            #extra_attrs
            pub struct #ident #generics {
                #(#api_fields)*
            }
        ));

        if let Some(event_name) = attrs.event_name.as_ref() {
            output.api.extend(quote!(
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

            match attrs.subject {
                Subject::Endpoint => {
                    output.subscriber.extend(quote!(
                        #[doc = #subscriber_doc]
                        #[inline]
                        fn #function(&mut self, meta: &Meta, event: &#ident) {
                            let _ = meta;
                            let _ = event;
                        }
                    ));

                    output.tuple_subscriber.extend(quote!(
                        #[inline]
                        fn #function(&mut self, meta: &Meta, event: &#ident) {
                            (self.0).#function(meta, event);
                            (self.1).#function(meta, event);
                        }
                    ));

                    output.endpoint_publisher.extend(quote!(
                        #[doc = #publisher_doc]
                        fn #function(&mut self, event: builder::#ident);
                    ));

                    output.endpoint_publisher_subscriber.extend(quote!(
                        #[inline]
                        fn #function(&mut self, event: builder::#ident) {
                            let event = event.into_event();
                            self.subscriber.#function(&self.meta, &event);
                            self.subscriber.on_event(&self.meta, &event);
                        }
                    ));

                    output.subscriber_testing.extend(quote!(
                        fn #function(&mut self, _meta: &api::Meta, _event: &api::#ident) {
                            self.#counter += 1;
                        }
                    ));

                    output.endpoint_publisher_testing.extend(quote!(
                        fn #function(&mut self, _event: builder::#ident) {
                            self.#counter += 1;
                        }
                    ));
                }
                Subject::Connection => {
                    output.subscriber.extend(quote!(
                        #[doc = #subscriber_doc]
                        #[inline]
                        fn #function(&mut self, context: &mut Self::ConnectionContext, meta: &Meta, event: &#ident) {
                            let _ = context;
                            let _ = meta;
                            let _ = event;
                        }
                    ));

                    output.tuple_subscriber.extend(quote!(
                        #[inline]
                        fn #function(&mut self, context: &mut Self::ConnectionContext, meta: &Meta, event: &#ident) {
                            (self.0).#function(&mut context.0, meta, event);
                            (self.1).#function(&mut context.1, meta, event);
                        }
                    ));

                    output.connection_publisher.extend(quote!(
                        #[doc = #publisher_doc]
                        fn #function(&mut self, event: builder::#ident);
                    ));

                    output.connection_publisher_subscriber.extend(quote!(
                        #[inline]
                        fn #function(&mut self, event: builder::#ident) {
                            let event = event.into_event();
                            self.subscriber.#function(&mut self.context, &self.meta, &event);
                            self.subscriber.on_connection_event(&mut self.context, &self.meta, &event);
                            self.subscriber.on_event(&self.meta, &event);
                        }
                    ));

                    output.subscriber_testing.extend(quote!(
                        fn #function(&mut self, _context: &mut Self::ConnectionContext, _meta: &api::Meta, _event: &api::#ident) {
                            self.#counter += 1;
                        }
                    ));

                    output.connection_publisher_testing.extend(quote!(
                        fn #function(&mut self, _event: builder::#ident) {
                            self.#counter += 1;
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

        let extra_attrs = &attrs.extra;

        let builder_fields = variants.iter().map(Variant::builder);
        let builder_field_impls = variants.iter().map(Variant::builder_impl);
        let api_fields = variants.iter().map(Variant::api);

        output.builders.extend(quote!(
            #[derive(Clone, Debug)]
            #extra_attrs
            pub enum #ident #generics {
                #(#builder_fields)*
            }

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

        output.api.extend(quote!(
            #[derive(Clone, Debug)]
            #[non_exhaustive]
            #extra_attrs
            pub enum #ident #generics {
                #(#api_fields)*
            }
        ));
    }
}

#[derive(Debug)]
struct ContainerAttrs {
    event_name: Option<syn::LitStr>,
    subject: Subject,
    exhaustive: bool,
    extra: TokenStream,
}

impl ContainerAttrs {
    fn parse(attrs: Vec<syn::Attribute>) -> Self {
        let mut v = Self {
            // events must include a name to be considered an event
            event_name: None,
            // most event subjects relate to actual connections so make that the default
            // subject: Subject::Connection,
            subject: Subject::Endpoint, // TODO make connection the default when we get context implemented
            // default to #[non_exhaustive]
            exhaustive: false,
            extra: quote!(),
        };

        for attr in attrs {
            if attr.path.is_ident("event") {
                v.event_name = Some(attr.parse_args().unwrap());
            } else if attr.path.is_ident("subject") {
                v.subject = attr.parse_args().unwrap();
            } else if attr.path.is_ident("exhaustive") {
                v.exhaustive = true;
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
        let name: syn::LitStr = input.parse()?;
        match name.value().as_str() {
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
        let Self { ident, ty, .. } = self;
        if let Some(name) = ident {
            quote!(
                pub #name: #ty,
            )
        } else {
            quote!(pub #ty,)
        }
    }

    fn enum_api(&self) -> TokenStream {
        let Self { ident, ty, .. } = self;
        if let Some(name) = ident {
            quote!(
                #name: #ty,
            )
        } else {
            quote!(#ty,)
        }
    }

    fn destructure(&self) -> TokenStream {
        let Self { ident, .. } = self;
        quote!(#ident, )
    }

    fn builder(&self) -> TokenStream {
        let Self { ident, .. } = self;
        let ty = self.builder_type();
        if let Some(name) = ident {
            quote!(pub #name: #ty,)
        } else {
            quote!(pub #ty,)
        }
    }

    fn enum_builder(&self) -> TokenStream {
        let Self { ident, .. } = self;
        let ty = self.builder_type();
        if let Some(name) = ident {
            quote!(#name: #ty,)
        } else {
            quote!(#ty,)
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
    // TODO attributes
    ident: syn::Ident,
    fields: Vec<Field>,
}

impl Variant {
    fn parse(item: syn::Variant) -> Self {
        Self {
            ident: item.ident,
            fields: item.fields.into_iter().map(Field::parse).collect(),
        }
    }

    fn api(&self) -> TokenStream {
        let Self { ident, fields } = self;
        let fields = fields.iter().map(Field::enum_api);
        quote!(
            #[non_exhaustive]
            #ident { #(#fields)* },
        )
    }

    fn builder(&self) -> TokenStream {
        let Self { ident, fields } = self;
        if fields.is_empty() {
            quote!(#ident,)
        } else {
            let fields = fields.iter().map(Field::enum_builder);
            quote!(#ident { #(#fields)* },)
        }
    }

    fn builder_impl(&self) -> TokenStream {
        let Self { ident, fields } = self;
        if fields.is_empty() {
            quote!(Self::#ident => #ident { },)
        } else {
            let destructure = fields.iter().map(Field::destructure);
            let fields = fields.iter().map(Field::builder_impl);

            quote!(Self::#ident { #(#destructure)* } => #ident { #(#fields)* },)
        }
    }
}

pub fn parse(contents: &str) -> Result<File> {
    let file = syn::parse_str(contents)?;
    let common = File::parse(file);
    Ok(common)
}
