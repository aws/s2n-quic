// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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

#[cfg(feature = "tracing")]
use s2n_quic_core::event::*;
use tracing::info;

// TODO we should implement Display for Events or maybe opt into serde as a feature
impl super::Subscriber for Subscriber {
    fn on_version_information(&mut self, event: &events::VersionInformation) {
        info!("{:?}", event);
    }

    fn on_alpn_information(&mut self, event: &events::AlpnInformation) {
        info!("{:?}", event);
    }
}
