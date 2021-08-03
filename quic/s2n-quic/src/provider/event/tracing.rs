// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::event::{common, Event};
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
    fn on_event<E: Event>(&mut self, meta: &common::Meta, event: &E) {
        debug!(
            group_id = meta.group_id,
            "{:?} {:?}",
            meta.timestamp.duration_since_start(),
            event
        );
    }
}
