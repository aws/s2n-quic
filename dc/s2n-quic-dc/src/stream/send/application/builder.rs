// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{
        runtime,
        send::application::{Inner, Writer},
        shared::ArcShared,
        socket,
    },
};

pub struct Builder<Sub>
where
    Sub: event::Subscriber,
{
    runtime: runtime::ArcHandle<Sub>,
}

impl<Sub> Builder<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    pub fn new(runtime: runtime::ArcHandle<Sub>) -> Self {
        Self { runtime }
    }

    #[inline]
    pub fn build(self, shared: ArcShared<Sub>, sockets: socket::ArcApplication) -> Writer<Sub> {
        let Self { runtime } = self;
        Writer(Box::new(Inner {
            shared,
            sockets,
            queue: Default::default(),
            pacer: Default::default(),
            status: Default::default(),
            runtime,
        }))
    }
}
