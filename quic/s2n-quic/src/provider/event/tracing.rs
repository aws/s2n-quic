// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::provider::event::{self, ConnectionInfo, ConnectionMeta, Event, Meta};
use tracing::debug;

#[derive(Debug, Default)]
pub struct Provider;

impl super::Provider for Provider {
    type Subscriber = Subscriber;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Subscriber, Self::Error> {
        Ok(Subscriber)
    }
}

pub struct Subscriber;

// TODO we should implement Display for Events or maybe opt into serde as a feature
impl super::Subscriber for Subscriber {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &ConnectionMeta,
        _info: &ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_event<M: Meta, E: Event>(&mut self, meta: &M, event: &E) {
        debug!(
            "{:?} {:?} {:?}",
            meta.subject(),
            meta.timestamp().duration_since_start(),
            event,
        );
    }
}
