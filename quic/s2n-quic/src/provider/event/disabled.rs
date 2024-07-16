// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::provider::event::{ConnectionInfo, ConnectionMeta};

#[derive(Debug, Default)]
pub struct Provider;

impl super::Provider for Provider {
    type Subscriber = Subscriber;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Subscriber, Self::Error> {
        Ok(Subscriber)
    }
}

#[derive(Debug, Default)]
pub struct Subscriber;

impl super::Subscriber for Subscriber {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &ConnectionMeta,
        _info: &ConnectionInfo,
    ) -> Self::ConnectionContext {
    }
}
