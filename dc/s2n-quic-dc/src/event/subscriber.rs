// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::event::{self, IntoEvent as _};
use s2n_quic_core::time::Timestamp;

pub struct Subscriber<Sub>
where
    Sub: event::Subscriber,
{
    pub subscriber: Sub,
    pub context: Sub::ConnectionContext,
}

impl<Sub> Subscriber<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    pub fn publisher(&self, timestamp: Timestamp) -> event::ConnectionPublisherSubscriber<'_, Sub> {
        event::ConnectionPublisherSubscriber::new(
            event::builder::ConnectionMeta {
                id: 0,
                timestamp: timestamp.into_event(),
            },
            0,
            &self.subscriber,
            &self.context,
        )
    }

    #[inline]
    pub fn endpoint_publisher(
        &self,
        timestamp: Timestamp,
    ) -> event::EndpointPublisherSubscriber<'_, Sub> {
        event::EndpointPublisherSubscriber::new(
            event::builder::EndpointMeta {
                timestamp: timestamp.into_event(),
            },
            None,
            &self.subscriber,
        )
    }
}
