// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    parser::{File, Subject},
    Output,
};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};

fn new_str(value: impl AsRef<str>) -> TokenStream {
    let value_c = format!("{}\0", value.as_ref());
    quote!(Str::new(#value_c))
}

pub fn emit(output: &Output, files: &[File]) -> TokenStream {
    let events = files
        .iter()
        .flat_map(|file| file.structs.iter())
        .filter(|s| s.attrs.event_name.is_some())
        .filter(|s| s.attrs.allow_deprecated.is_empty());

    let mode = &output.mode;

    let receiver = output.mode.receiver();
    let s2n_quic_core_path = &output.s2n_quic_core_path;

    let mut subscriber = quote!();

    let mut info = InfoList::default();
    let mut counters = Registry::new(
        quote!(counters),
        quote!(register_counter),
        format!("{}__event__counter", output.crate_name),
    );

    let mut nominal_counters = Registry::new(
        quote!(nominal_counters),
        quote!(register_nominal_counter),
        format!("{}__event__nominal_counter", output.crate_name),
    );
    nominal_counters.nominal_offsets = quote!(nominal_offsets);

    let mut measures = Registry::new(
        quote!(measures),
        quote!(register_measure),
        format!("{}__event__measure", output.crate_name),
    );
    let mut gauges = Registry::new(
        quote!(gauges),
        quote!(register_gauge),
        format!("{}__event__gauge", output.crate_name),
    );
    let mut timers = Registry::new(
        quote!(timers),
        quote!(register_timer),
        format!("{}__event__timer", output.crate_name),
    );

    for event in events {
        let ident = &event.ident;
        let snake = event.ident_snake();
        let function = event.function();

        let mut on_event = quote!();

        let count_info = &info.push(&snake, "");

        let count_id = counters.push(count_info, None);

        on_event.extend(quote!(
            self.count(#count_info, #count_id, 1);
        ));

        for field in &event.fields {
            let entries = [
                (quote!(count), &field.attrs.counter, &mut counters),
                (quote!(measure), &field.attrs.measure, &mut measures),
                (quote!(gauge), &field.attrs.gauge, &mut gauges),
            ];

            for (function, list, target) in entries {
                for metric in list {
                    let name = format!("{snake}.{}", metric.name.value());
                    let units = metric.unit.as_ref().map(|v| v.value()).unwrap_or_default();
                    let info = &info.push(&name, &units);
                    let id = target.push(info, None);

                    let field = field.ident.as_ref().unwrap();
                    on_event.extend(quote!(
                        self.#function(#info, #id, event.#field.as_metric(#units));
                    ));
                }
            }

            for metric in &field.attrs.timer {
                let name = format!("{snake}.{}", metric.name.value());
                let units = "us";
                let info = &info.push(&name, units);
                let id = timers.push(info, None);

                let field = field.ident.as_ref().unwrap();
                on_event.extend(quote!(
                    self.time(#info, #id, event.#field.as_metric(#units));
                ))
            }

            for metric in &field.attrs.nominal_counter {
                let name = format!("{snake}.{}", metric.name.value());
                let units = metric.unit.as_ref().map(|v| v.value()).unwrap_or_default();
                let info = &info.push(&name, units);
                let id = nominal_counters.push(info, Some(&field.ty));

                let field = field.ident.as_ref().unwrap();
                on_event.extend(quote!(
                    self.nominal_count(#info, #id, &event.#field);
                ));
            }
        }

        match event.attrs.subject {
            Subject::Connection => {
                subscriber.extend(quote!(
                    #[inline]
                    fn #function(
                        &#receiver self,
                        context: &#receiver Self::ConnectionContext,
                        meta: &api::ConnectionMeta,
                        event: &api::#ident
                    ) {
                        #on_event
                        let _ = context;
                        let _ = meta;
                        let _ = event;
                    }
                ));
            }
            Subject::Endpoint => {
                subscriber.extend(quote!(
                    #[inline]
                    fn #function(
                        &#receiver self,
                        meta: &api::EndpointMeta,
                        event: &api::#ident
                    ) {
                        #on_event
                        let _ = event;
                        let _ = meta;
                    }
                ));
            }
        }
    }

    let counters_init = counters.init();
    let counters_probes = counters.probe();
    let counters_len = counters.len;
    let nominal_counters_init = nominal_counters.init();
    let nominal_counters_probes = nominal_counters.probe();
    let nominal_counters_len = nominal_counters.len;
    let measures_init = measures.init();
    let measures_probes = measures.probe();
    let measures_len = measures.len;
    let gauges_init = gauges.init();
    let gauges_probes = gauges.probe();
    let gauges_len = gauges.len;
    let timers_init = timers.init();
    let timers_probes = timers.probe();
    let timers_len = timers.len;
    let info_len = info.len;
    let mut imports = quote!();

    if !output.feature_alloc.is_empty() {
        imports.extend(quote!(
            use alloc::{vec::Vec, boxed::Box};
        ));
    }

    let tokens = quote!(
        #imports
        use crate::event::{
            metrics::aggregate::{
                Registry,
                Recorder,
                NominalRecorder,
                Info,
                info::{self, Str},
                AsMetric as _,
                AsVariant,
            },
            api,
            self
        };

        static INFO: &[Info; #info_len] = &[#info];

        pub struct Subscriber<R: Registry> {
            #[allow(dead_code)]
            counters: Box<[R::Counter; #counters_len]>,
            #[allow(dead_code)]
            nominal_counters: Box<[R::NominalCounter]>,
            #[allow(dead_code)]
            nominal_offsets: Box<[usize; #nominal_counters_len]>,
            #[allow(dead_code)]
            measures: Box<[R::Measure; #measures_len]>,
            #[allow(dead_code)]
            gauges: Box<[R::Gauge; #gauges_len]>,
            #[allow(dead_code)]
            timers: Box<[R::Timer; #timers_len]>,
            #[allow(dead_code)]
            registry: R,
        }

        impl<R: Registry + Default> Default for Subscriber<R> {
            fn default() -> Self {
                Self::new(R::default())
            }
        }

        impl<R: Registry> Subscriber<R> {
            /// Creates a new subscriber with the given registry
            ///
            /// # Note
            ///
            /// All of the recorders are registered on initialization and cached for the lifetime
            /// of the subscriber.
            #[allow(unused_mut)]
            #[inline]
            pub fn new(registry: R) -> Self {
                let mut counters = Vec::with_capacity(#counters_len);
                let mut nominal_offsets = Vec::with_capacity(#nominal_counters_len);
                let mut nominal_counters = Vec::with_capacity(#nominal_counters_len);
                let mut measures = Vec::with_capacity(#measures_len);
                let mut gauges = Vec::with_capacity(#gauges_len);
                let mut timers = Vec::with_capacity(#timers_len);

                #counters_init
                #nominal_counters_init
                #measures_init
                #gauges_init
                #timers_init

                Self {
                    counters: counters.try_into().unwrap_or_else(|_| panic!("invalid len")),
                    nominal_counters: nominal_counters.into(),
                    nominal_offsets: nominal_offsets.try_into().unwrap_or_else(|_| panic!("invalid len")),
                    measures: measures.try_into().unwrap_or_else(|_| panic!("invalid len")),
                    gauges: gauges.try_into().unwrap_or_else(|_| panic!("invalid len")),
                    timers: timers.try_into().unwrap_or_else(|_| panic!("invalid len")),
                    registry,
                }
            }

            /// Returns all of the registered counters
            #[inline]
            pub fn counters(&self) -> impl Iterator<Item = (&'static Info, &R::Counter)> + '_ {
                #counters
            }

            #[allow(dead_code)]
            #[inline(always)]
            fn count(&self, info: usize, id: usize, value: u64) {
                let info = &INFO[info];
                let counter = &self.counters[id];
                counter.record(info, value);
            }

            /// Returns all of the registered nominal counters
            #[inline]
            pub fn nominal_counters(&self) -> impl Iterator<Item = (&'static Info, &[R::NominalCounter], &[info::Variant])> + '_ {
                use api::*;
                #nominal_counters
            }

            #[allow(dead_code)]
            #[inline(always)]
            fn nominal_count<T: AsVariant>(&self, info: usize, id: usize, value: &T) {
                let info = &INFO[info];
                let idx = self.nominal_offsets[id] + value.variant_idx();
                let counter = &self.nominal_counters[idx];
                counter.record(info, value.as_variant(), 1);
            }

            /// Returns all of the registered measures
            #[inline]
            pub fn measures(&self) -> impl Iterator<Item = (&'static Info, &R::Measure)> + '_ {
                #measures
            }

            #[allow(dead_code)]
            #[inline(always)]
            fn measure(&self, info: usize, id: usize, value: u64) {
                let info = &INFO[info];
                let measure = &self.measures[id];
                measure.record(info, value);
            }

            /// Returns all of the registered gauges
            #[inline]
            pub fn gauges(&self) -> impl Iterator<Item = (&'static Info, &R::Gauge)> + '_ {
                #gauges
            }

            #[allow(dead_code)]
            #[inline(always)]
            fn gauge(&self, info: usize, id: usize, value: u64) {
                let info = &INFO[info];
                let gauge = &self.gauges[id];
                gauge.record(info, value);
            }

            /// Returns all of the registered timers
            #[inline]
            pub fn timers(&self) -> impl Iterator<Item = (&'static Info, &R::Timer)> + '_ {
                #timers
            }

            #[allow(dead_code)]
            #[inline(always)]
            fn time(&self, info: usize, id: usize, value: core::time::Duration) {
                let info = &INFO[info];
                let timer = &self.timers[id];
                timer.record(info, value);
            }
        }

        impl<R: Registry> event::Subscriber for Subscriber<R> {
            // TODO include some per-connection context to get aggregates for those
            type ConnectionContext = ();

            fn create_connection_context(
                &#mode self,
                _meta: &api::ConnectionMeta,
                _info: &api::ConnectionInfo
            ) -> Self::ConnectionContext {}

            #subscriber
        }
    );

    let probe = quote!(
        use #s2n_quic_core_path::probe::define;
        use crate::event::metrics::aggregate::{self, Recorder, NominalRecorder, Info, info};

        mod counter {
            #counters_probes
        }
        mod nominal_counter {
            #nominal_counters_probes
        }
        mod measure {
            #measures_probes
        }
        mod gauge {
            #gauges_probes
        }
        mod timer {
            #timers_probes
        }

        #[derive(Default)]
        pub struct Registry(());

        impl aggregate::Registry for Registry {
            type Counter = counter::Recorder;
            type NominalCounter = nominal_counter::Recorder;
            type Measure = measure::Recorder;
            type Gauge = gauge::Recorder;
            type Timer = timer::Recorder;

            #[inline]
            fn register_counter(&self, info: &'static Info) -> Self::Counter {
                counter::Recorder::new(info)
            }

            #[inline]
            fn register_nominal_counter(&self, info: &'static Info, variant: &'static info::Variant) -> Self::NominalCounter {
                nominal_counter::Recorder::new(info, variant)
            }

            #[inline]
            fn register_measure(&self, info: &'static Info) -> Self::Measure {
                measure::Recorder::new(info)
            }

            #[inline]
            fn register_gauge(&self, info: &'static Info) -> Self::Gauge {
                gauge::Recorder::new(info)
            }

            #[inline]
            fn register_timer(&self, info: &'static Info) -> Self::Timer {
                timer::Recorder::new(info)
            }
        }
    );

    output.emit("generated/metrics/aggregate.rs", tokens);
    output.emit("generated/metrics/probe.rs", probe);

    let feature_alloc = &output.feature_alloc;
    quote!(
        #feature_alloc
        pub(crate) mod aggregate;
        pub(crate) mod probe;
    )
}

#[derive(Default)]
struct InfoList {
    len: usize,
    entries: TokenStream,
}

impl InfoList {
    pub fn push(&mut self, name: impl AsRef<str>, units: impl AsRef<str>) -> Info {
        let id = self.len;
        self.len += 1;

        let name = name.as_ref();
        let name_t = new_str(name);
        let units_t = new_str(units);

        let entry = quote!(
            info::Builder {
                id: #id,
                name: #name_t,
                units: #units_t,
            }.build(),
        );

        self.entries.extend(entry);

        Info {
            idx: id,
            name: name.replace('.', "__"),
        }
    }
}

impl ToTokens for InfoList {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.entries.to_tokens(tokens);
    }
}

#[derive(Debug)]
struct Info {
    idx: usize,
    name: String,
}

impl ToTokens for Info {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.idx.to_tokens(tokens);
    }
}

struct Registry {
    len: usize,
    dest: TokenStream,
    register: TokenStream,
    init: TokenStream,
    probe_path: String,
    probe_new: TokenStream,
    probe_defs: TokenStream,
    entries: TokenStream,
    nominal_offsets: TokenStream,
}

impl Registry {
    pub fn new(dest: TokenStream, register: TokenStream, probe_path: String) -> Self {
        Self {
            len: 0,
            dest,
            register,
            init: quote!(),
            probe_path,
            probe_new: quote!(),
            probe_defs: quote!(),
            entries: quote!(),
            nominal_offsets: quote!(),
        }
    }

    pub fn init(&mut self) -> TokenStream {
        if !self.nominal_offsets.is_empty() {
            let init = &self.init;
            quote!({
                #[allow(unused_imports)]
                use api::*;
                #init
            })
        } else {
            self.init.clone()
        }
    }

    pub fn probe(&self) -> TokenStream {
        let probe_new = &self.probe_new;

        let probe_new = if probe_new.is_empty() {
            quote!(unreachable!("invalid info: {info:?}"))
        } else {
            quote!(
                match info.id {
                    #probe_new
                    _ => unreachable!("invalid info: {info:?}"),
                }
            )
        };

        let probe_defs = &self.probe_defs;
        let probe_defs = if probe_defs.is_empty() {
            quote!()
        } else {
            quote!(
                super::define!(
                    extern "probe" {
                        #probe_defs
                    }
                );
            )
        };

        if !self.nominal_offsets.is_empty() {
            quote!(
                #![allow(non_snake_case)]

                use super::{info, Info};
                use crate::event::metrics::aggregate::AsMetric;

                pub struct Recorder(fn(u64, u64, &info::Str));

                impl Recorder {
                    pub(super) fn new(info: &'static Info, _variant: &'static info::Variant) -> Self {
                        #probe_new
                    }
                }

                impl super::NominalRecorder<u64> for Recorder {
                    fn record(&self, _info: &'static Info, variant: &'static info::Variant, value: u64) {
                        (self.0)(value, variant.id as _, variant.name);
                    }
                }

                impl super::NominalRecorder<core::time::Duration> for Recorder {
                    fn record(&self, info: &'static Info, variant: &'static info::Variant, value: core::time::Duration) {
                        (self.0)(value.as_metric(info.units), variant.id as _, variant.name);
                    }
                }

                #probe_defs
            )
        } else {
            quote!(
                #![allow(non_snake_case)]

                use super::Info;
                use crate::event::metrics::aggregate::AsMetric;

                pub struct Recorder(fn(u64));

                impl Recorder {
                    pub(super) fn new(info: &'static Info) -> Self {
                        #probe_new
                    }
                }

                impl super::Recorder<u64> for Recorder {
                    fn record(&self, _info: &'static Info, value: u64) {
                        (self.0)(value);
                    }
                }

                impl super::Recorder<core::time::Duration> for Recorder {
                    fn record(&self, info: &'static Info, value: core::time::Duration) {
                        (self.0)(value.as_metric(info.units));
                    }
                }

                #probe_defs
            )
        }
    }

    pub fn push(&mut self, info: &Info, field_ty: Option<&syn::Type>) -> usize {
        let id = self.len;
        self.len += 1;

        let dest = &self.dest;
        let register = &self.register;

        let probe = &Ident::new(&info.name, Span::call_site());
        let link_name = &Ident::new(
            &format!("{}__{}", self.probe_path, info.name),
            Span::call_site(),
        );

        let info_id = info.idx;
        self.probe_new.extend(quote!(
            #info_id => Self(#probe),
        ));

        if !self.nominal_offsets.is_empty() {
            let nominal_offsets = &self.nominal_offsets;

            let field_ty = field_ty.expect("need field type for nominal");

            // trim off any generics
            let field_ty_tokens = quote!(#field_ty);
            let mut field_ty: syn::Path = syn::parse2(field_ty_tokens).unwrap();

            if let Some(syn::PathSegment { arguments, .. }) = field_ty.segments.last_mut() {
                *arguments = syn::PathArguments::None;
            }

            let variants = &quote!(<#field_ty as AsVariant>::VARIANTS);

            self.init.extend(quote!({
                let offset = #dest.len();
                let mut count = 0;

                for variant in #variants.iter() {
                    #dest.push(registry.#register(&INFO[#info], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                #nominal_offsets.push(offset);
            }));

            self.entries.extend(quote!(
                #id => {
                    let offset = *entry;
                    let variants = #variants;
                    let entries = &self.#dest[offset..offset + variants.len()];
                    (&INFO[#info], entries, variants)
                }
            ));

            self.probe_defs.extend(quote!(
                #[link_name = #link_name]
                fn #probe(value: u64, variant: u64, variant_name: &info::Str);
            ));
        } else {
            self.init.extend(quote!(
                #dest.push(registry.#register(&INFO[#info]));
            ));

            self.entries.extend(quote!(
                #id => (&INFO[#info], entry),
            ));

            self.probe_defs.extend(quote!(
                #[link_name = #link_name]
                fn #probe(value: u64);
            ));
        }

        id
    }
}

impl ToTokens for Registry {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if self.len == 0 {
            tokens.extend(quote!(core::iter::empty()));
            return;
        }

        let dest = if self.nominal_offsets.is_empty() {
            &self.dest
        } else {
            &self.nominal_offsets
        };

        let entries = &self.entries;
        tokens.extend(quote!(
            self.#dest.iter().enumerate().map(|(idx, entry)| {
                match idx {
                    #entries
                    _ => unsafe { core::hint::unreachable_unchecked() },
                }
            })
        ));
    }
}
