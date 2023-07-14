// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use core::future::Future;
use tokio::task::JoinSet;

pub struct Limiter<Output> {
    credits: u64,
    tasks: JoinSet<Output>,
}

impl<Output: 'static + Send> Limiter<Output> {
    pub fn new(credits: u64) -> Self {
        assert_ne!(credits, 0);
        Self {
            credits,
            tasks: JoinSet::new(),
        }
    }

    pub async fn spawn<F>(&mut self, f: F) -> Option<Result<Output>>
    where
        F: 'static + Future<Output = Output> + Send,
    {
        let prev = if self.credits == 0 {
            self.join_next().await
        } else {
            None
        };

        self.credits -= 1;

        self.tasks.spawn(f);

        prev
    }

    pub async fn join_next(&mut self) -> Option<Result<Output>> {
        let res = self.tasks.join_next().await;
        self.credits += 1;
        res.map(|res| res.map_err(|err| err.into()))
    }
}
