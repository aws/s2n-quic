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
        quote!(u64),
        quote!(.as_u64()),
    );

    let mut bool_counters = Registry::new(
        quote!(bool_counters),
        quote!(register_bool_counter),
        format!("{}__event__counter__bool", output.crate_name),
        quote!(bool),
        quote!(),
    );
    bool_counters.registry_type = RegistryType::BoolCounter;

    let mut nominal_counters = Registry::new(
        quote!(nominal_counters),
        quote!(register_nominal_counter),
        format!("{}__event__counter__nominal", output.crate_name),
        quote!(u64),
        quote!(.as_u64()),
    );
    nominal_counters.registry_type = RegistryType::NominalCounter {
        nominal_offsets: quote!(nominal_offsets),
    };

    let mut measures = Registry::new(
        quote!(measures),
        quote!(register_measure),
        format!("{}__event__measure", output.crate_name),
        quote!(u64),
        quote!(.as_u64()),
    );
    let mut gauges = Registry::new(
        quote!(gauges),
        quote!(register_gauge),
        format!("{}__event__gauge", output.crate_name),
        quote!(u64),
        quote!(.as_u64()),
    );
    let mut timers = Registry::new(
        quote!(timers),
        quote!(register_timer),
        format!("{}__event__timer", output.crate_name),
        quote!(core::time::Duration),
        quote!(.as_duration()),
    );

    let units_none = Ident::new("None", Span::call_site());
    let units_duration = Ident::new("Duration", Span::call_site());

    for event in events {
        let ident = &event.ident;
        let snake = event.ident_snake();
        let function = event.function();

        let mut on_event = quote!();

        let count_info = &info.push(&snake, &units_none);

        let count_id = counters.push(count_info, None);

        on_event.extend(quote!(
            self.count(#count_info, #count_id, 1usize);
        ));

        for field in &event.fields {
            let metrics = [
                (quote!(count), &field.attrs.counter, &mut counters, None),
                (
                    quote!(count_nominal),
                    &field.attrs.nominal_counter,
                    &mut nominal_counters,
                    Some(&field.ty),
                ),
                (quote!(measure), &field.attrs.measure, &mut measures, None),
                (quote!(gauge), &field.attrs.gauge, &mut gauges, None),
            ];

            for (function, list, target, field_ty) in metrics {
                let borrow = if field_ty.is_some() {
                    quote!(&)
                } else {
                    quote!()
                };
                for metric in list {
                    let name = format!("{snake}.{}", metric.name.value());
                    let units = metric.unit.as_ref().unwrap_or(&units_none);
                    let info = &info.push(&name, units);
                    let id = target.push(info, field_ty);

                    let field = field.ident.as_ref().unwrap();
                    on_event.extend(quote!(
                        self.#function(#info, #id, #borrow event.#field);
                    ));
                }
            }

            let metrics = [
                (
                    quote!(time),
                    &field.attrs.timer,
                    &mut timers,
                    &units_duration,
                ),
                (
                    quote!(count_bool),
                    &field.attrs.bool_counter,
                    &mut bool_counters,
                    &units_none,
                ),
            ];

            for (function, list, target, units) in metrics {
                for metric in list {
                    let name = format!("{snake}.{}", metric.name.value());
                    let info = &info.push(&name, units);
                    let id = target.push(info, None);

                    let field = field.ident.as_ref().unwrap();
                    on_event.extend(quote!(
                        self.#function(#info, #id, event.#field);
                    ))
                }
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
    let bool_counters_init = bool_counters.init();
    let bool_counters_probes = bool_counters.probe();
    let bool_counters_len = bool_counters.len;
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
                BoolRecorder,
                NominalRecorder,
                Info,
                info::{self, Str},
                Metric,
                AsVariant,
                Units,
            },
            api,
            self
        };

        static INFO: &[Info; #info_len] = &[#info];

        pub struct Subscriber<R: Registry> {
            #[allow(dead_code)]
            counters: Box<[R::Counter; #counters_len]>,
            #[allow(dead_code)]
            bool_counters: Box<[R::BoolCounter; #bool_counters_len]>,
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
                let mut bool_counters = Vec::with_capacity(#bool_counters_len);
                let mut nominal_offsets = Vec::with_capacity(#nominal_counters_len);
                let mut nominal_counters = Vec::with_capacity(#nominal_counters_len);
                let mut measures = Vec::with_capacity(#measures_len);
                let mut gauges = Vec::with_capacity(#gauges_len);
                let mut timers = Vec::with_capacity(#timers_len);

                #counters_init
                #bool_counters_init
                #nominal_counters_init
                #measures_init
                #gauges_init
                #timers_init

                Self {
                    counters: counters.try_into().unwrap_or_else(|_| panic!("invalid len")),
                    bool_counters: bool_counters.try_into().unwrap_or_else(|_| panic!("invalid len")),
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
            fn count<T: Metric>(&self, info: usize, id: usize, value: T) {
                let info = &INFO[info];
                let counter = &self.counters[id];
                counter.record(info, value);
            }

            /// Returns all of the registered bool counters
            #[inline]
            pub fn bool_counters(&self) -> impl Iterator<Item = (&'static Info, &R::BoolCounter)> + '_ {
                #bool_counters
            }

            #[allow(dead_code)]
            #[inline(always)]
            fn count_bool(&self, info: usize, id: usize, value: bool) {
                let info = &INFO[info];
                let counter = &self.bool_counters[id];
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
            fn count_nominal<T: AsVariant>(&self, info: usize, id: usize, value: &T) {
                let info = &INFO[info];
                let idx = self.nominal_offsets[id] + value.variant_idx();
                let counter = &self.nominal_counters[idx];
                counter.record(info, value.as_variant(), 1usize);
            }

            /// Returns all of the registered measures
            #[inline]
            pub fn measures(&self) -> impl Iterator<Item = (&'static Info, &R::Measure)> + '_ {
                #measures
            }

            #[allow(dead_code)]
            #[inline(always)]
            fn measure<T: Metric>(&self, info: usize, id: usize, value: T) {
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
            fn gauge<T: Metric>(&self, info: usize, id: usize, value: T) {
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
        use crate::event::metrics::aggregate::{
            self,
            Recorder as MetricRecorder,
            NominalRecorder,
            BoolRecorder,
            Info,
            info
        };

        mod counter {
            #counters_probes

            pub mod bool {
                #bool_counters_probes
            }

            pub mod nominal {
                #nominal_counters_probes
            }
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
            type BoolCounter = counter::bool::Recorder;
            type NominalCounter = counter::nominal::Recorder;
            type Measure = measure::Recorder;
            type Gauge = gauge::Recorder;
            type Timer = timer::Recorder;

            #[inline]
            fn register_counter(&self, info: &'static Info) -> Self::Counter {
                counter::Recorder::new(info)
            }

            #[inline]
            fn register_bool_counter(&self, info: &'static Info) -> Self::BoolCounter {
                counter::bool::Recorder::new(info)
            }

            #[inline]
            fn register_nominal_counter(&self, info: &'static Info, variant: &'static info::Variant) -> Self::NominalCounter {
                counter::nominal::Recorder::new(info, variant)
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
    pub fn push(&mut self, name: impl AsRef<str>, units: &Ident) -> Info {
        let id = self.len;
        self.len += 1;

        let name = name.as_ref();
        let name_t = new_str(name);

        let entry = quote!(
            info::Builder {
                id: #id,
                name: #name_t,
                units: Units::#units,
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

enum RegistryType {
    Basic,
    BoolCounter,
    NominalCounter { nominal_offsets: TokenStream },
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
    registry_type: RegistryType,
    metric_ty: TokenStream,
    as_metric: TokenStream,
}

impl Registry {
    pub fn new(
        dest: TokenStream,
        register: TokenStream,
        probe_path: String,
        metric_ty: TokenStream,
        as_metric: TokenStream,
    ) -> Self {
        Self {
            len: 0,
            dest,
            register,
            init: quote!(),
            probe_path,
            probe_new: quote!(),
            probe_defs: quote!(),
            entries: quote!(),
            registry_type: RegistryType::Basic,
            metric_ty,
            as_metric,
        }
    }

    pub fn init(&mut self) -> TokenStream {
        if matches!(self.registry_type, RegistryType::NominalCounter { .. }) {
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
                define!(
                    extern "probe" {
                        #probe_defs
                    }
                );
            )
        };

        let metric_ty = &self.metric_ty;
        let as_metric = &self.as_metric;

        match self.registry_type {
            RegistryType::Basic => {
                quote!(
                    #![allow(non_snake_case)]

                    use super::*;
                    use crate::event::metrics::aggregate::Metric;

                    pub struct Recorder(fn(#metric_ty));

                    impl Recorder {
                        pub(crate) fn new(info: &'static Info) -> Self {
                            #probe_new
                        }
                    }

                    impl MetricRecorder for Recorder {
                        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
                            (self.0)(value #as_metric);
                        }
                    }

                    #probe_defs
                )
            }
            RegistryType::BoolCounter => {
                quote!(
                    #![allow(non_snake_case)]

                    use super::*;

                    pub struct Recorder(fn(#metric_ty));

                    impl Recorder {
                        pub(crate) fn new(info: &'static Info) -> Self {
                            #probe_new
                        }
                    }

                    impl BoolRecorder for Recorder {
                        fn record(&self, _info: &'static Info, value: bool) {
                            (self.0)(value #as_metric);
                        }
                    }

                    #probe_defs
                )
            }
            RegistryType::NominalCounter { .. } => {
                quote!(
                    #![allow(non_snake_case)]

                    use super::*;
                    use crate::event::metrics::aggregate::Metric;

                    pub struct Recorder(fn(#metric_ty, u64, &info::Str));

                    impl Recorder {
                        pub(crate) fn new(info: &'static Info, _variant: &'static info::Variant) -> Self {
                            #probe_new
                        }
                    }

                    impl NominalRecorder for Recorder {
                        fn record<T: Metric>(&self, _info: &'static Info, variant: &'static info::Variant, value: T) {
                            (self.0)(value #as_metric, variant.id as _, variant.name);
                        }
                    }

                    #probe_defs
                )
            }
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

        let metric_ty = &self.metric_ty;

        match &self.registry_type {
            RegistryType::Basic | RegistryType::BoolCounter => {
                self.init.extend(quote!(
                    #dest.push(registry.#register(&INFO[#info]));
                ));

                self.entries.extend(quote!(
                    #id => (&INFO[#info], entry),
                ));

                self.probe_defs.extend(quote!(
                    #[link_name = #link_name]
                    fn #probe(value: #metric_ty);
                ));
            }
            RegistryType::NominalCounter { nominal_offsets } => {
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
                    fn #probe(value: #metric_ty, variant: u64, variant_name: &info::Str);
                ));
            }
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

        let dest = if let RegistryType::NominalCounter { nominal_offsets } = &self.registry_type {
            nominal_offsets
        } else {
            &self.dest
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
