// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This crate handles our metrics emission and processing.
mod appender;
mod bool_counter;
mod callback;
mod counter;
mod registry;
mod rseq;
mod runtime;
mod summary;
mod task;

pub use appender::MetricsWriter;
pub use bool_counter::BoolCounter;
pub use counter::Counter;
pub use registry::Registry;
pub use summary::{logging_util_float_to_integer, Summary, SummaryInner};

pub use runtime::TaskMonitor;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Unit {
    Count,
    Microsecond,
    Byte,
    Second,
    Percent,
}

impl Unit {
    pub fn pmet_str(self) -> &'static str {
        match self {
            Unit::Count => "",
            Unit::Percent => " %",
            Unit::Microsecond => " us",
            Unit::Byte => " B",
            Unit::Second => " s",
        }
    }
}

/// `counters_for_enum` provides a convenient definition of a set of counters, one per variant of
/// an enum. As an example, this is used to report the cause of connection or packet send failure
/// based on s2n-quic connection or stream errors.
///
/// The generated code works well with `#[non_exhaustive]` enums with an automatic `Other` variant
/// added to the list.
#[macro_export]
macro_rules! counters_for_enum {
    (enum $actual:path as $name:ident { $($variant:ident,)+ }) => {
        #[derive(Clone)]
        #[allow(non_snake_case)]
        pub struct $name {
            $($variant: $crate::Counter,)+
            other: $crate::Counter,
        }

        impl $name {
            /// Create a counter set with a given `metric` and `prefix`.
            ///
            /// The `metric` is the high-level concept being aggregated -- this is usually not
            /// directly related to the type of the enum, but rather the event that may be
            /// differentiated into each of the enum sub-variants.
            ///
            /// The `prefix` is inserted before the name of each variant (or "Other") to allow
            /// multiple independent counter sets created with the `counters_for_enum!` macro to
            /// share a single overall metric, permitting aggregation across the full set.
            pub fn new(metric: &str, prefix: &str, registry: &$crate::Registry) -> Self {
                Self {
                    $($variant: registry.register_counter(
                        metric.into(),
                        Some(format!("Variant|{}-{}", prefix, stringify!($variant))),
                    ),)+
                    other: registry.register_counter(
                        metric.into(),
                        Some(format!("Variant|{}-Other", prefix)),
                    ),
                }
            }

            /// Increments the counter associated with the provided value.
            pub fn count(&self, value: &$actual) {
                use $actual::*;
                match value {
                    $($variant { .. } => self.$variant.increment(1),)+
                    _ => self.other.increment(1),
                }
            }
        }
    };
}

#[cfg(test)]
mod test;
