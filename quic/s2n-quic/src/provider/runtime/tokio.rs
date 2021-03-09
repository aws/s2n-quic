// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::provider::runtime;
use core::{future::Future, time::Duration};

#[derive(Debug)]
pub struct Provider {
    handle: Option<tokio::runtime::Handle>,
}

impl Default for Provider {
    fn default() -> Self {
        Self { handle: None }
    }
}

impl runtime::Provider for Provider {
    type Environment = Environment;
    type Error = tokio::runtime::TryCurrentError;

    fn start<Start, Fut>(self, start: Start) -> Result<(), Self::Error>
    where
        Start: FnOnce(Self::Environment) -> Fut,
        Fut: 'static + Future<Output = ()> + Send,
    {
        let handle = if let Some(handle) = self.handle {
            handle
        } else {
            tokio::runtime::Handle::try_current()?
        };

        handle.enter(move || tokio::spawn(start(Environment)));

        Ok(())
    }
}

impl runtime::TryInto for tokio::runtime::Handle {
    type Provider = Provider;
    type Error = core::convert::Infallible;

    fn try_into(self) -> Result<Self::Provider, Self::Error> {
        Ok(Provider { handle: Some(self) })
    }
}

#[derive(Debug)]
pub struct Environment;

impl super::Environment for Environment {
    type Delay = tokio::time::Delay;

    fn delay(&self, duration: Duration) -> Self::Delay {
        tokio::time::delay_for(duration)
    }
}
