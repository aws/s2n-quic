// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::hash::Hasher;
use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::{quote, quote_spanned, ToTokens};
use syn::{
    parse::{Parse, ParseBuffer},
    parse_macro_input, parse_quote,
    spanned::Spanned,
    Expr, Item, Token,
};

const VERSION: u32 = 0;

mod kw {
    syn::custom_keyword!(source);
    syn::custom_keyword!(level);
    syn::custom_keyword!(format);
    syn::custom_keyword!(DEFAULT);
    syn::custom_keyword!(AUTO);
    syn::custom_keyword!(MUST);
    syn::custom_keyword!(SHOULD);
    syn::custom_keyword!(MAY);
    syn::custom_keyword!(IETF);
}

macro_rules! mac {
    ($name:ident, $annotation:expr) => {
        #[proc_macro]
        pub fn $name(input: TokenStream) -> TokenStream {
            let spec: SpecRef = parse_macro_input!(input);
            let ident = spec.gensyn();
            let item: Item = parse_quote!(const #ident: () = (););
            annotation(spec, item, quote!($annotation)).into()
        }
    };
}

mac!(citation, b"CITATION");
mac!(specification, b"SPEC");
mac!(exception, b"EXCEPTION");

macro_rules! attr {
    ($name:ident, $annotation:expr) => {
        #[proc_macro_attribute]
        pub fn $name(attr: TokenStream, input: TokenStream) -> TokenStream {
            let spec: SpecRef = parse_macro_input!(attr);
            let item: Item = parse_macro_input!(input);
            annotation(spec, item, quote!($annotation)).into()
        }
    };
}

attr!(tests, b"TEST");
attr!(implements, b"CITATION");
attr!(excludes, b"EXCEPTION");

struct SpecRef {
    quote: String,
    source: SpecSource,
    level: Option<SpecLevel>,
    format: Option<SpecFormat>,
}

impl SpecRef {
    fn gensyn(&self) -> Ident {
        let id = fnv(&std::time::Instant::now());
        Ident::new(&format!("__COMPIANCE_{}", id), self.source.0.span())
    }
}

impl Parse for SpecRef {
    fn parse(buffer: &ParseBuffer) -> Result<Self, syn::Error> {
        let span = buffer.span();
        let mut quote = String::new();

        if let Ok(attrs) = syn::Attribute::parse_outer(buffer) {
            for attr in &attrs {
                let doc = get_doc(&attr)?;
                quote.push_str(doc.trim());
                quote.push(' ');
            }
        }

        if !quote.is_empty() {
            quote.pop();
        }

        let mut source = None;
        let mut level = None;
        let mut format = None;

        macro_rules! dispatch {
            ($buf:ident, $else_c:expr) => {
                if $buf.peek(Token![,]) {
                    let _: Token![,] = buffer.parse()?;
                } else if $buf.peek(kw::source) {
                    source = Some(buffer.parse()?);
                } else if $buf.peek(kw::level) {
                    level = Some(buffer.parse()?);
                } else if $buf.peek(kw::format) {
                    format = Some(buffer.parse()?);
                } else {
                    $else_c
                }
            };
        }

        dispatch!(buffer, source = Some(buffer.parse()?));

        while !buffer.is_empty() {
            let lookahead = buffer.lookahead1();
            dispatch!(lookahead, return Err(lookahead.error()));
        }

        let source = if let Some(source) = source {
            source
        } else {
            return Err(syn::Error::new(span, "missing 'source' field"));
        };

        Ok(Self {
            source,
            level,
            format,
            quote,
        })
    }
}

struct SpecSource(Expr);

impl Parse for SpecSource {
    fn parse(buffer: &ParseBuffer) -> Result<Self, syn::Error> {
        if buffer.peek(kw::source) {
            let _source: kw::source = buffer.parse()?;
            let _eq_token: Token![=] = buffer.parse()?;
        }
        let value = buffer.parse()?;
        Ok(Self(value))
    }
}

impl ToTokens for SpecSource {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        StrAsBytes(&self.0).to_tokens(tokens)
    }
}

enum SpecLevel {
    DEFAULT(kw::DEFAULT),
    MUST(kw::MUST),
    SHOULD(kw::SHOULD),
    MAY(kw::MAY),
}

impl Parse for SpecLevel {
    fn parse(buffer: &ParseBuffer) -> Result<Self, syn::Error> {
        let _level_token: kw::level = buffer.parse()?;
        let _eq_token: Token![=] = buffer.parse()?;

        let l = buffer.lookahead1();
        if l.peek(kw::MUST) {
            Ok(Self::MUST(buffer.parse()?))
        } else if l.peek(kw::SHOULD) {
            Ok(Self::SHOULD(buffer.parse()?))
        } else if l.peek(kw::MAY) {
            Ok(Self::MAY(buffer.parse()?))
        } else if l.peek(kw::DEFAULT) {
            Ok(Self::DEFAULT(buffer.parse()?))
        } else {
            Err(l.error())
        }
    }
}

impl ToTokens for SpecLevel {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        tokens.extend(match self {
            Self::DEFAULT(_) => quote!(b"DEFAULT"),
            Self::MUST(_) => quote!(b"MUST"),
            Self::SHOULD(_) => quote!(b"SHOULD"),
            Self::MAY(_) => quote!(b"MAY"),
        })
    }
}

enum SpecFormat {
    AUTO(kw::AUTO),
    IETF(kw::IETF),
}

impl Parse for SpecFormat {
    fn parse(buffer: &ParseBuffer) -> Result<Self, syn::Error> {
        let _level_token: kw::level = buffer.parse()?;
        let _eq_token: Token![=] = buffer.parse()?;

        let l = buffer.lookahead1();
        if l.peek(kw::IETF) {
            Ok(Self::IETF(buffer.parse()?))
        } else if l.peek(kw::AUTO) {
            Ok(Self::AUTO(buffer.parse()?))
        } else {
            Err(l.error())
        }
    }
}

impl ToTokens for SpecFormat {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        tokens.extend(match self {
            Self::AUTO(_) => quote!(b"AUTO"),
            Self::IETF(_) => quote!(b"IETF"),
        })
    }
}

fn get_doc(attr: &syn::Attribute) -> Result<String, syn::Error> {
    let message = "expected #[doc = \"...\"]";
    if !attr.path.is_ident("doc") {
        return Err(syn::Error::new_spanned(attr, message));
    }

    match attr.parse_meta()? {
        syn::Meta::NameValue(syn::MetaNameValue {
            lit: syn::Lit::Str(lit_str),
            ..
        }) => Ok(lit_str.value()),
        _ => Err(syn::Error::new_spanned(attr, message)),
    }
}

fn annotation(spec: SpecRef, item: Item, annotation: TokenStream2) -> TokenStream2 {
    let item_tokens = quote!(#item);
    let code = item_tokens.to_string();

    let (name, item_span) = match &item {
        Item::Const(item) => (
            format!("__COMPLIANCE_const_{}", item.ident),
            item.const_token.span(),
        ),
        Item::Enum(item) => (
            format!("__COMPLIANCE_enum_{}", item.ident),
            item.enum_token.span(),
        ),
        Item::ExternCrate(item) => (
            format!("__COMPLIANCE_extern_crate_{}", item.ident),
            item.extern_token.span(),
        ),
        Item::Fn(item) => (
            format!("__COMPLIANCE_fn_{}", item.sig.ident),
            item.sig.span(),
        ),
        Item::ForeignMod(_item) => (
            format!("__COMPLIANCE_foreign_mod_{}", fnv(code.as_bytes())),
            item.span(),
        ),
        Item::Impl(item) => (
            format!("__COMPLIANCE_impl_{}", fnv(code.as_bytes())),
            item.impl_token.span(),
        ),
        Item::Macro(item) => (
            format!("__COMPLIANCE_macro_{}", fnv(code.as_bytes())),
            item.span(),
        ),
        Item::Macro2(item) => (
            format!("__COMPLIANCE_macro2_{}", item.ident),
            item.macro_token.span(),
        ),
        Item::Mod(item) => (
            format!("__COMPLIANCE_mod_{}", item.ident),
            item.mod_token.span(),
        ),
        Item::Static(item) => (
            format!("__COMPLIANCE_static_{}", item.ident),
            item.static_token.span(),
        ),
        Item::Struct(item) => (
            format!("__COMPLIANCE_struct_{}", item.ident),
            item.struct_token.span(),
        ),
        Item::Trait(item) => (
            format!("__COMPLIANCE_trait_{}", item.ident),
            item.trait_token.span(),
        ),
        Item::TraitAlias(item) => (
            format!("__COMPLIANCE_trait_alias_{}", item.ident),
            item.trait_token.span(),
        ),
        Item::Type(item) => (
            format!("__COMPLIANCE_type_{}", item.ident),
            item.type_token.span(),
        ),
        Item::Union(item) => (
            format!("__COMPLIANCE_union_{}", item.ident),
            item.union_token.span(),
        ),
        Item::Use(item) => (
            format!("__COMPLIANCE_use_{}", fnv(code.as_bytes())),
            item.use_token.span(),
        ),
        Item::Verbatim(item) => (
            format!("__COMPLIANCE_verbatim_{}", fnv(code.as_bytes())),
            item.span(),
        ),
        _ => (
            format!("__COMPLIANCE_unknown_{}", fnv(code.as_bytes())),
            item.span(),
        ),
    };

    let ident = Ident::new(&name, item_span);

    let mut chunks = quote!(&#VERSION.to_le_bytes(),);
    let mut fields = quote!();

    macro_rules! chunk {
        ($ident:ident, $value:expr) => {{
            Field {
                name: stringify!($ident),
                value: $value,
            }
            .to_tokens(&mut fields);

            chunks.extend(quote!(
                stringify!($ident).as_bytes(),
                &($ident.len() as u32).to_le_bytes(),
                $ident,
            ));
        }};
    }

    chunk!(spec, spec.source);
    if let Some(level) = &spec.level {
        chunk!(slvl, level);
    }
    if let Some(format) = &spec.format {
        chunk!(sfmt, format);
    }
    chunk!(quot, StrAsBytes(spec.quote));
    chunk!(anno, annotation);
    chunk!(alin, quote!(&line!().to_le_bytes()));
    chunk!(acol, quote!(&column!().to_le_bytes()));
    chunk!(file, quote_spanned!(item_span=> file!().as_bytes()));
    chunk!(
        mand,
        quote_spanned!(item_span=> env!("CARGO_MANIFEST_DIR").as_bytes())
    );
    chunk!(path, quote_spanned!(item_span=> module_path!().as_bytes()));
    chunk!(ilin, quote_spanned!(item_span=> &line!().to_le_bytes()));
    chunk!(icol, quote_spanned!(item_span=> &column!().to_le_bytes()));

    let chunks = Chunks(chunks);

    let section = quote!(
        #[cfg(compliance)]
        #[used]
        #[allow(non_upper_case_globals, clippy::string_lit_as_bytes)]
        static #ident: () = {
            #fields
            #chunks
        };
    );

    match item {
        Item::Const(mut item) => {
            let expr = &item.expr;
            item.expr = parse_quote!({
                #section
                #expr
            });
            quote!(#item)
        }
        Item::Fn(item) => {
            let syn::ItemFn {
                attrs,
                vis,
                sig,
                block,
            } = item;
            quote!(#(#attrs)* #vis #sig {
                #section
                #block
            })
        }
        Item::Static(mut item) => {
            let expr = &item.expr;
            item.expr = parse_quote!({
                #section
                #expr
            });
            quote!(#item)
        }
        _ => quote!(#item_tokens #section),
    }
}

struct Field<Value> {
    name: &'static str,
    value: Value,
}

impl<Value: ToTokens> ToTokens for Field<Value> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let name = self.name.as_bytes();
        assert_eq!(name.len(), 4);

        let value = &self.value;
        let ident = Ident::new(self.name, value.span());

        tokens.extend(quote!(
            const #ident: &'static [u8] = #value;
        ));
    }
}

struct StrAsBytes<Value>(Value);

impl<Value: ToTokens> ToTokens for StrAsBytes<Value> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let value = &self.0;
        tokens.extend(quote!({
            const VALUE: &'static str = #value;
            VALUE.as_bytes()
        }));
    }
}

struct Chunks<Value>(Value);

impl<Value: ToTokens> ToTokens for Chunks<Value> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let chunks = &self.0;
        tokens.extend(quote!({
            const VALUE: &[&[u8]] = &[#chunks];
            const LEN: usize = {
                let mut i = 0usize;
                let mut len = 0usize;
                loop {
                    if i < VALUE.len() {
                        len += VALUE[i].len();
                        i += 1;
                    } else {
                        break;
                    }
                }
                len
            };

            #[used]
            #[cfg_attr(
                any(target_os = "linux", target_os = "android"),
                link_section = ".note.compliance"
            )]
            #[cfg_attr(target_os = "freebsd", link_section = ".note.compliance")]
            #[cfg_attr(
                any(target_os = "macos", target_os = "ios"),
                link_section = "__DATA,__compliance"
            )]
            #[cfg_attr(windows, link_section = ".debug_compliance")]
            static COMPLIANCE: [u8; LEN + 4] = {
                let mut output = [0u8; LEN + 4];
                let len_prefix = (LEN as u32).to_le_bytes();
                output[0] = len_prefix[0];
                output[1] = len_prefix[1];
                output[2] = len_prefix[2];
                output[3] = len_prefix[3];

                let mut dest = 4usize;
                let mut i = 0usize;
                loop {
                    if i < VALUE.len() {
                        let mut src = 0usize;
                        let value = &VALUE[i];

                        loop {
                            if src < value.len() {
                                output[dest] = value[src];
                                src += 1;
                                dest += 1;
                            } else {
                                break;
                            }
                        }

                        i += 1;
                    } else {
                        break;
                    }
                }
                output
            };
        }));
    }
}

fn fnv<H: core::hash::Hash + ?Sized>(value: &H) -> u64 {
    let mut hasher = fnv::FnvHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}
