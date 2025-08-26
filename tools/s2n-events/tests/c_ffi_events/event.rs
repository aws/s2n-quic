// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use s2n_quic_core::event::{Event, IntoEvent, Timestamp};

#[cfg(any(test, feature = "testing"))]
use s2n_quic_core::event::snapshot;

pub trait Meta: core::fmt::Debug {
    fn subject(&self) -> api::Subject;
}

impl Meta for api::ConnectionMeta {
    fn subject(&self) -> api::Subject {
        builder::Subject::Connection { id: self.id }.into_event()
    }
}

impl Meta for api::EndpointMeta {
    fn subject(&self) -> api::Subject {
        builder::Subject::Endpoint {}.into_event()
    }
}

mod generated;
pub use generated::*;
pub use s2n_quic_core::time::Duration;

pub mod metrics {
    pub use crate::event::generated::metrics::*;
    pub use s2n_quic_core::event::metrics::Recorder;

    pub mod aggregate {
        pub use crate::event::generated::metrics::aggregate::*;
        pub use s2n_quic_core::event::metrics::aggregate::{
            info, AsVariant, BoolRecorder, Info, Metric, NominalRecorder, Recorder, Registry, Units,
        };

        pub mod probe {
            pub use crate::event::generated::metrics::probe::*;
            pub use s2n_quic_core::event::metrics::aggregate::probe::dynamic;
        }
    }
}
