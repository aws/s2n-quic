// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(test, feature = "testing"))]
use s2n_quic_core::event::snapshot;

pub use s2n_quic_core::event::{Event, IntoEvent};

/// Provides metadata related to an event
pub trait Meta: core::fmt::Debug {
    /// A context from which the event is being emitted
    ///
    /// An event can occur in the context of an Endpoint or Connection
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
